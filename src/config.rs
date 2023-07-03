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
