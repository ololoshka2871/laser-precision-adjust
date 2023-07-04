use std::io::Error as IoError;
use std::time::{Duration, SystemTime};

use futures::{SinkExt, StreamExt};
use kosa_interface::Kosa;
use laser_setup_interface::LaserSetup;
use tokio::sync::Mutex;
use tokio_serial::SerialPortBuilderExt;
use tokio_util::codec::Decoder;

use crate::{gcode_codec, gcode_ctrl::GCodeCtrl};

#[derive(Debug)]
pub enum Error {
    Laser(IoError),
    LaserSetup(laser_setup_interface::Error),
    Kosa(kosa_interface::Error),
}

#[derive(Debug, Clone, Copy)]
pub enum Side {
    Left,
    Right,
}

#[derive(Debug, Clone, Copy)]
pub struct PrivStatus {
    current_channel: usize,
    current_side: Side,
    current_step: usize,
}

pub struct Status {
    current_channel: usize,
    current_side: Side,
    current_step: usize,

    timestamp: SystemTime,
    current_frequency: f32,
}

pub struct PrecisionAdjust {
    positions: Vec<crate::config::ResonatroPlacement>,
    laser_setup: LaserSetup,
    laser_control: tokio_util::codec::Framed<tokio_serial::SerialStream, gcode_codec::LineCodec>,
    kosa: Mutex<Option<Kosa>>,
    timeout: Duration,

    kosa_status_rx: Mutex<Option<tokio::sync::mpsc::Receiver<(SystemTime, f32)>>>,

    status: Mutex<PrivStatus>,

    start_time: SystemTime,
}

impl PrecisionAdjust {
    pub fn with_config(config: crate::Config) -> Self {
        let timeout = Duration::from_millis(config.port_timeout_ms);
        let laser_port = tokio_serial::new(config.laser_control_port, 1500000)
            .open_native_async()
            .unwrap();
        let laser_setup = LaserSetup::new(&config.laser_setup_port, timeout);
        let kosa = Kosa::new(&config.kosa_port);

        Self {
            laser_setup,
            laser_control: gcode_codec::LineCodec.framed(laser_port),
            kosa: Mutex::new(Some(kosa)),
            positions: config.resonator_placement,
            timeout,

            kosa_status_rx: Mutex::new(None),

            status: Mutex::new(PrivStatus {
                current_channel: 0,
                current_side: Side::Left,
                current_step: 0,
            }),

            start_time: SystemTime::now(),
        }
    }

    pub async fn test_connection(&mut self) -> Result<(), Error> {
        {
            let mut kosa = self.kosa.lock().await;
            let kosa = kosa.as_mut().unwrap();
            kosa.get_measurement(self.timeout)
                .await
                .map_err(Error::Kosa)?;
        }

        self.laser_setup.read().await.map_err(Error::LaserSetup)?;
        self.raw_gcode("\n").await.map_err(Error::Laser)?;
        Ok(())
    }

    async fn get_gcode_result(&mut self) -> Result<(), IoError> {
        use std::io::ErrorKind;
        match tokio::time::timeout(self.timeout, self.laser_control.next()).await {
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

    pub async fn print_status(&mut self, fmt: &mut impl std::io::Write) -> Result<(), IoError> {
        use colored::Colorize;

        match self.get_status().await {
            Ok(status) => writeln!(
                fmt,
                "[{:0>8.3}]: Ch: {}; Step: [{}:{}]; F: {}",
                status
                    .timestamp
                    .duration_since(self.start_time)
                    .unwrap()
                    .as_millis() as f32
                    / 1000.0,
                format!("{:02}", status.current_channel).green().bold(),
                format!("{:>2}", status.current_step).purple().bold(),
                format!("{:>5?}", status.current_side).blue(),
                format!("{:0>7.2}", status.current_frequency).yellow()
            ),
            Err(Error::Kosa(kosa_interface::Error::ZeroResponce)) => {
                log::error!(
                    "Kosa status channel not initialized! please call start_monitoring() first!"
                );
                return Ok(());
            }
            Err(e) => {
                log::error!("Error getting status: {:?}", e);
                Err(IoError::new(
                    std::io::ErrorKind::Other,
                    "Error getting status",
                ))
            }
        }
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
                    timestamp: t,
                    current_frequency: f,
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

        tokio::spawn(async move {
            loop {
                let res = kosa.get_measurement(Duration::from_secs(1)).await;
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
}
