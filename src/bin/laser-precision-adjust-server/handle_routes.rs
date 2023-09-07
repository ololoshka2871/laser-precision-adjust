use std::sync::Arc;

use axum::{
    extract::{Path, State},
    response::IntoResponse,
    Json,
};
use axum_template::{Key, RenderHtml};
use laser_precision_adjust::{Config, PrecisionAdjust};

use serde::{Deserialize, Serialize};
use tokio::sync::Mutex;

use crate::{AppEngine, ChannelState};

#[derive(Deserialize, Debug)]
pub struct ControlRequest {
    #[serde(rename = "Channel")]
    pub channel: Option<u32>,

    #[serde(rename = "CameraAction")]
    pub camera_action: Option<String>,

    #[serde(rename = "TargetPosition")]
    pub target_position: Option<i32>,

    #[serde(rename = "MoveOffset")]
    pub move_offset: Option<i32>,
}

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

    #[serde(rename = "InitialFreq")]
    initial_freq: f32,

    #[serde(rename = "WorkOffsetHz")]
    work_offset_hz: f32,

    #[serde(rename = "CurrentStep")]
    channel_step: u32,

    #[serde(rename = "Points")]
    points: Vec<(u128, f32)>,

    #[serde(rename = "CloseTimestamp")]
    close_timestamp: Option<u128>,
}

pub(super) async fn handle_work(
    State(channels): State<Arc<Mutex<Vec<ChannelState>>>>,
    State(engine): State<AppEngine>,
) -> impl IntoResponse {
    #[derive(Serialize)]
    struct Model {
        pub resonators: Vec<ChannelState>,
    }

    RenderHtml(
        Key("work".to_owned()),
        engine,
        Model {
            resonators: channels.lock().await.clone(),
        },
    )
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
    Path(path): Path<String>,
    State(_config): State<Config>,
    State(channels): State<Arc<Mutex<Vec<ChannelState>>>>,
    State(status_rx): State<tokio::sync::watch::Receiver<laser_precision_adjust::Status>>,
    State(precision_adjust): State<Arc<Mutex<PrecisionAdjust>>>,
    Json(payload): Json<ControlRequest>,
) -> Json<ControlResult> {
    let ok_result = Json(ControlResult {
        success: true,
        error: None,
    });
    let status = status_rx.borrow().clone();

    tracing::debug!("Handle control: {}: {:?}", path, payload);

    match path.as_str() {
        "select" => {
            if let Some(ch) = payload.channel {
                tracing::info!("Select channel {}", ch);

                let move_to_pos = channels.lock().await[ch as usize].current_step;

                let mut lock = precision_adjust.lock().await;

                if let Err(e) = lock.select_channel(ch).await {
                    return Json(ControlResult {
                        success: false,
                        error: Some(format!("Не удалось переключить канал: {:?}", e)),
                    });
                }
                if move_to_pos != 0 {
                    tracing::info!("Restore position {}", move_to_pos);
                    if let Err(e) = lock.step(move_to_pos as i32).await {
                        return Json(ControlResult {
                            success: false,
                            error: Some(format!(
                                "Не удалось перейти к позиции {}: {:?}",
                                move_to_pos, e
                            )),
                        });
                    }
                }

                ok_result
            } else {
                Json(ControlResult {
                    success: false,
                    error: Some("Не указано поле 'channel'".to_owned()),
                })
            }
        }
        "camera" => {
            if let Some(action) = payload.camera_action {
                tracing::info!("Camera action: {}", action);
                match action.as_str() {
                    "close" => {
                        if let Err(e) = precision_adjust.lock().await.close_camera(false).await {
                            return Json(ControlResult {
                                success: false,
                                error: Some(format!("Не удалось закрыть камеру: {:?}", e)),
                            });
                        } else {
                            ok_result
                        }
                    }
                    "open" => {
                        if let Err(e) = precision_adjust.lock().await.open_camera().await {
                            return Json(ControlResult {
                                success: false,
                                error: Some(format!("Не удалось открыть камеру: {:?}", e)),
                            });
                        } else {
                            ok_result
                        }
                    }
                    "vac" => {
                        if let Err(e) = precision_adjust.lock().await.close_camera(true).await {
                            return Json(ControlResult {
                                success: false,
                                error: Some(format!("Не удалось включить вакуум: {:?}", e)),
                            });
                        } else {
                            ok_result
                        }
                    }
                    act => Json(ControlResult {
                        success: false,
                        error: Some(format!("Неизвестная команда: {}", act)),
                    }),
                }
            } else {
                Json(ControlResult {
                    success: false,
                    error: Some("Не указано поле действия 'CameraAction'".to_owned()),
                })
            }
        }
        "move" => {
            if let Some(target_pos) = payload.target_position {
                if target_pos < 0 {
                    return Json(ControlResult {
                        success: false,
                        error: Some("Target position < 0".to_owned()),
                    });
                }

                let offset = target_pos - status.current_step as i32;

                tracing::info!("Move to {}", target_pos);
                if let Err(e) = precision_adjust.lock().await.step(offset).await {
                    return Json(ControlResult {
                        success: false,
                        error: Some(format!(
                            "Не удалось перейти к позиции {}: {:?}",
                            target_pos, e
                        )),
                    });
                } else {
                    channels.lock().await[status.current_channel as usize].current_step =
                        target_pos as u32;
                    ok_result
                }
            } else if let Some(move_offset) = payload.move_offset {
                tracing::info!("Move by {}", move_offset);
                if let Err(e) = precision_adjust.lock().await.step(move_offset).await {
                    return Json(ControlResult {
                        success: false,
                        error: Some(format!("Не сместиться на {} шагов: {:?}", move_offset, e)),
                    });
                } else {
                    channels.lock().await[status.current_channel as usize].current_step =
                        (status.current_step as i32 + move_offset) as u32;
                    ok_result
                }
            } else {
                Json(ControlResult {
                    success: false,
                    error: Some("No 'TargetPosition' selected".to_owned()),
                })
            }
        }
        "burn" => {
            let autostep = payload.move_offset.unwrap_or(0);

            tracing::info!("Burn with autostep {}", autostep);

            if let Err(e) = precision_adjust.lock().await.burn().await {
                return Json(ControlResult {
                    success: false,
                    error: Some(format!("Не удалось сжечь: {:?}", e)),
                });
            }

            if autostep != 0 {
                if let Err(e) = precision_adjust.lock().await.step(autostep).await {
                    return Json(ControlResult {
                        success: false,
                        error: Some(format!(
                            "Не удалось сместиться на {} шагов: {:?}",
                            autostep, e
                        )),
                    });
                }
            }

            ok_result
        }
        "scan-all" => Json(ControlResult {
            success: true,
            error: None,
        }),
        _ => {
            tracing::error!("Unknown command: {}", path);
            Json(ControlResult {
                success: false,
                error: Some("Unknown command".to_owned()),
            })
        }
    }
}

// Сюда будут поступать запросы на состояние от веб-интерфейса
pub(super) async fn handle_state(
    State(channels): State<Arc<Mutex<Vec<ChannelState>>>>,
    State(config): State<Config>,
    State(adjust_target): State<Arc<Mutex<f32>>>,
    State(mut status_rx): State<tokio::sync::watch::Receiver<laser_precision_adjust::Status>>,
    State(close_timestamp): State<Arc<Mutex<Option<u128>>>>,
) -> impl IntoResponse {
    tracing::trace!("handle_state");

    let stream = async_stream::stream! {
        loop {
            status_rx.changed().await.ok();

            let status = status_rx.borrow().clone();
            let freq_target = adjust_target.lock().await.clone();

            let timestamp = status.since_start.as_millis();
            let (initial_freq, points) = {
                let mut channels = channels.lock().await;
                let channel = channels.get_mut(status.current_channel as usize).unwrap();
                channel.points.push((timestamp, status.current_frequency));
                if channel.points.len() > config.display_points_count {
                    channel.points.remove(0);
                }
                (channel.initial_freq, channel.points.clone())
            };

            let close_timestamp = {
                let mut close_timestamp_guard = close_timestamp.lock().await;
                match status.camera_state {
                    laser_setup_interface::CameraState::Close => {
                        if close_timestamp_guard.is_none() {
                            let res = Some(timestamp);
                            *close_timestamp_guard = res;
                            res
                        } else {
                            *close_timestamp_guard
                        }
                    },
                    laser_setup_interface::CameraState::Open => {
                        let res = None;
                            *close_timestamp_guard = res;
                            res
                    },
                }
            };

            yield StateResult {
                timestamp,
                seleced_channel: status.current_channel,
                current_freq: status.current_frequency,
                target_freq: freq_target,
                work_offset_hz: freq_target * config.working_offset_ppm / 1_000_000.0,
                channel_step: status.current_step,
                initial_freq,
                points,
                close_timestamp,
            };
        }
    };

    axum_streams::StreamBodyAs::json_nl(stream)
}
