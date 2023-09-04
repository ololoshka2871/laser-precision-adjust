use axum::{extract::State, response::IntoResponse};
use axum_template::{Key, RenderHtml};
use laser_precision_adjust::Config;

use crate::AppEngine;

pub(super) async fn handle_work(State(engine): State<AppEngine>) -> impl IntoResponse {
    RenderHtml(Key("work".to_owned()), engine, ())
}

pub(super) async fn handle_stat(State(engine): State<AppEngine>) -> impl IntoResponse {
    RenderHtml(Key("stat".to_owned()), engine, ())
}

pub(super) async fn handle_config(
    State(engine): State<AppEngine>,
    State(config): State<Config>,
    State(config_file): State<std::path::PathBuf>,
) -> impl IntoResponse {
    use serde::Serialize;

    #[derive(Serialize)]
    struct ConfigModel {
        config_file: String,
        config: Config,
    }

    let model: ConfigModel = ConfigModel {
        config_file: config_file.to_string_lossy().to_string(),
        config,
    };

    RenderHtml(Key("config".to_owned()), engine, model)
}
