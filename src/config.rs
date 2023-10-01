use std::path::PathBuf;

use serde::{Deserialize, Serialize};

#[derive(Deserialize, Clone, Copy, Serialize)]
pub struct ResonatroPlacement {
    #[serde(rename = "Xcenter", serialize_with = "crate::serialize_float_2dgt")]
    pub x: f32,

    #[serde(rename = "Ycenter", serialize_with = "crate::serialize_float_2dgt")]
    pub y: f32,

    #[serde(rename = "Width", serialize_with = "crate::serialize_float_2dgt")]
    pub w: f32,

    #[serde(rename = "Height", serialize_with = "crate::serialize_float_2dgt")]
    pub h: f32,
}

#[derive(Deserialize, Clone, Copy, Serialize)]
pub struct AxisConfig {
    #[serde(rename = "SwapXY")]
    pub swap_xy: bool,

    #[serde(rename = "ReverseX")]
    pub reverse_x: bool,

    #[serde(rename = "ReverseY")]
    pub reverse_y: bool,
}

#[derive(Deserialize, Clone, Copy, Serialize)]
pub struct ForecastConfig {
    #[serde(rename = "MinFreqGrow", serialize_with = "crate::serialize_float_2dgt")]
    pub min_freq_grow: f32,

    #[serde(rename = "MaxFreqGrow", serialize_with = "crate::serialize_float_2dgt")]
    pub max_freq_grow: f32,

    #[serde(rename = "MedianFreqGrow", serialize_with = "crate::serialize_float_2dgt")]
    pub median_freq_grow: f32,
}

#[derive(Deserialize, Clone, Copy, Serialize)]
pub struct AutoAdjustLimits {
    #[serde(rename = "MinFreqOffset", serialize_with = "crate::serialize_float_2dgt")]
    pub min_freq_offset: f32,

    #[serde(rename = "MaxForwardSteps")]
    pub max_forward_steps: u32,

    #[serde(rename = "MaxRetreatSteps")]
    pub max_retreat_steps: u32,

    #[serde(rename = "FastForwardStepLimit")]
    pub fast_forward_step_limit: u32,

    #[serde(rename = "EdgeDetectSintervalSt")]
    pub edge_detect_interval: u32,
}

#[derive(Deserialize, Clone, Serialize)]
pub struct Config {
    #[serde(rename = "LaserSetupPort")]
    pub laser_setup_port: String,

    #[serde(rename = "LaserControlPort")]
    pub laser_control_port: String,

    #[serde(rename = "DataLogFile")]
    pub data_log_file: Option<PathBuf>,

    #[serde(rename = "FreqMeterI2CAddr")]
    pub freq_meter_i2c_addr: u8,

    #[serde(rename = "PortTimeoutMs")]
    pub port_timeout_ms: u64,

    #[serde(rename = "GCodeTimeoutMs")]
    pub gcode_timeout_ms: u64,

    #[serde(rename = "AxisConfig")]
    pub axis_config: AxisConfig,

    #[serde(rename = "BurnLaserS", serialize_with = "crate::serialize_float_2dgt")]
    pub burn_laser_pump_power: f32,

    #[serde(rename = "BurnLaserA", serialize_with = "crate::serialize_float_2dgt")]
    pub burn_laser_power: f32,

    #[serde(rename = "BurnLaserB")]
    pub burn_laser_frequency: u32,

    #[serde(rename = "BurnLaserF", serialize_with = "crate::serialize_float_2dgt")]
    pub burn_laser_feedrate: f32,

    #[serde(rename = "TotalVerticalSteps")]
    pub total_vertical_steps: u32,

    #[serde(rename = "FreqmeterOffset", serialize_with = "crate::serialize_float_2dgt")]
    pub freqmeter_offset: f32,

    #[serde(rename = "WorkingOffsetPPM", serialize_with = "crate::serialize_float_2dgt")]
    pub working_offset_ppm: f32,

    #[serde(rename = "TargetFreqCenter", serialize_with = "crate::serialize_float_2dgt")]
    pub target_freq_center: f32,

    #[serde(rename = "UpdateIntervalMs")]
    pub update_interval_ms: u32,

    #[serde(rename = "DisplayPointsCount")]
    pub display_points_count: usize,

    #[serde(rename = "ForecastConfig")]
    pub forecast_config: ForecastConfig,

    #[serde(rename = "CooldownTimeMs")]
    pub cooldown_time_ms: u32,

    #[serde(rename = "AutoAdjustLimits")]
    pub auto_adjust_limits: AutoAdjustLimits,

    #[serde(rename = "StableVal", serialize_with = "crate::serialize_float_2dgt")]
    pub stable_val: f32,

    #[serde(rename = "ResonatorsPlacement")]
    pub resonator_placement: Vec<ResonatroPlacement>,
}

impl Config {
    pub fn load() -> (Self, PathBuf) {
        use std::path;

        if let Some(base_dirs) = directories::BaseDirs::new() {
            let path = base_dirs
                .config_dir()
                .join(path::Path::new("laser-precision-adjust"))
                .join(path::Path::new("config.json"));

            if let Ok(contents) = std::fs::read_to_string(path.clone()) {
                (serde_json::from_str::<Config>(&contents).unwrap(), path)
            } else {
                panic!(
                    "Failed to read {:?} file! Please copy config.json.example and fill it!",
                    path
                );
            }
        } else {
            panic!("Failed to get config directory!");
        }
    }
}

impl std::fmt::Display for Config {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        writeln!(f, "LaserSetupPort: {}", self.laser_setup_port)?;
        writeln!(f, "LaserControlPort: {}", self.laser_control_port)?;
        writeln!(f, "FreqFifo: {:?}", self.data_log_file)?;
        writeln!(f, "FreqMeterI2CAddr: {}", self.freq_meter_i2c_addr)?;
        writeln!(f, "PortTimeoutMs: {}", self.port_timeout_ms)?;
        writeln!(f, "GCodeTimeoutMs: {}", self.gcode_timeout_ms)?;

        writeln!(f, "AxisConfig:")?;
        writeln!(f, "  SwapXY: {}", self.axis_config.swap_xy)?;
        writeln!(f, "  ReverseX: {}", self.axis_config.reverse_x)?;
        writeln!(f, "  ReverseY: {}", self.axis_config.reverse_y)?;

        writeln!(f, "BurnLaserS: {}", self.burn_laser_pump_power)?;
        writeln!(f, "BurnLaserA: {}", self.burn_laser_power)?;
        writeln!(f, "BurnLaserB: {}", self.burn_laser_frequency)?;
        writeln!(f, "BurnLaserF: {}", self.burn_laser_feedrate)?;
        writeln!(f, "VerticalStep: {}", self.total_vertical_steps)?;
        writeln!(f, "FreqmeterOffset: {}", self.freqmeter_offset)?;
        writeln!(f, "WorkingOffsetPPM: {}", self.working_offset_ppm)?;
        writeln!(f, "TargetFreqCenter: {}", self.target_freq_center)?;
        writeln!(f, "UpdateIntervalMs: {}", self.update_interval_ms)?;
        writeln!(f, "DisplayPointsCount: {}", self.display_points_count)?;

        writeln!(f, "ForecastConfig:")?;
        writeln!(f, "  MinFreqGrow: {}", self.forecast_config.min_freq_grow)?;
        writeln!(f, "  MaxFreqGrow: {}", self.forecast_config.max_freq_grow)?;
        writeln!(f, "  MedianFreqGrow: {}", self.forecast_config.median_freq_grow)?;

        writeln!(f, "CooldownTimeMs: {}", self.cooldown_time_ms)?;

        writeln!(f, "AutoAdjustLimits:")?;
        writeln!(f, "  MinFreqOffset: {}", self.auto_adjust_limits.min_freq_offset)?;
        writeln!(f, "  MaxForwardSteps: {}", self.auto_adjust_limits.max_forward_steps)?;
        writeln!(f, "  MaxRetreatSteps: {}", self.auto_adjust_limits.max_retreat_steps)?;
        writeln!(f, "  FastForwardStepLimit: {}", self.auto_adjust_limits.fast_forward_step_limit)?;
        writeln!(f, "  EdgeDetectSintervalSt: {}", self.auto_adjust_limits.edge_detect_interval)?;

        writeln!(f, "StableVal: {}", self.stable_val)?;

        // write resonators placement as a table
        writeln!(f, "ResonatorsPlacement:")?;
        writeln!(f, "  Center\t| Width\t| Height")?;
        writeln!(f, "  ------\t| -----\t| ------")?;
        for placement in &self.resonator_placement {
            writeln!(
                f,
                "  X{} Y{}\t| {}\t| {}",
                placement.x, placement.y, placement.w, placement.h
            )?;
        }
        Ok(())
    }
}
