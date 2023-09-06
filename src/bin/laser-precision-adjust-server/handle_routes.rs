use std::{sync::Arc, time::SystemTime};

use axum::{extract::State, response::IntoResponse, Json};
use axum_template::{Key, RenderHtml};
use laser_precision_adjust::Config;

use serde::{Deserialize, Serialize};
use tokio::sync::Mutex;

use crate::AppEngine;

#[derive(Deserialize, Debug)]
pub struct ControlRequest {}

#[derive(Serialize, Debug)]
pub struct ControlResult {
    pub success: bool,
    pub error: Option<String>,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct StateResult {
    #[serde(rename = "TimesTamp")]
    timestamp: u128,
    #[serde(rename = "SelectedChannel")]
    seleced_channel: u32,
    #[serde(rename = "CurrentFreq")]
    current_freq: f32,
    #[serde(rename = "TargetFreq")]
    target_freq: f32,
    #[serde(rename = "WorkOffsetHz")]
    work_offset_hz: f32,
}

pub(super) async fn handle_work(State(engine): State<AppEngine>) -> impl IntoResponse {
    #[derive(Serialize)]
    struct Rezonator {
        pub step: u32,
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
            step: i,
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

// Сюда будут поступать команды от веб-интерфейса
pub(super) async fn handle_control(
    State(config): State<Config>,
    Json(payload): Json<ControlRequest>,
) -> Json<ControlResult> {
    tracing::trace!("handle_control: {:?}", payload);

    Json(ControlResult {
        success: true,
        error: None,
    })
}

// Сюда будут поступать запросы на состояние от веб-интерфейса
pub(super) async fn handle_state(
    State(config): State<Config>,
    State(start_time): State<SystemTime>,
    State(adjust_target): State<Arc<Mutex<f32>>>,
) -> impl IntoResponse {
    tracing::trace!("handle_state");

    let a = 1.5;

    let stream = async_stream::stream! {
        loop {
            let tmp = SystemTime::now().duration_since(start_time).unwrap().as_millis() as f32;
            let freq_target = adjust_target.lock().await.clone();

            yield StateResult {
                timestamp: SystemTime::now().duration_since(start_time).unwrap().as_millis(),
                seleced_channel: 0,
                current_freq: freq_target + a * tmp.sin(),
                target_freq: freq_target,
                work_offset_hz: freq_target * config.working_offset_ppm / 1_000_000.0,
            };

            // sleep for 50 milliseconds
            tokio::time::sleep(std::time::Duration::from_millis(100)).await;
        }
    };

    axum_streams::StreamBodyAs::json_nl(stream)
}
