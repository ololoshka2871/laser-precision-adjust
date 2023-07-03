use serde::Deserialize;

#[derive(Deserialize)]
pub struct ResonatroPlacement {
    #[serde(rename = "Xcenter")]
    pub x: f64,

    #[serde(rename = "Ycenter")]
    pub y: f64,

    #[serde(rename = "Width")]
    pub w: f64,

    #[serde(rename = "Height")]
    pub h: f64,
}

#[derive(Deserialize)]
pub struct Config {
    #[serde(rename = "LaserSetupPort")]
    pub laser_setup_port: String,

    #[serde(rename = "LaserControlPort")]
    pub laser_control_port: String,

    #[serde(rename = "KosaPort")]
    pub kosa_port: String,

    #[serde(rename = "PortTimeoutMs")]
    pub port_timeout_ms: u64,

    #[serde(rename = "ResonatorsPlacement")]
    pub resonator_placement: Vec<ResonatroPlacement>,
}

impl Config {
    pub fn load() -> Self {
        use std::path;

        if let Some(base_dirs) = directories::BaseDirs::new() {
            let path = base_dirs
                .config_dir()
                .join(path::Path::new("laser-precision-adjust"))
                .join(path::Path::new("config.json"));

            if let Ok(contents) = std::fs::read_to_string(path.clone()) {
                serde_json::from_str::<Config>(&contents).unwrap()
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
        writeln!(f, "KosaPort: {}", self.kosa_port)?;

        // write resonators placement as a table
        writeln!(f, "ResonatorsPlacement:")?;
        writeln!(f, "  Center\t| Width\t| Height")?;
        writeln!(f, "  ------\t| -----\t| ------")?;
        for placement in &self.resonator_placement {
            writeln!(f, "  X{} Y{}\t| {}\t| {}", placement.x, placement.y, placement.w, placement.h)?;
        }
        Ok(())
    }
}