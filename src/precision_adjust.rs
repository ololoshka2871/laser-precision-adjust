use std::io::Error as IoError;
use std::time::Duration;

use futures::{SinkExt, StreamExt};
use kosa_interface::Kosa;
use laser_setup_interface::LaserSetup;
use tokio_serial::SerialPortBuilderExt;
use tokio_util::codec::Decoder;

use crate::{gcode_ctrl::GCodeCtrl, gcode_codec};

#[derive(Debug)]
pub enum Error {
    Laser(IoError),
    LaserSetup(laser_setup_interface::Error),
    Kosa(kosa_interface::Error),
}

pub struct PrecisionAdjust {
    positions: Vec<crate::config::ResonatroPlacement>,
    laser_setup: LaserSetup,
    laser_control: tokio_util::codec::Framed<tokio_serial::SerialStream, gcode_codec::LineCodec>,
    kosa: Kosa,
    timeout: Duration,
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
            kosa,
            positions: config.resonator_placement,
            timeout,
        }
    }

    pub async fn test_connection(&mut self) -> Result<(), Error> {
        self.laser_setup.read().await.map_err(Error::LaserSetup)?;
        self.kosa
            .get_measurement(self.timeout)
            .await
            .map_err(Error::Kosa)?;
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
}
