mod config;
mod limit_float_precission;
mod precision_adjust;

pub mod predict;

pub mod box_plot;
pub mod coordinates;
pub(crate) mod gcode_codec;
pub(crate) mod gcode_ctrl;

use num_traits::Float;

pub use config::{AutoAdjustLimits, Config, ForecastConfig};
pub use limit_float_precission::serialize_float_2dgt;
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
pub struct DataPoint<T: Float + serde::Serialize> {
    #[serde(serialize_with = "serialize_float_2dgt")]
    x: T,
    #[serde(serialize_with = "serialize_float_2dgt")]
    y: T,
}

impl<T: Float + serde::Serialize> DataPoint<T> {
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

impl<T: Float + serde::Serialize> IDataPoint<T> for DataPoint<T> {
    fn x(&self) -> T {
        self.x
    }

    fn y(&self) -> T {
        self.y
    }
}
