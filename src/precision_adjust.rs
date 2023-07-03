use std::time::Duration;

use kosa_interface::Kosa;
use laser_setup_interface::LaserSetup;
use tokio_serial::SerialPortBuilderExt;
use tokio_util::codec::Decoder;

use crate::line_codec;

pub struct PrecisionAdjust {
    positions: Vec<crate::config::ResonatroPlacement>,
    laser_setup: LaserSetup,
    laser_control: tokio_util::codec::Framed<tokio_serial::SerialStream, line_codec::LineCodec>,
    kosa: Kosa,
}

impl PrecisionAdjust {
    pub fn with_config(config: crate::Config) -> Self {
        let laser_port = tokio_serial::new(config.laser_control_port, 1500000)
            .open_native_async()
            .unwrap();
        let laser_setup = LaserSetup::new(&config.laser_setup_port, Duration::from_millis(config.port_timeout_ms));
        let kosa = Kosa::new(&config.kosa_port);

        Self {
            laser_setup,
            laser_control: line_codec::LineCodec.framed(laser_port),
            kosa,
            positions: config.resonator_placement,
        }
    }
}
