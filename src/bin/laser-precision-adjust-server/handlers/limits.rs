use laser_precision_adjust::Config;
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Clone, Copy, Debug)]
pub enum RezStatus {
    #[serde(rename = "unknown")]
    Unknown,
    #[serde(rename = "upper")]
    UpperBound,
    #[serde(rename = "ok")]
    Ok,
    #[serde(rename = "lower")]
    LowerBound,
    #[serde(rename = "lowerest")]
    LowerLimit,
}

pub struct Limits {
    pub upper_limit: f32,
    pub lower_limit: f32,
    pub ultra_low_limit: f32,
}

impl Limits {
    pub fn to_status(&self, f: f32) -> RezStatus {
        if f.is_nan() || f == 0.0 {
            RezStatus::Unknown
        } else if f < self.ultra_low_limit {
            RezStatus::LowerLimit
        } else if f < self.lower_limit {
            RezStatus::LowerBound
        } else if f > self.upper_limit {
            RezStatus::UpperBound
        } else {
            RezStatus::Ok
        }
    }

    pub fn to_status_icon(&self, f: f32) -> &'static str {
        if f.is_nan() || f == 0.0 {
            "-"
        } else if f < self.ultra_low_limit {
            "▼"
        } else if f < self.lower_limit {
            "▽"
        } else if f > self.upper_limit {
            "▲"
        } else {
            "✔"
        }
    }

    pub fn ppm(&self, f: f32) -> f32 {
        let f_center = (self.lower_limit + self.upper_limit) / 2.0;
        (f - f_center) / f_center * 1_000_000.0
    }

    pub fn from_config(target: f32, config: &Config) -> Self {
        let ppm2hz = target * config.working_offset_ppm / 1_000_000.0;
        Self {
            upper_limit: target + ppm2hz,
            lower_limit: target - ppm2hz,
            ultra_low_limit: target - config.auto_adjust_limits.min_freq_offset,
        }
    }
}
