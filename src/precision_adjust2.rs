use std::default::Default;
use std::fmt::Debug;
use std::io::Error as IoError;
use std::sync::Arc;
use std::time::{Duration, SystemTime};

use tokio::sync::watch::{Receiver, Sender};
use tokio::sync::Mutex;

use laser_setup_interface::{CameraState, ControlState, ValveState};

use crate::laser_setup_controller::LaserSetupStatus;
use crate::{LaserController, LaserSetupController};

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
pub struct Status {
    pub current_channel: u32,
    pub current_step: u32,

    pub since_start: Duration,
    pub current_frequency: f32,

    pub camera_state: CameraState,
    pub valve_state: ValveState,

    pub shot_mark: bool,
}

#[derive(Debug, Clone, Copy, Default)]
struct PrivStatusEvent {
    chanel_select: Option<u32>,
    camera: Option<CameraState>,
    shot_mark: Option<bool>,
    step: Option<i32>,
}

pub struct PrecisionAdjust2 {
    laser_setup: Arc<Mutex<LaserSetupController>>,
    laser_controller: Arc<Mutex<LaserController>>,
    status_rx: Receiver<Status>,
    ev_tx: tokio::sync::mpsc::Sender<PrivStatusEvent>,
}

pub const TRYS: usize = 3;

impl PrecisionAdjust2 {
    pub async fn new(
        laser_setup: Arc<Mutex<LaserSetupController>>,
        laser_controller: Arc<Mutex<LaserController>>,
    ) -> Self {
        let (status_tx, status_rx) = tokio::sync::watch::channel(Status {
            current_channel: 0,
            current_step: 0,

            since_start: Duration::from_secs(0),
            current_frequency: 0.0,

            camera_state: CameraState::Close,
            valve_state: ValveState::Atmosphere,

            shot_mark: false,
        });

        let (ev_tx, ev_rx) = tokio::sync::mpsc::channel(5);

        let lss_rx = laser_setup.lock().await.subscribe();

        tokio::spawn(status_watcher(lss_rx, status_tx, ev_rx));

        Self {
            laser_setup,
            laser_controller,
            status_rx,
            ev_tx,
        }
    }

    pub async fn test_connection(&mut self) -> Result<(), Error> {
        self.laser_setup
            .lock()
            .await
            .test_connection()
            .await
            .map_err(|e| Error::LaserSetup(e))?;
        self.laser_controller.lock().await.test_connection().await?;
        Ok(())
    }

    pub async fn reset(&mut self) -> Result<(), Error> {
        self.laser_setup
            .lock()
            .await
            .reset()
            .await
            .map_err(|e| Error::LaserSetup(e))?;
        self.laser_controller.lock().await.reset().await?;
        Ok(())
    }

    pub async fn select_channel(&mut self, channel: u32) -> Result<(), Error> {
        self.laser_setup
            .lock()
            .await
            .select_channel(channel)
            .await
            .map_err(|e| Error::LaserSetup(e))?;
        self.laser_controller
            .lock()
            .await
            .select_channel(channel, None, Some(TRYS))
            .await?;

        self.ev_tx
            .send(PrivStatusEvent {
                chanel_select: Some(channel),
                ..Default::default()
            })
            .await
            .ok();

        Ok(())
    }

    pub async fn open_camera(&mut self) -> Result<(), Error> {
        self.laser_setup
            .lock()
            .await
            .camera_control(CameraState::Open)
            .await
            .map_err(|e| Error::LaserSetup(e))?;

        self.ev_tx
            .send(PrivStatusEvent {
                camera: Some(CameraState::Open),
                ..Default::default()
            })
            .await
            .ok();

        Ok(())
    }

    pub async fn close_camera(&mut self, vacuum: bool) -> Result<(), Error> {
        let mut guard = self.laser_setup.lock().await;
        guard
            .camera_control(CameraState::Close)
            .await
            .map_err(|e| Error::LaserSetup(e))?;
        if vacuum {
            guard
                .valve_control(ValveState::Vacuum)
                .await
                .map_err(|e| Error::LaserSetup(e))?;
        }
        self.ev_tx
            .send(PrivStatusEvent {
                camera: Some(CameraState::Close),
                ..Default::default()
            })
            .await
            .ok();

        Ok(())
    }

    pub async fn step(&mut self, count: i32) -> Result<(), Error> {
        self.laser_controller
            .lock()
            .await
            .step(count, Some(TRYS))
            .await?;

        self.ev_tx
            .send(PrivStatusEvent {
                step: Some(count),
                ..Default::default()
            })
            .await
            .ok();

        Ok(())
    }

    pub async fn burn(&mut self, soft_mode: bool) -> Result<(), Error> {
        self.laser_controller
            .lock()
            .await
            .burn(1, Some(1), Some(TRYS), soft_mode)
            .await?;
        self.ev_tx
            .send(PrivStatusEvent {
                shot_mark: Some(true),
                ..Default::default()
            })
            .await
            .ok();

        Ok(())
    }

    pub async fn set_freq_meter_offset(&mut self, offset: f32) {
        self.laser_setup
            .lock()
            .await
            .set_freq_meter_offset(offset)
            .await;
    }

    pub async fn get_freq_meter_offset(&self) -> f32 {
        self.laser_setup.lock().await.get_freq_meter_offset()
    }

    pub async fn get_current_step(&self) -> u32 {
        self.laser_controller.lock().await.get_current_step()
    }

    pub fn subscribe_status(&self) -> Receiver<Status> {
        self.status_rx.clone()
    }
}

async fn status_watcher(
    mut rx: Receiver<LaserSetupStatus>,
    tx: Sender<Status>,
    mut ev_rx: tokio::sync::mpsc::Receiver<PrivStatusEvent>,
) {
    let start_time = SystemTime::now();
    let mut status = Status {
        current_channel: 0,
        current_step: 0,

        since_start: Duration::from_secs(0),
        current_frequency: 0.0,

        camera_state: CameraState::Close,
        valve_state: ValveState::Atmosphere,

        shot_mark: false,
    };

    loop {
        tokio::select! {
            ev = ev_rx.recv() => {
                // command recived
                if let Some(ev)= ev {
                    if let Some(channel) = ev.chanel_select {
                        status.current_channel = channel;
                        status.current_step = 0;
                    }
                    if let Some(camera) = ev.camera {
                        status.camera_state = camera;
                    }
                    if let Some(shot_mark) = ev.shot_mark {
                        status.shot_mark = shot_mark;
                    }
                    if let Some(step) = ev.step {
                        status.current_step = (status.current_step as i32 + step) as u32;
                    }
                }
            }
            s = rx.changed() => {
                // Status changed
                if s.is_ok() {
                    let s = rx.borrow();
                    status.current_frequency = s.current_frequency;
                    status.since_start = SystemTime::now().duration_since(start_time).unwrap();
                    status.valve_state = s.valve_state;
                    tx.send(status).unwrap();
                    status.shot_mark = false;
                }
            }
        }
    }
}
