use axum::{extract::State, response::IntoResponse};
use axum_template::{Key, RenderHtml};
use laser_precision_adjust::Config;

use serde::Serialize;

use crate::AppEngine;

pub(super) async fn handle_work(State(engine): State<AppEngine>) -> impl IntoResponse {
    #[derive(Serialize)]
    struct Rezonator {
        pub f_start: f32,
        pub f_end: f32,
    }

    #[derive(Serialize)]
    struct Model {
        pub resonators: Vec<Rezonator>,
    }

    let mut resonators = vec![];
    for i in 0..16 {
        resonators.push(Rezonator {
            f_start: i as f32 * 100.0,
            f_end: (i + 1) as f32 * 100.0,
        });
    }

    RenderHtml(Key("work".to_owned()), engine, Model { resonators })
}

pub(super) async fn handle_stat(State(engine): State<AppEngine>) -> impl IntoResponse {
    RenderHtml(Key("stat".to_owned()), engine, ())
}

pub(super) async fn handle_config(
    State(engine): State<AppEngine>,
    State(config): State<Config>,
    State(config_file): State<std::path::PathBuf>,
) -> impl IntoResponse {
    #[derive(Serialize)]
    struct ConfigModel {
        pub config_file: String,
        pub config: Config,
    }

    let model: ConfigModel = ConfigModel {
        config_file: config_file.to_string_lossy().to_string(),
        config,
    };

    RenderHtml(Key("config".to_owned()), engine, model)
}
