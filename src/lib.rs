mod config;
mod precision_adjust;

pub mod predict;

pub mod box_plot;
pub mod coordinates;
pub(crate) mod gcode_codec;
pub(crate) mod gcode_ctrl;

pub use config::{AutoAdjustLimits, Config, ForecastConfig};
pub use precision_adjust::{Error, PrecisionAdjust, Status};

#[derive(Clone)]
pub struct AdjustConfig {
    pub target_freq: f32,
    pub work_offset_hz: f32,
}

pub trait IDataPoint<T> {
    fn x(&self) -> T;
    fn y(&self) -> T;
}

#[derive(Clone, Copy, Default, serde::Serialize, serde::Deserialize)]
pub struct DataPoint<T: serde::Serialize> {
    x: T,
    y: T,
}

impl<T: num_traits::Float + serde::Serialize> DataPoint<T> {
    pub fn new(x: T, y: T) -> Self {
        Self { x, y }
    }

    pub fn nan() -> Self {
        Self {
            x: T::nan(),
            y: T::nan(),
        }
    }
}

impl<T: num_traits::Float + serde::Serialize> IDataPoint<T> for DataPoint<T> {
    fn x(&self) -> T {
        self.x
    }

    fn y(&self) -> T {
        self.y
    }
}
