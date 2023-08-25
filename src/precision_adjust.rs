use std::default::Default;
use std::fmt::Debug;
use std::io::Error as IoError;
use std::ops::DerefMut;
use std::sync::Arc;
use std::time::{Duration, SystemTime};

use futures::{SinkExt, StreamExt};
use laser_setup_interface::{CameraState, ControlState, LaserSetup, ValveState};
use tokio::io::AsyncWriteExt;
use tokio::sync::Mutex;
use tokio_serial::SerialPortBuilderExt;
use tokio_util::codec::Decoder;

use crate::coordinates::{CoordiantesCalc, Side};
use crate::{gcode_codec, gcode_ctrl::GCodeCtrl};

#[derive(Debug)]
pub enum Error {
    Laser(IoError),
    LaserSetup(laser_setup_interface::Error),
    Logick(String),
}

#[derive(Default)]
pub struct LaserCtrl {
    valve: Option<ValveState>,
    channel: Option<u32>,
    camera: Option<CameraState>,
}

impl ControlState for LaserCtrl {
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

#[derive(Debug, Clone, Copy)]
pub struct PrivStatus {
    current_channel: u32,
    current_side: Side,
    current_step: u32,

    current_camera_state: CameraState,
    current_valve_state: ValveState,

    prev_freq: Option<f32>,
}

pub struct Status {
    pub current_channel: u32,
    pub current_side: Side,
    pub current_step: u32,

    pub since_start: Duration,
    pub current_frequency: f32,

    pub camera_state: CameraState,
    pub valve_state: ValveState,
}

pub struct PrecisionAdjust {
    positions: Vec<crate::config::ResonatroPlacement>,
    total_vertical_steps: u32,

    laser_setup: Arc<Mutex<LaserSetup>>,
    laser_control: tokio_util::codec::Framed<tokio_serial::SerialStream, gcode_codec::LineCodec>,
    gcode_timeout: Duration,

    freqmeter_status_rx: Mutex<Option<tokio::sync::mpsc::Receiver<(SystemTime, f32)>>>,

    status: Mutex<PrivStatus>,

    start_time: SystemTime,

    burn_laser_pump_power: f32,
    burn_laser_power: f32,
    burn_laser_frequency: u32,
    burn_laser_feedrate: f32,

    axis_config: crate::config::AxisConfig,

    freq_fifo: Option<tokio::fs::File>,

    freq_meter_i2c_addr: u8,
}

impl PrecisionAdjust {
    pub async fn with_config(config: crate::Config) -> Self {
        let laser_port = tokio_serial::new(config.laser_control_port, 1500000)
            .open_native_async()
            .unwrap();
        let laser_setup = LaserSetup::new(
            &config.laser_setup_port,
            Duration::from_millis(config.port_timeout_ms),
        );

        Self {
            laser_setup: Arc::new(Mutex::new(laser_setup)),
            total_vertical_steps: config.total_vertical_steps,

            laser_control: gcode_codec::LineCodec.framed(laser_port),
            positions: config.resonator_placement,
            gcode_timeout: Duration::from_millis(config.gcode_timeout_ms),

            freqmeter_status_rx: Mutex::new(None),

            status: Mutex::new(PrivStatus {
                current_channel: 0,
                current_side: Side::Left,
                current_step: 0,

                current_camera_state: CameraState::Close,
                current_valve_state: ValveState::Atmosphere,

                prev_freq: None,
            }),

            start_time: SystemTime::now(),

            burn_laser_pump_power: config.burn_laser_pump_power,
            burn_laser_power: config.burn_laser_power,
            burn_laser_frequency: config.burn_laser_frequency,
            burn_laser_feedrate: config.burn_laser_feedrate,

            axis_config: config.axis_config,

            freq_fifo: {
                match config.freq_fifo.as_ref() {
                    Some(freq_fifo) => {
                        let file = tokio::fs::OpenOptions::new()
                            .write(true)
                            .create(false)
                            .open(freq_fifo)
                            .await
                            .unwrap();
                        Some(file)
                    }
                    None => None,
                }
            },

            freq_meter_i2c_addr: config.freq_meter_i2c_addr,
        }
    }

    pub async fn test_connection(&mut self) -> Result<(), Error> {
        self.laser_setup
            .lock()
            .await
            .read()
            .await
            .map_err(Error::LaserSetup)?;
        self.raw_gcode("\n").await.map_err(Error::Laser)?;
        Ok(())
    }

    async fn get_gcode_result(&mut self) -> Result<(), IoError> {
        use std::io::ErrorKind;
        match tokio::time::timeout(self.gcode_timeout, self.laser_control.next()).await {
            Ok(Some(r)) => match r {
                Ok(gcode_codec::CmdResp::Ok) => Ok(()),
                Ok(gcode_codec::CmdResp::Err) => {
                    Err(IoError::new(ErrorKind::Other, "Command error"))
                }
                Err(_e) => Err(IoError::new(ErrorKind::Other, "Unexpected response")),
            },
            Ok(None) => Err(IoError::new(
                ErrorKind::UnexpectedEof,
                "Unexpected end of stream",
            )),
            Err(_e) => Err(IoError::new(ErrorKind::TimedOut, "Laser Resp timeout")),
        }
    }

    pub async fn raw_gcode(&mut self, cmd: &str) -> Result<(), IoError> {
        self.laser_control
            .send(GCodeCtrl::Raw(cmd.to_string()))
            .await?;

        self.get_gcode_result().await
    }

    pub async fn get_status(&mut self) -> Result<Status, Error> {
        let mut guard = self.freqmeter_status_rx.lock().await;
        if let Some(rx) = guard.as_mut() {
            if let Some((t, f)) = rx.recv().await {
                let mut status = self.status.lock().await;

                let e = if let Some(prev_f) = &mut status.prev_freq {
                    let v_prev_f = *prev_f;
                    if f > v_prev_f + 500.0 {
                        Err(Error::Logick(format!(
                            "Random frequency jump detected! {} -> {}",
                            prev_f, f
                        )))
                    }
                    else if (f.is_nan() || f < 1.0) && !v_prev_f.is_nan() {
                        Err(Error::Logick("Empty result".to_owned()))
                    }
                    else {
                        Ok(())
                    }
                } else {
                    status.prev_freq.replace(f);
                    Ok(())
                };

                if e.is_err() {
                    status.prev_freq = None;
                    e?;
                } else {
                    status.prev_freq = Some(f);
                }

                if let Some(fifo) = self.freq_fifo.as_mut() {
                    fifo.write_all(format!("{{ \"f\": {}}}\n", f).as_bytes())
                        .await
                        .unwrap();
                }

                Ok(Status {
                    current_channel: status.current_channel,
                    current_side: status.current_side,
                    current_step: status.current_step,
                    since_start: t.duration_since(self.start_time).unwrap(),
                    current_frequency: f,

                    camera_state: status.current_camera_state,
                    valve_state: status.current_valve_state,
                })
            } else {
                unreachable!()
            }
        } else {
            log::error!(
                "Freqmeter status channel not initialized! please call start_monitoring() first!"
            );
            return Err(Error::LaserSetup(laser_setup_interface::Error::Timeout));
        }
    }

    pub async fn start_monitoring(&mut self) -> tokio::task::JoinHandle<()> {
        let (tx, rx) = tokio::sync::mpsc::channel(1);
        self.freqmeter_status_rx.lock().await.replace(rx);

        let dev = self.laser_setup.clone();
        let freq_meter_i2c_addr = self.freq_meter_i2c_addr;

        tokio::spawn(async move {
            loop {
                let res = {
                    let mut guard = dev.lock().await;
                    Self::i2c_read(guard.deref_mut(), freq_meter_i2c_addr, 0x00, 4).await
                };

                match res {
                    Ok(r) => {
                        if r.len() == std::mem::size_of::<f32>() {
                            let byte_array: [u8; 4] = r[0..4].try_into().unwrap();
                            let f = f32::from_le_bytes(byte_array);
                            tx.send((SystemTime::now(), f)).await.ok();
                        } else {
                            log::debug!("Freqmeter returned invalid data, skipping...");
                        }
                    }
                    Err(e) => log::debug!("Freqmeter error: {:?}", e),
                }

                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
    }

    pub async fn reset(&mut self) -> Result<(), Error> {
        let a = self.burn_laser_power;
        let b = self.burn_laser_frequency;

        let status = self.current_status().await;

        let mut new_status = self
            .execute_gcode(status, move |status, _| {
                let mut commands = vec![];

                commands.push(GCodeCtrl::Reset);
                commands.push(GCodeCtrl::Setup { a, b });

                (status, commands)
            })
            .await?;

        // read current laser setup state
        let state = self
            .laser_setup
            .lock()
            .await
            .read()
            .await
            .map_err(Error::LaserSetup)?;
        {
            let mut guard = self.status.lock().await;
            new_status.current_channel = state.channel;
            new_status.current_valve_state = state.valve;
            new_status.current_camera_state = state.camera;
            *guard = new_status;
        }

        Ok(())
    }

    pub async fn select_channel(&mut self, channel: u32) -> Result<(), Error> {
        if channel >= self.positions.len() as u32 {
            return Err(Error::Logick(format!(
                "Channel {} is out of range (0 - {})!",
                channel,
                self.positions.len() - 1
            )));
        }

        {
            // disable kosa update while changing channel
            let mut guard = self.laser_setup.lock().await;

            guard
                .write(&LaserCtrl {
                    channel: Some(channel),
                    ..Default::default()
                })
                .await
                .map_err(Error::LaserSetup)?;

            // sleep 200 ms to let laser setup to change channel
            tokio::time::sleep(Duration::from_millis(200)).await;
        }

        let ax_conf = self.axis_config;
        let total_vertical_steps = self.total_vertical_steps;
        let mut status = self.current_status().await;
        status.current_channel = channel;
        let new_status = self
            .execute_gcode(status, move |mut status, workspace| {
                status.current_step = 0;
                status.current_side = Side::Left;

                let new_abs_coordinates =
                    workspace.to_abs(&ax_conf, 0, Side::Left, total_vertical_steps);
                let cmd = GCodeCtrl::G0 {
                    x: new_abs_coordinates.0,
                    y: new_abs_coordinates.1,
                };

                (status, vec![cmd])
            })
            .await?;

        self.update_status(new_status).await;

        Ok(())
    }

    pub async fn open_camera(&mut self) -> Result<(), Error> {
        {
            let mut guard = self.status.lock().await;
            guard.current_camera_state = CameraState::Open;
            guard.current_valve_state = ValveState::Atmosphere;
        }

        self.write_laser_setup(LaserCtrl {
            valve: Some(ValveState::Atmosphere),
            camera: Some(CameraState::Open),
            ..Default::default()
        })
        .await
    }

    pub async fn close_camera(&mut self, vacuum: bool) -> Result<(), Error> {
        let valve_state = {
            let mut guard = self.status.lock().await;

            if guard.current_camera_state != CameraState::Close && vacuum {
                return Err(Error::Logick(
                    "Close the camera before turn vacuum on!".to_string(),
                ));
            }

            guard.current_camera_state = CameraState::Close;
            guard.current_valve_state = if vacuum {
                ValveState::Vacuum
            } else {
                ValveState::Atmosphere
            };

            guard.current_valve_state
        };

        self.write_laser_setup(LaserCtrl {
            valve: Some(valve_state),
            camera: Some(CameraState::Close),
            ..Default::default()
        })
        .await
    }

    async fn write_laser_setup(&mut self, ctrl: LaserCtrl) -> Result<(), Error> {
        self.laser_setup
            .lock()
            .await
            .write(&ctrl)
            .await
            .map_err(Error::LaserSetup)
            .map(|_| ())
    }

    async fn execute_gcode(
        &mut self,
        status: PrivStatus,
        f: impl Fn(PrivStatus, &crate::config::ResonatroPlacement) -> (PrivStatus, Vec<GCodeCtrl>),
    ) -> Result<PrivStatus, Error> {
        let workspace = &self.positions[status.current_channel as usize];

        let (new_status, cmds) = f(status, workspace);

        for cmd in cmds {
            log::trace!("Sending {:?}...", cmd);
            self.laser_control
                .send(cmd)
                .await
                .map_err(|e| Error::Laser(e))?;

            log::trace!("Waiting conformation");
            self.get_gcode_result().await.map_err(|e| {
                log::error!("Can't setup initial position: {:?}", e);
                Error::Laser(e)
            })?;
        }

        Ok(new_status)
    }

    pub async fn step(&mut self, count: i8) -> Result<(), Error> {
        let ax_conf = self.axis_config;
        let total_vertical_steps = self.total_vertical_steps;
        let status = self.current_status().await;

        if status.current_step >= self.total_vertical_steps {
            return Err(Error::Logick("Maximum steps wriched!".to_owned()));
        }

        let new_status = self
            .execute_gcode(status, move |mut status, workspace| {
                status.current_step += count as u32;

                let new_abs_coordinates = workspace.to_abs(
                    &ax_conf,
                    status.current_step,
                    status.current_side,
                    total_vertical_steps,
                );
                let cmd = GCodeCtrl::G0 {
                    x: new_abs_coordinates.0,
                    y: new_abs_coordinates.1,
                };

                (status, vec![cmd])
            })
            .await?;

        self.update_status(new_status).await;

        Ok(())
    }

    pub async fn burn(&mut self) -> Result<(), Error> {
        let ax_conf = self.axis_config;
        let total_vertical_steps = self.total_vertical_steps;
        let burn_laser_power = self.burn_laser_pump_power;
        let f = self.burn_laser_feedrate;
        let status = self.current_status().await;

        let new_status = self
            .execute_gcode(status, move |mut status, workspace| {
                let mut commands = vec![];

                status.current_side = status.current_side.morrored();

                let new_abs_coordinates = workspace.to_abs(
                    &ax_conf,
                    status.current_step,
                    status.current_side,
                    total_vertical_steps,
                );

                let cmd = GCodeCtrl::G1 {
                    x: new_abs_coordinates.0,
                    y: new_abs_coordinates.1,
                    f,
                };

                commands.push(GCodeCtrl::M3 {
                    s: burn_laser_power,
                });
                commands.push(cmd);
                commands.push(GCodeCtrl::M5);

                (status, commands)
            })
            .await?;

        self.update_status(new_status).await;

        Ok(())
    }

    pub async fn show(
        &mut self,
        burn: bool,
        override_pump: Option<f32>,
        override_s: Option<f32>,
        override_f: Option<f32>,
    ) -> Result<(), Error> {
        const SHOW_COUNT: u32 = 2;

        let s = override_s.unwrap_or(self.burn_laser_pump_power);
        let f = override_f.unwrap_or(self.burn_laser_feedrate);
        let a = override_pump.unwrap_or(self.burn_laser_power);
        let default_a = self.burn_laser_power;
        let total_vertical_steps = self.total_vertical_steps;
        let status = self.current_status().await;
        let ax_conf = self.axis_config;

        let new_status = self
            .execute_gcode(status, move |mut status, workspace| {
                let pos2g1 = |step: u32, side: Side| -> GCodeCtrl {
                    let new_abs_coordinates =
                        workspace.to_abs(&ax_conf, step, side, total_vertical_steps);

                    GCodeCtrl::G1 {
                        x: new_abs_coordinates.0,
                        y: new_abs_coordinates.1,
                        f,
                    }
                };

                let mut commands = vec![];

                if burn {
                    commands.push(GCodeCtrl::Raw(format!("G0 A{a}")));
                    commands.push(GCodeCtrl::M3 { s });
                }

                let init_cmd = pos2g1(0, Side::Left);
                commands.push(init_cmd.clone());

                for _ in 0..SHOW_COUNT {
                    commands.push(pos2g1(0, Side::Right));
                    commands.push(pos2g1(total_vertical_steps - 1, Side::Right));
                    commands.push(pos2g1(total_vertical_steps - 1, Side::Left));

                    commands.push(init_cmd.clone()); // to init possition
                }
                status.current_side = Side::Left;
                status.current_step = 0;

                if burn {
                    commands.push(GCodeCtrl::M5);
                    commands.push(GCodeCtrl::Raw(format!("G0 A{default_a}")));
                }

                (status, commands)
            })
            .await?;

        self.update_status(new_status).await;

        Ok(())
    }

    async fn current_status(&mut self) -> PrivStatus {
        self.status.lock().await.clone()
    }

    async fn update_status(&mut self, new_status: PrivStatus) {
        let mut guard = self.status.lock().await;
        *guard = new_status;
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
}
