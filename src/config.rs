use std::path::PathBuf;

use serde::{Deserialize, Serialize};

#[derive(Deserialize, Clone, Copy, Serialize)]
pub struct ResonatroPlacement {
    #[serde(rename = "Xcenter")]
    pub x: f32,

    #[serde(rename = "Ycenter")]
    pub y: f32,

    #[serde(rename = "Width")]
    pub w: f32,

    #[serde(rename = "Height")]
    pub h: f32,

    /// Следующие параметры необязательные модификации паарметорв лажера для канала
    #[serde(rename = "MulS")]
    pub mul_laser_pump_power: Option<f32>,

    #[serde(rename = "MulA")]
    pub mul_laser_power: Option<f32>,

    #[serde(rename = "MulB")]
    pub mul_laser_pwm: Option<f32>,

    #[serde(rename = "MulF")]
    pub mul_laser_feedrate: Option<f32>,
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
    #[serde(rename = "MinFreqGrow")]
    pub min_freq_grow: f32,

    #[serde(rename = "MaxFreqGrow")]
    pub max_freq_grow: f32,

    #[serde(rename = "MedianFreqGrow")]
    pub median_freq_grow: f32,
}

#[derive(Deserialize, Clone, Copy, Serialize)]
pub struct AutoAdjustLimits {
    #[serde(rename = "MinFreqOffset")]
    pub min_freq_offset: f32,

    #[serde(rename = "MaxForwardSteps")]
    pub max_forward_steps: u32,

    #[serde(rename = "FastForwardStepLimit")]
    pub fast_forward_step_limit: u32,

    #[serde(rename = "EdgeDetectSintervalSt")]
    pub edge_detect_interval: u32,
}

#[derive(Deserialize, Clone, Serialize)]
pub struct I2CCommand {
    #[serde(rename = "Addr")]
    pub addr: u8,

    #[serde(rename = "Data")]
    pub data: Vec<u8>,
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

    #[serde(rename = "BurnLaserS")]
    pub burn_laser_pump_power: f32,

    #[serde(rename = "BurnLaserA")]
    pub burn_laser_power: f32,

    #[serde(rename = "BurnLaserB")]
    pub burn_laser_frequency: u32,

    #[serde(rename = "BurnLaserF")]
    pub burn_laser_feedrate: f32,

    #[serde(rename = "SoftModeSMultiplier")]
    pub soft_mode_s_multiplier: f32,

    #[serde(rename = "TotalVerticalSteps")]
    pub total_vertical_steps: u32,

    #[serde(rename = "FreqmeterOffset")]
    pub freqmeter_offset: f32,

    #[serde(rename = "WorkingOffsetPPM")]
    pub working_offset_ppm: f32,

    #[serde(rename = "TargetFreqCenter")]
    pub target_freq_center: f32,

    #[serde(rename = "UpdateIntervalMs")]
    pub update_interval_ms: u32,

    #[serde(rename = "SwitchChannelDelayMs")]
    pub switch_channel_delay_ms: u32,

    #[serde(rename = "DisplayPointsCount")]
    pub display_points_count: usize,

    #[serde(rename = "ForecastConfig")]
    pub forecast_config: ForecastConfig,

    #[serde(rename = "CooldownTimeMs")]
    pub cooldown_time_ms: u32,

    #[serde(rename = "AutoAdjustLimits")]
    pub auto_adjust_limits: AutoAdjustLimits,

    #[serde(rename = "StableVal")]
    pub stable_val: f32,

    #[serde(rename = "ResonatorsPlacement")]
    pub resonator_placement: Vec<ResonatroPlacement>,

    #[serde(rename = "I2CCommands")]
    pub i2c_commands: Vec<I2CCommand>,
}

impl Config {
    fn get_path() -> PathBuf {
        use std::path;

        if let Some(base_dirs) = directories::BaseDirs::new() {
            base_dirs
                .config_dir()
                .join(path::Path::new("laser-precision-adjust"))
                .join(path::Path::new("config.json"))
        } else {
            panic!("Failed to get config directory!");
        }
    }

    pub fn load() -> (Self, PathBuf) {
        let path = Self::get_path();
        if let Ok(contents) = std::fs::read_to_string(path.clone()) {
            (serde_json::from_str::<Config>(&contents).unwrap(), path)
        } else {
            panic!(
                "Failed to read {:?} file! Please copy config.json.example and fill it!",
                path
            );
        }
    }

    pub fn save(
        &mut self,
        target_freq_override: f32,
        freqmeter_offset_hz_override: f32,
        working_offset_ppm_override: f32,
    ) {
        tracing::debug!("Save settings");

        self.target_freq_center = target_freq_override;
        self.freqmeter_offset = freqmeter_offset_hz_override;
        self.working_offset_ppm = working_offset_ppm_override;

        let path = Self::get_path();

        match std::fs::File::create(path) {
            Ok(f) => serde_json::to_writer_pretty(f, self).expect("Failed to save settings"),
            Err(e) => tracing::error!("Faled to save settings: {e}"),
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
        writeln!(f, "SoftModeSMultiplier: {}", self.soft_mode_s_multiplier)?;
        writeln!(f, "VerticalStep: {}", self.total_vertical_steps)?;
        writeln!(f, "FreqmeterOffset: {}", self.freqmeter_offset)?;
        writeln!(f, "WorkingOffsetPPM: {}", self.working_offset_ppm)?;
        writeln!(f, "TargetFreqCenter: {}", self.target_freq_center)?;
        writeln!(f, "UpdateIntervalMs: {}", self.update_interval_ms)?;
        writeln!(f, "SwitchChannelDelayMs: {}", self.switch_channel_delay_ms)?;
        writeln!(f, "DisplayPointsCount: {}", self.display_points_count)?;

        writeln!(f, "ForecastConfig:")?;
        writeln!(f, "  MinFreqGrow: {}", self.forecast_config.min_freq_grow)?;
        writeln!(f, "  MaxFreqGrow: {}", self.forecast_config.max_freq_grow)?;
        writeln!(
            f,
            "  MedianFreqGrow: {}",
            self.forecast_config.median_freq_grow
        )?;

        writeln!(f, "CooldownTimeMs: {}", self.cooldown_time_ms)?;

        writeln!(f, "AutoAdjustLimits:")?;
        writeln!(
            f,
            "  MinFreqOffset: {}",
            self.auto_adjust_limits.min_freq_offset
        )?;
        writeln!(
            f,
            "  MaxForwardSteps: {}",
            self.auto_adjust_limits.max_forward_steps
        )?;
        writeln!(
            f,
            "  FastForwardStepLimit: {}",
            self.auto_adjust_limits.fast_forward_step_limit
        )?;
        writeln!(
            f,
            "  EdgeDetectSintervalSt: {}",
            self.auto_adjust_limits.edge_detect_interval
        )?;

        writeln!(f, "StableVal: {}", self.stable_val)?;

        // write resonators placement as a table
        writeln!(f, "ResonatorsPlacement:")?;
        writeln!(
            f,
            "  Center\t| Width\t| Height\t| MulS\t| MulA\t| MulB\t| MulF"
        )?;
        writeln!(
            f,
            "  ------\t| -----\t| ------\t| ----\t| ----\t| ----\t| ----"
        )?;
        for placement in &self.resonator_placement {
            writeln!(
                f,
                "  X{} Y{}\t| {}\t| {}\t| {:?}\t| {:?}\t| {:?}\t| {:?}",
                placement.x,
                placement.y,
                placement.w,
                placement.h,
                placement.mul_laser_pump_power,
                placement.mul_laser_power,
                placement.mul_laser_pwm,
                placement.mul_laser_feedrate
            )?;
        }
        Ok(())
    }
}
