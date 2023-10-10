use laser_setup_interface::{CameraState, Error, LaserSetup, ValveState};
use std::fmt::Debug;
use std::ops::DerefMut;
use std::sync::Arc;
use std::time::{Duration, SystemTime};
use tokio::sync::watch::{Receiver, Sender};
use tokio::sync::Mutex;

#[derive(Debug, Clone, Copy)]
pub struct LaserSetupStatus {
    pub current_frequency: f32,
    pub camera_state: CameraState,
    pub valve_state: ValveState,
    pub channel: u32,
    pub freq_offset: f32,
}

impl LaserSetupStatus {
    fn update(&mut self, ctrl: &LaserCtrl) {
        if let Some(valve) = ctrl.valve {
            self.valve_state = valve;
        }
        if let Some(camera_state) = ctrl.camera {
            self.camera_state = camera_state;
        }
        if let Some(chanel) = ctrl.channel {
            self.channel = chanel;
        }
        if let Some(freqmeter_offset) = ctrl.freqmeter_offset {
            self.freq_offset = freqmeter_offset;
        }
    }
    fn update_freq(&mut self, f: f32) {
        self.current_frequency = f;
    }
}

#[derive(Default, Clone)]
pub struct LaserCtrl {
    valve: Option<ValveState>,
    channel: Option<u32>,
    camera: Option<CameraState>,
    freqmeter_offset: Option<f32>,
}

impl laser_setup_interface::ControlState for LaserCtrl {
    fn valve(&self) -> Option<ValveState> {
        self.valve
    }

    fn channel(&self) -> Option<u32> {
        self.channel
    }

    fn camera(&self) -> Option<CameraState> {
        self.camera
    }
}

pub struct LaserSetupController {
    channels_count: u32,
    laser_setup: Arc<Mutex<LaserSetup>>,
    status_rx: Receiver<LaserSetupStatus>,
    control_tx: tokio::sync::mpsc::Sender<LaserCtrl>,
}

impl LaserSetupController {
    pub fn new<'a>(
        port: impl Into<std::borrow::Cow<'a, str>>,
        channels_count: u32,
        timeout: Duration,
        freq_meter_i2c_addr: u8,
        update_interval: Duration,
        initial_freq_offset: f32,
        emulate_center: Option<f32>,
    ) -> Self {
        let laser_setup = Arc::new(Mutex::new(LaserSetup::new(port, timeout)));

        let (status_tx, status_rx) = tokio::sync::watch::channel(LaserSetupStatus {
            current_frequency: 0.0,
            camera_state: CameraState::Close,
            valve_state: ValveState::Atmosphere,
            freq_offset: initial_freq_offset,
            channel: 0,
        });

        let (control_tx, control_rx) = tokio::sync::mpsc::channel(5);

        tokio::spawn(control_task(
            status_tx,
            control_rx,
            laser_setup.clone(),
            freq_meter_i2c_addr,
            update_interval,
            emulate_center,
            initial_freq_offset,
        ));

        Self {
            channels_count,
            laser_setup,
            status_rx,
            control_tx,
        }
    }

    /// Получить экземпляр рессивера обновленя статуса
    pub fn subscribe(&self) -> Receiver<LaserSetupStatus> {
        self.status_rx.clone()
    }

    /// Выбрать канал
    pub async fn select_channel(&mut self, channel: u32) -> Result<(), Error> {
        if channel > self.channels_count {
            return Err(Error::IoError(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                format!("Channel number out of range: {channel}"),
            )));
        }
        self.control_tx
            .send(LaserCtrl {
                channel: Some(channel),
                ..Default::default()
            })
            .await
            .ok();

        Ok(())
    }

    /// Управление камерой
    pub async fn camera_control(&mut self, state: CameraState) -> Result<(), Error> {
        self.control_tx
            .send(LaserCtrl {
                camera: Some(state),
                ..Default::default()
            })
            .await
            .ok();

        Ok(())
    }

    /// Управление вакуумным клапаном
    pub async fn valve_control(&mut self, state: ValveState) -> Result<(), Error> {
        self.control_tx
            .send(LaserCtrl {
                valve: Some(state),
                ..Default::default()
            })
            .await
            .ok();

        Ok(())
    }

    /// Установить поправку частотомера
    pub async fn set_freq_meter_offset(&mut self, offset: f32) {
        self.control_tx
            .send(LaserCtrl {
                freqmeter_offset: Some(offset),
                ..Default::default()
            })
            .await
            .ok();
    }

    /// Получить поправку частотомера
    pub fn get_freq_meter_offset(&mut self) -> f32 {
        self.status_rx.borrow().freq_offset
    }

    /// Запросить текущий канал
    pub fn current_channel(&mut self) -> u32 {
        self.status_rx.borrow().channel
    }

    /// Запросить текущую частоту
    pub fn current_frequency(&mut self) -> f32 {
        self.status_rx.borrow().current_frequency
    }

    pub async fn test_connection(&self) -> Result<(), Error> {
        self.laser_setup.lock().await.read().await?;
        Ok(())
    }
}

async fn control_task(
    tx: Sender<LaserSetupStatus>,
    mut rx: tokio::sync::mpsc::Receiver<LaserCtrl>,
    laser_setup: Arc<Mutex<LaserSetup>>,

    freq_meter_i2c_addr: u8,
    update_interval: Duration,
    emulate_center: Option<f32>,
    initial_freq_offset: f32,
) {
    const TRYS: usize = 3;

    // read curent status
    let mut current_status = {
        let mut i = 0;
        loop {
            match laser_setup.lock().await.read().await {
                Ok(status) => {
                    break LaserSetupStatus {
                        current_frequency: f32::NAN,
                        camera_state: status.camera,
                        valve_state: status.valve,
                        freq_offset: initial_freq_offset,
                        channel: status.channel,
                    };
                }
                Err(e) => {
                    i += 1;
                    if i == TRYS - 1 {
                        panic!("Can't read status: {e:?}, give up!");
                    } else {
                        tracing::error!("Can't read status: {:?}", e);
                    }
                }
            }
        }
    };

    tx.send(current_status).ok();

    loop {
        // wait for control command or timeout=update_interval
        match tokio::time::timeout(update_interval, rx.recv()).await {
            Ok(Some(ctrl)) => {
                // read control command
                for i in 0..TRYS {
                    // write control command to device
                    if let Err(e) = laser_setup.lock().await.write(&ctrl).await {
                        if i == TRYS - 1 {
                            panic!("Can't write control command: {e:?}, give up!");
                        } else {
                            tracing::error!("Can't write control command: {:?}", e);
                        }
                    } else {
                        current_status.update(&ctrl);
                        tx.send(current_status).ok();
                        break;
                    }
                }
            }
            _ => {
                // read current status
                for _ in 0..TRYS {
                    match i2c_read(
                        laser_setup.lock().await.deref_mut(),
                        freq_meter_i2c_addr,
                        0x08,
                        std::mem::size_of::<f32>(),
                    )
                    .await
                    {
                        Ok(r) => {
                            if r.len() == std::mem::size_of::<f32>() {
                                let f = if let Some(fake_freq) = emulate_center {
                                    generate_fake_freq(fake_freq)
                                } else {
                                    let byte_array: [u8; 4] = r[0..4].try_into().unwrap();
                                    f32::from_le_bytes(byte_array)
                                };

                                current_status.update_freq(f + current_status.freq_offset);
                                tx.send(current_status).ok();

                                break;
                            } else {
                                tracing::debug!("Freqmeter returned invalid data, skipping...");
                            }
                        }
                        Err(e) => {
                            tracing::error!("Can't read status: {:?}", e);
                        }
                    }
                }
            }
        }
    }
}

async fn i2c_read<'a, E: Debug, I: laser_setup_interface::I2c<Error = E>>(
    d: &'a mut I,
    dev_addr: u8,
    start_addr: u8,
    data_len: usize,
) -> Result<Vec<u8>, E> {
    let addr = [start_addr; 1];
    let mut buf = vec![0; data_len];

    let mut ops = vec![
        laser_setup_interface::Operation::Write(&addr),
        laser_setup_interface::Operation::Read(&mut buf),
    ];

    d.transaction(dev_addr, &mut ops).await?;

    Ok(buf)
}

fn generate_fake_freq(center: f32) -> f32 {
    let angle = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .unwrap()
        .as_millis() as f64;
    const A: f64 = 1.5;
    const B: f64 = 0.75;
    (center as f64 + A * angle.sin() + B * angle.cos()) as f32
}
