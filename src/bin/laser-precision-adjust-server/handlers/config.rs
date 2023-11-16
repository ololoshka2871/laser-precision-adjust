use std::sync::Arc;

use axum::{extract::State, http::StatusCode, response::IntoResponse, Json};
use axum_template::{Key, RenderHtml};
use laser_precision_adjust::{Config, PrecisionAdjust2};

use serde::{Deserialize, Serialize};

use tokio::sync::Mutex;

use crate::{AdjustConfig, AppEngine};

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct UpdateConfigValues {
    #[serde(rename = "TargetFreq", skip_serializing_if = "Option::is_none")]
    target_freq: Option<f32>,

    #[serde(rename = "WorkOffsetHz", skip_serializing_if = "Option::is_none")]
    work_offset_hz: Option<f32>,
}

pub(crate) async fn handle_config(
    State(engine): State<AppEngine>,
    State(config): State<Config>,
    State(freqmeter_config): State<Arc<Mutex<AdjustConfig>>>,
    State(config_file): State<std::path::PathBuf>,
) -> impl IntoResponse {
    #[derive(Serialize)]
    struct ConfigModel {
        pub config_file: String,
        pub config: Config,
    }

    let mut config = config.clone();
    {
        // update using current selected values
        let guard = freqmeter_config.lock().await;
        config.target_freq_center = guard.target_freq;
        config.freqmeter_offset = guard.work_offset_hz;
    }

    let model: ConfigModel = ConfigModel {
        config_file: config_file.to_string_lossy().to_string(),
        config,
    };

    RenderHtml(Key("config".to_owned()), engine, model)
}

pub(crate) async fn handle_update_config(
    State(freqmeter_config): State<Arc<Mutex<AdjustConfig>>>,
    State(precision_adjust): State<Arc<Mutex<PrecisionAdjust2>>>,
    Json(input): Json<UpdateConfigValues>,
) -> impl IntoResponse {
    tracing::debug!("handle_update_config: {:?}", input);

    if let Some(target_freq) = input.target_freq {
        if target_freq > 0.0 {
            freqmeter_config.lock().await.target_freq = target_freq;
        } else {
            return (
                StatusCode::RANGE_NOT_SATISFIABLE,
                "TargetFreq Должен быть больше 0",
            );
        }
    }

    if let Some(work_offset_hz) = input.work_offset_hz {
        freqmeter_config.lock().await.work_offset_hz = work_offset_hz;
        precision_adjust
            .lock()
            .await
            .set_freq_meter_offset(work_offset_hz)
            .await;
    }

    (StatusCode::OK, "Done")
}
