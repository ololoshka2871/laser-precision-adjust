use std::default::Default;
use std::io::Error as IoError;
use std::sync::Arc;
use std::time::{Duration, SystemTime};

use futures::{SinkExt, StreamExt};
use kosa_interface::Kosa;
use laser_setup_interface::{CameraState, ControlState, LaserSetup, ValveState};
use tokio::sync::Mutex;
use tokio_serial::SerialPortBuilderExt;
use tokio_util::codec::Decoder;

use crate::coordinates::{CoordiantesCalc, Side};
use crate::{gcode_codec, gcode_ctrl::GCodeCtrl};

#[derive(Debug)]
pub enum Error {
    Laser(IoError),
    LaserSetup(laser_setup_interface::Error),
    Kosa(kosa_interface::Error),
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

    laser_setup: LaserSetup,
    laser_control: tokio_util::codec::Framed<tokio_serial::SerialStream, gcode_codec::LineCodec>,
    kosa: Mutex<Option<Kosa>>,
    timeout: Duration,
    gcode_timeout: Duration,

    kosa_status_rx: Mutex<Option<tokio::sync::mpsc::Receiver<(SystemTime, f32)>>>,

    status: Mutex<PrivStatus>,

    kosa_update_locker: Arc<Mutex<()>>,

    start_time: SystemTime,

    burn_laser_power: f32,
    burn_laser_pump_power: f32,
    burn_laser_feedrate: f32,

    axis_config: crate::config::AxisConfig,
}

impl PrecisionAdjust {
    pub fn with_config(config: crate::Config) -> Self {
        let timeout = Duration::from_millis(config.port_timeout_ms);
        let gcode_timeout = Duration::from_millis(config.gcode_timeout_ms);
        let laser_port = tokio_serial::new(config.laser_control_port, 1500000)
            .open_native_async()
            .unwrap();
        let laser_setup = LaserSetup::new(&config.laser_setup_port, timeout);
        let kosa = Kosa::new(&config.kosa_port);

        Self {
            laser_setup,
            total_vertical_steps: config.total_vertical_steps,

            laser_control: gcode_codec::LineCodec.framed(laser_port),
            kosa: Mutex::new(Some(kosa)),
            positions: config.resonator_placement,
            timeout,
            gcode_timeout,

            kosa_status_rx: Mutex::new(None),

            status: Mutex::new(PrivStatus {
                current_channel: 0,
                current_side: Side::Left,
                current_step: 0,

                current_camera_state: CameraState::Close,
                current_valve_state: ValveState::Atmosphere,
            }),

            kosa_update_locker: Arc::new(Mutex::new(())),

            start_time: SystemTime::now(),

            burn_laser_power: config.burn_laser_power,
            burn_laser_pump_power: config.burn_laser_pump_power,
            burn_laser_feedrate: config.burn_laser_feedrate,

            axis_config: config.axis_config,
        }
    }

    pub async fn test_connection(&mut self) -> Result<(), Error> {
        {
            let mut kosa = self.kosa.lock().await;
            if let Some(kosa) = kosa.as_mut() {
                kosa.get_measurement(self.timeout)
                    .await
                    .map_err(Error::Kosa)?;
            }
        }

        self.laser_setup.read().await.map_err(Error::LaserSetup)?;
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
        let mut guard = self.kosa_status_rx.lock().await;
        if let Some(rx) = guard.as_mut() {
            if let Some((t, f)) = rx.recv().await {
                let status = self.status.lock().await;

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
                "Kosa status channel not initialized! please call start_monitoring() first!"
            );
            return Err(Error::Kosa(kosa_interface::Error::ZeroResponce));
        }
    }

    pub async fn start_monitoring(&mut self) -> tokio::task::JoinHandle<()> {
        let mut kosa = self.kosa.lock().await.take().expect("Kosa already taken!");
        let (tx, rx) = tokio::sync::mpsc::channel(1);
        self.kosa_status_rx.lock().await.replace(rx);

        let kosa_update_locker = Arc::<tokio::sync::Mutex<()>>::downgrade(&self.kosa_update_locker);

        tokio::spawn(async move {
            loop {
                let res = {
                    match kosa_update_locker.upgrade() {
                        Some(guard) => {
                            let _guard = guard.lock().await;
                            kosa.get_measurement(Duration::from_secs(1)).await
                        }
                        None => break,
                    }
                };
                match res {
                    Ok(r) => {
                        let f = r.freq();
                        if f != 0.0 {
                            tx.send((SystemTime::now(), r.freq())).await.ok();
                        } else {
                            log::debug!("Kosa returned F=0.0, skipping...");
                        }
                    }
                    Err(e) => log::debug!("Kosa error: {:?}", e),
                }
            }
        })
    }

    pub async fn reset(&mut self) -> Result<(), Error> {
        let a = self.burn_laser_pump_power;

        let status = self.current_status().await;

        let mut new_status = self
            .execute_gcode(status, move |status, _| {
                let mut commands = vec![];

                commands.push(GCodeCtrl::Reset);
                commands.push(GCodeCtrl::Setup { a });

                (status, commands)
            })
            .await?;

        // read current laser setup state
        let state = self.laser_setup.read().await.map_err(Error::LaserSetup)?;
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
            let _kosa_guard = self.kosa.lock().await;

            self.laser_setup
                .write(&LaserCtrl {
                    channel: Some(channel),
                    ..Default::default()
                })
                .await
                .map_err(Error::LaserSetup)?;

            // sleep 100 ms to let laser setup to change channel
            tokio::time::sleep(Duration::from_millis(100)).await;
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
        let burn_laser_power = self.burn_laser_power;
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

        let s = override_s.unwrap_or(self.burn_laser_power);
        let f = override_f.unwrap_or(self.burn_laser_feedrate);
        let a = override_pump.unwrap_or(self.burn_laser_pump_power);
        let default_a = self.burn_laser_pump_power;
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
}
