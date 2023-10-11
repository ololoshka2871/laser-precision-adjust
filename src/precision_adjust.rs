use std::default::Default;
use std::fmt::Debug;
use std::io::Error as IoError;
use std::ops::DerefMut;
use std::sync::Arc;
use std::time::{Duration, SystemTime};

use futures::{SinkExt, StreamExt};
use laser_setup_interface::{CameraState, ControlState, LaserSetup, ValveState};
use serde::Serialize;
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

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Error::Laser(e) => write!(f, "Laser error: {}", e),
            Error::LaserSetup(e) => write!(f, "Laser setup error: {:?}", e),
            Error::Logick(e) => write!(f, "Logick error: {}", e),
        }
    }
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

    shot_requested: bool,
}

#[derive(Debug, Clone, Copy)]
pub struct Status {
    pub current_channel: u32,
    pub current_side: Side,
    pub current_step: u32,

    pub since_start: Duration,
    pub current_frequency: f32,

    pub camera_state: CameraState,
    pub valve_state: ValveState,

    pub shot_mark: bool,
}

pub struct PrecisionAdjust {
    positions: Vec<crate::config::ResonatroPlacement>,
    total_vertical_steps: u32,

    laser_setup: Arc<Mutex<LaserSetup>>,
    laser_control: tokio_util::codec::Framed<tokio_serial::SerialStream, gcode_codec::LineCodec>,
    gcode_timeout: Duration,

    status: Arc<Mutex<PrivStatus>>,

    start_time: SystemTime,

    burn_laser_pump_power: f32,
    burn_laser_power: f32,
    burn_laser_frequency: u32,
    burn_laser_feedrate: f32,

    axis_config: crate::config::AxisConfig,

    data_log_file: Arc<Mutex<Option<tokio::fs::File>>>,

    freq_meter_i2c_addr: u8,
    freq_merer_offset: Arc<Mutex<f32>>,

    update_interval: Duration,
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

            status: Arc::new(Mutex::new(PrivStatus {
                current_channel: 0,
                current_side: Side::Left,
                current_step: 0,

                current_camera_state: CameraState::Close,
                current_valve_state: ValveState::Atmosphere,

                prev_freq: None,
                shot_requested: false,
            })),

            start_time: SystemTime::now(),

            burn_laser_pump_power: config.burn_laser_pump_power,
            burn_laser_power: config.burn_laser_power,
            burn_laser_frequency: config.burn_laser_frequency,
            burn_laser_feedrate: config.burn_laser_feedrate,

            axis_config: config.axis_config,

            data_log_file: {
                Arc::new(Mutex::new(match config.data_log_file.clone() {
                    Some(data_log_file_name) => {
                        let file = tokio::fs::OpenOptions::new()
                            .write(true)
                            .create(true)
                            .truncate(true)
                            .open({
                                let now = chrono::offset::Local::now();
                                now.format(data_log_file_name.to_str().unwrap()).to_string()
                            })
                            .await
                            .unwrap();
                        Some(file)
                    }
                    None => None,
                }))
            },

            freq_meter_i2c_addr: config.freq_meter_i2c_addr,
            freq_merer_offset: Arc::new(Mutex::new(config.freqmeter_offset)),

            update_interval: Duration::from_millis(config.update_interval_ms as u64),
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

    pub async fn start_monitoring(
        &mut self,
        emulate_center: Option<f32>,
    ) -> tokio::sync::watch::Receiver<Status> {
        let (tx, rx) = tokio::sync::watch::channel(Status {
            current_channel: 0,
            current_side: Side::Left,
            current_step: 0,
            since_start: Duration::from_millis(0),
            current_frequency: 0.0,

            camera_state: CameraState::Close,
            valve_state: ValveState::Atmosphere,

            shot_mark: false,
        });

        let dev = self.laser_setup.clone();
        let freq_meter_i2c_addr = self.freq_meter_i2c_addr;
        let update_interval = self.update_interval;

        let status = self.status.clone();
        let data_log_file = self.data_log_file.clone();
        let start_time = self.start_time;

        let freq_merer_offset = self.freq_merer_offset.clone();

        async fn update_status(
            status: &Mutex<PrivStatus>,
            f: f32,
            logfile: &Mutex<Option<tokio::fs::File>>,
            start_time: SystemTime,
        ) -> Result<Status, Error> {
            #[derive(Serialize)]
            struct LogEntry {
                channel: u32,
                f: f32,
                shot_mark: bool,
            }

            let mut status = status.lock().await;

            let e = if let Some(prev_f) = &mut status.prev_freq {
                let v_prev_f = *prev_f;
                if f > v_prev_f + 500.0 {
                    Err(Error::Logick(format!(
                        "Random frequency jump detected! {} -> {}",
                        prev_f, f
                    )))
                } else if f > 0.0 && f < 49.0 {
                    Err(Error::Logick("Empty result".to_owned()))
                } else if (f.is_nan() || f < 49.0) && !v_prev_f.is_nan() {
                    Err(Error::Logick(format!("Invalid result: {}", f)))
                } else {
                    Ok(())
                }
            } else {
                status.prev_freq.replace(f);
                Ok(())
            };

            if let Err(e) = e {
                status.prev_freq = None;
                Err(e)?;
            } else {
                status.prev_freq = Some(f);
            }

            let shot_mark = if status.shot_requested {
                status.shot_requested = false;
                true
            } else {
                false
            };

            if let Some(fifo) = logfile.lock().await.deref_mut() {
                let entry = LogEntry {
                    channel: status.current_channel,
                    f,
                    shot_mark,
                };
                fifo.write_all(
                    format!("{}\n", serde_json::to_string(&entry).unwrap_or_default()).as_bytes(),
                )
                .await
                .unwrap();
            }
            Ok(Status {
                current_channel: status.current_channel,
                current_side: status.current_side,
                current_step: status.current_step,
                since_start: SystemTime::now().duration_since(start_time).unwrap(),
                current_frequency: f,

                camera_state: status.current_camera_state,
                valve_state: status.current_valve_state,

                shot_mark,
            })
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

        tokio::spawn(async move {
            loop {
                let res = {
                    let mut guard = dev.lock().await;
                    Self::i2c_read(
                        guard.deref_mut(),
                        freq_meter_i2c_addr,
                        0x08,
                        std::mem::size_of::<f32>(),
                    )
                    .await
                };

                match res {
                    Ok(r) => {
                        if r.len() == std::mem::size_of::<f32>() {
                            let f = if let Some(fake_freq) = emulate_center {
                                generate_fake_freq(fake_freq)
                            } else {
                                let byte_array: [u8; 4] = r[0..4].try_into().unwrap();
                                f32::from_le_bytes(byte_array)
                            };

                            let new_status = update_status(
                                &status,
                                f + *freq_merer_offset.lock().await,
                                &data_log_file,
                                start_time,
                            )
                            .await;
                            match new_status {
                                Ok(s) => {
                                    tx.send(s).ok();
                                }
                                Err(e) => tracing::error!("Freqmeter error: {:?}", e),
                            }
                        } else {
                            tracing::debug!("Freqmeter returned invalid data, skipping...");
                        }
                    }
                    Err(e) => tracing::debug!("Freqmeter error: {:?}", e),
                }

                tokio::time::sleep(update_interval).await;
            }
        });

        rx
    }

    pub async fn reset(
        &mut self,
        dev_addr: u8,
        i2c_comands: Vec<crate::config::I2CCommand>,
    ) -> Result<(), Error> {
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

        {
            // i2c init commands
            let mut guard = self.laser_setup.lock().await;
            for w in i2c_comands {
                Self::i2c_write(guard.deref_mut(), dev_addr, w.addr, &w.data)
                    .await
                    .map_err(Error::LaserSetup)?;
            }
        }

        Ok(())
    }

    pub async fn configure_position(&mut self, position: u32) -> Result<(), Error> {
        let mut a = self.burn_laser_power;
        let mut b = self.burn_laser_frequency;

        if let Some(pos) = self.positions.get(position as usize) {
            tracing::debug!(
                "Position {} config: MulS={:?}, MulA={:?}, MulB={:?}, MulF={:?}",
                position,
                pos.mul_laser_pump_power,
                pos.mul_laser_power,
                pos.mul_laser_pwm,
                pos.mul_laser_feedrate,
            );

            if let Some(mula) = pos.mul_laser_power {
                a *= mula;
            }
            if let Some(mulb) = pos.mul_laser_pwm {
                b = (b as f32 * mulb) as u32;
            }

            let status = self.current_status().await;

            self.execute_gcode(status, move |s, _| {
                let mut commands = vec![];

                commands.push(GCodeCtrl::Setup { a, b });

                (s, commands)
            })
            .await?;

            Ok(())
        } else {
            Err(Error::Logick("Invalid channel number".to_owned()))
        }
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
            // disable freqmeter update while changing channel
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

        self.configure_position(channel).await?;

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
            tracing::trace!("Sending {:?}...", cmd);
            self.laser_control
                .send(cmd)
                .await
                .map_err(|e| Error::Laser(e))?;

            tracing::trace!("Waiting conformation");
            self.get_gcode_result().await.map_err(|e| {
                tracing::error!("Can't setup initial position: {:?}", e);
                Error::Laser(e)
            })?;
        }

        Ok(new_status)
    }

    pub async fn step(&mut self, count: i32) -> Result<(), Error> {
        let ax_conf = self.axis_config;
        let total_vertical_steps = self.total_vertical_steps;
        let status = self.current_status().await;

        if count > 0 && status.current_step >= self.total_vertical_steps {
            return Err(Error::Logick("Maximum steps wriched!".to_owned()));
        }
        if count < 0 && status.current_step < (-count) as u32 {
            return Err(Error::Logick("Can't step below zero position".to_owned()));
        }

        let new_status = self
            .execute_gcode(status, move |mut status, workspace| {
                status.current_step = status.current_step.wrapping_add_signed(count);

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

        let status = self.current_status().await;

        let ch_cfg = self.positions[status.current_channel as usize];

        let s = self.burn_laser_pump_power * ch_cfg.mul_laser_power.unwrap_or(1.0);
        let f = self.burn_laser_feedrate * ch_cfg.mul_laser_feedrate.unwrap_or(1.0);

        let mut trys = 2;
        let new_status = loop {
            match self
                .execute_gcode(status, move |mut status, workspace| {
                    let mut commands = vec![];

                    status.current_side = status.current_side.mirrored();
                    status.shot_requested = true;

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

                    commands.push(GCodeCtrl::M3 { s });
                    commands.push(cmd);
                    commands.push(GCodeCtrl::M5);

                    (status, commands)
                })
                .await
            {
                Ok(s) => break s,
                Err(Error::Logick(e)) => return Err(Error::Logick(e)),
                Err(e) => {
                    trys -= 1;
                    if trys == 0 {
                        return Err(e);
                    } else {
                        tracing::warn!("Burn: {}", e);
                    }
                }
            }
        };

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

    async fn i2c_write<'a, E: Debug, I: laser_setup_interface::I2c<Error = E>>(
        d: &'a mut I,
        dev_addr: u8,
        start_addr: u8,
        data: &[u8],
    ) -> Result<(), E> {
        let mut data_to_tx = vec![start_addr];
        data_to_tx.extend(data.into_iter());

        let mut ops = vec![laser_setup_interface::Operation::Write(&data_to_tx)];

        d.transaction(dev_addr, &mut ops).await
    }

    pub async fn set_freq_meter_offset(&mut self, offset: f32) {
        *self.freq_merer_offset.lock().await = offset;
    }

    pub async fn get_freq_meter_offset(&self) -> f32 {
        *self.freq_merer_offset.lock().await
    }

    pub async fn get_current_step(&self) -> u32 {
        self.status.lock().await.current_step
    }
}
