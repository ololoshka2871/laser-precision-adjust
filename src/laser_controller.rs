use std::io::Error as IoError;
use std::time::Duration;

use futures_util::{SinkExt, StreamExt};
use tokio_serial::SerialPortBuilderExt;
use tokio_util::codec::Decoder;

use crate::Error;
use crate::{gcode_codec, gcode_ctrl::GCodeCtrl};

pub struct LaserController {
    laser_control: tokio_util::codec::Framed<tokio_serial::SerialStream, gcode_codec::LineCodec>,
    gcode_timeout: Duration,
    positions: Vec<crate::config::ResonatroPlacement>,
}

impl LaserController {
    pub fn new<'a>(
        path: impl Into<std::borrow::Cow<'a, str>>,
        gcode_timeout: Duration,
        positions: Vec<crate::config::ResonatroPlacement>,
    ) -> Self {
        let laser_port = tokio_serial::new(path, 1500000)
            .open_native_async()
            .unwrap();
        Self {
            laser_control: gcode_codec::LineCodec.framed(laser_port),
            gcode_timeout,
            positions,
        }
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

    async fn execute_gcode_trys(
        &mut self,
        cmds: Vec<GCodeCtrl>,
        trys: Option<usize>,
    ) -> Result<(), Error> {
        for cmd in cmds {
            let mut ctrys = trys.unwrap_or(1);
            tracing::trace!("Sending {:?}...", cmd);
            loop {
                if let Err(e) = self.laser_control.send(cmd.clone()).await {
                    ctrys -= 1;
                    if ctrys == 0 {
                        return Err(Error::Laser(e));
                    }
                } else {
                    break; // ok
                }
            }

            tracing::trace!("Waiting conformation");
            self.get_gcode_result().await.map_err(|e| {
                tracing::error!("Can't setup initial position: {:?}", e);
                Error::Laser(e)
            })?;
        }

        Ok(())
    }

    async fn execute_gcode(&mut self, cmds: Vec<GCodeCtrl>) -> Result<(), Error> {
        self.execute_gcode_trys(cmds, None).await
    }

    /// Выбрать канал и переместиться к initial_step
    async fn select_channel(
        &mut self,
        channel: u32,
        initial_step: Option<u32>,
        trys: Option<usize>,
    ) -> Result<(), Error> {
        let initial_step = initial_step.unwrap_or(0);
        Ok(())
    }

    /// Сделать burn_count шагов с шагом burn_step
    async fn burn(
        &mut self,
        burn_count: u32,
        burn_step: Option<i32>,
        trys: Option<usize>,
    ) -> Result<(), Error> {
        let burn_step = burn_step.unwrap_or(0);
        Ok(())
    }
}
