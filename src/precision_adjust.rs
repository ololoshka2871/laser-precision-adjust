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
struct Status {
    current_channel: usize,
}

pub struct PrecisionAdjust {
    positions: Vec<crate::config::ResonatroPlacement>,
    laser_setup: LaserSetup,
    laser_control: tokio_util::codec::Framed<tokio_serial::SerialStream, gcode_codec::LineCodec>,
    kosa: Mutex<Option<Kosa>>,
    timeout: Duration,

    kosa_status_rx: Mutex<Option<tokio::sync::mpsc::Receiver<(SystemTime, f32)>>>,

    status: Mutex<Status>,

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

            status: Mutex::new(Status { current_channel: 0 }),

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

    pub async fn status(&mut self, fmt: &mut impl std::io::Write) -> Result<(), IoError> {
        let mut guard = self.kosa_status_rx.lock().await;
        if let Some(rx) = guard.as_mut() {
            if let Some((t, f)) = rx.recv().await {
                let status = self.status.lock().await;
                writeln!(
                    fmt,
                    "Ch: {}; [{:0.3}] F = {:.2}",
                    status.current_channel,
                    t.duration_since(self.start_time)
                        .unwrap()
                        .as_millis() as f32 / 1000.0,
                    f
                )
            } else {
                unreachable!()
            }
        } else {
            log::error!(
                "Kosa status channel not initialized! please call start_monitoring() first!"
            );
            return Ok(());
        }
    }

    pub async fn start_monitoring(&mut self) {
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
        });
    }
}
