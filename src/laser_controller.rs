use std::io::Error as IoError;
use std::time::Duration;

use futures_util::{SinkExt, StreamExt};
use tokio_serial::SerialPortBuilderExt;
use tokio_util::codec::Decoder;

use crate::coordinates::{CoordiantesCalc, Side};
use crate::precision_adjust2::Error;
use crate::{gcode_codec, gcode_ctrl::GCodeCtrl};

pub struct LaserController {
    laser_control: tokio_util::codec::Framed<tokio_serial::SerialStream, gcode_codec::LineCodec>,
    gcode_timeout: Duration,
    positions: Vec<crate::config::ResonatroPlacement>,
    axis_config: crate::config::AxisConfig,
    total_vertical_steps: u32,

    burn_laser_pump_power: f32,
    burn_laser_power: f32,
    burn_laser_frequency: u32,
    burn_laser_feedrate: f32,

    current_channel: u32,
    current_step: u32,
    side: Side,
}

impl LaserController {
    pub fn new<'a>(
        path: impl Into<std::borrow::Cow<'a, str>>,
        gcode_timeout: Duration,
        positions: Vec<crate::config::ResonatroPlacement>,
        axis_config: crate::config::AxisConfig,
        total_vertical_steps: u32,

        burn_laser_pump_power: f32,
        burn_laser_power: f32,
        burn_laser_frequency: u32,
        burn_laser_feedrate: f32,
    ) -> Self {
        let laser_port = tokio_serial::new(path, 1500000)
            .open_native_async()
            .unwrap();
        Self {
            laser_control: gcode_codec::LineCodec.framed(laser_port),
            gcode_timeout,
            positions,
            axis_config,
            total_vertical_steps,

            burn_laser_pump_power,
            burn_laser_power,
            burn_laser_frequency,
            burn_laser_feedrate,

            current_channel: 0,
            current_step: 0,
            side: Side::Left,
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

    pub async fn execute_gcode_trys(
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

    pub async fn execute_gcode(&mut self, cmds: Vec<GCodeCtrl>) -> Result<(), Error> {
        self.execute_gcode_trys(cmds, None).await
    }

    /// Выбрать канал и переместиться к initial_step
    pub async fn select_channel(
        &mut self,
        channel: u32,
        initial_step: Option<u32>,
        trys: Option<usize>,
    ) -> Result<(), Error> {
        let initial_step = initial_step.unwrap_or(0);

        if channel >= self.positions.len() as u32 {
            return Err(Error::Logick(format!(
                "Channel {} is out of range (0 - {})!",
                channel,
                self.positions.len() - 1
            )));
        }

        if initial_step > self.total_vertical_steps {
            return Err(Error::Logick(format!(
                "Initial step {} is out of range (0 - {})!",
                initial_step, self.total_vertical_steps
            )));
        }

        let pos = self.positions[channel as usize];
        let ax_conf = self.axis_config;
        let total_vertical_steps = self.total_vertical_steps;

        let mut a = self.burn_laser_power;
        let mut b = self.burn_laser_frequency;

        if let Some(mula) = pos.mul_laser_power {
            a *= mula;
        }
        if let Some(mulb) = pos.mul_laser_pwm {
            b = (b as f32 * mulb) as u32;
        }

        let (new_x, new_y) = pos.to_abs(&ax_conf, 0, Side::Left, total_vertical_steps);

        let commands = vec![
            GCodeCtrl::M5,
            GCodeCtrl::Setup { a, b },
            GCodeCtrl::G0 { x: new_x, y: new_y },
        ];

        self.execute_gcode_trys(commands, trys).await?;

        self.current_channel = channel;
        self.current_step = initial_step;
        self.side = if initial_step % 2 == 1 {
            Side::Right
        } else {
            Side::Left
        };

        Ok(())
    }

    /// Сделать burn_count шагов с шагом burn_step
    pub async fn burn(
        &mut self,
        burn_count: u32,
        burn_step: Option<i32>,
        trys: Option<usize>,
    ) -> Result<(), Error> {
        let burn_step = burn_step.unwrap_or(0);

        let ch_cfg = self.positions[self.current_channel as usize];

        let s = self.burn_laser_pump_power * ch_cfg.mul_laser_power.unwrap_or(1.0);
        let f = self.burn_laser_feedrate * ch_cfg.mul_laser_feedrate.unwrap_or(1.0);

        let mut commands = vec![GCodeCtrl::M3 { s }];
        let mut side = self.side;
        let mut current_step = self.current_step;
        for _ in 0..burn_count {
            side = side.mirrored();
            if burn_step < 0 && current_step < (-burn_step) as u32 {
                return Err(Error::Laser(IoError::new(
                    std::io::ErrorKind::InvalidInput,
                    "Burn step too big",
                )));
            }
            if burn_step > 0 && current_step + burn_step as u32 > self.total_vertical_steps {
                return Err(Error::Laser(IoError::new(
                    std::io::ErrorKind::InvalidInput,
                    "Burn step too big",
                )));
            }
            current_step = current_step.wrapping_add_signed(burn_step);
            let new_abs_coordinates = ch_cfg.to_abs(
                &self.axis_config,
                current_step,
                side,
                self.total_vertical_steps,
            );
            let cmd = GCodeCtrl::G1 {
                x: new_abs_coordinates.0,
                y: new_abs_coordinates.1,
                f,
            };
            commands.push(cmd);
        }
        commands.push(GCodeCtrl::M5);

        self.execute_gcode_trys(commands, trys).await?;

        self.side = side;
        self.current_step = current_step;

        Ok(())
    }

    pub async fn step(&mut self, count: i32, trys: Option<usize>) -> Result<(), Error> {
        let ch_cfg = self.positions[self.current_channel as usize];

        let side = if count % 2 == 0 {
            self.side
        } else {
            self.side.mirrored()
        };
        let current_step = self
            .current_step
            .checked_add_signed(count)
            .ok_or(Error::Logick("Overflow".to_owned()))?;

        let new_abs_coordinates = ch_cfg.to_abs(
            &self.axis_config,
            current_step,
            side,
            self.total_vertical_steps,
        );
        let cmd = GCodeCtrl::G0 {
            x: new_abs_coordinates.0,
            y: new_abs_coordinates.1,
        };
        let commands = vec![cmd];

        self.execute_gcode_trys(commands, trys).await?;

        self.side = side;
        self.current_step = current_step;

        Ok(())
    }

    // Получить номер текущего шага
    pub fn get_current_step(&self) -> u32 {
        self.current_step
    }

    pub async fn test_connection(&mut self) -> Result<(), Error> {
        self.raw_gcode("\n").await.map_err(|e| Error::Laser(e))
    }
}
