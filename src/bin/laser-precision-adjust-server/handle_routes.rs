use std::{cmp::min, collections::HashSet, sync::Arc, time::Duration};

use axum::{
    extract::{Path, State},
    http::StatusCode,
    response::IntoResponse,
    Json,
};
use axum_template::{Key, RenderHtml};
use laser_precision_adjust::{Config, PrecisionAdjust};

use serde::{Deserialize, Serialize};
use tokio::sync::Mutex;

use crate::predict::Prediction;
use crate::{AdjustConfig, AppEngine, ChannelState, DataPoint};

#[derive(Deserialize, Debug)]
pub struct ControlRequest {
    #[serde(rename = "Channel")]
    channel: Option<u32>,

    #[serde(rename = "CameraAction")]
    camera_action: Option<String>,

    #[serde(rename = "TargetPosition")]
    target_position: Option<i32>,

    #[serde(rename = "MoveOffset")]
    move_offset: Option<i32>,
}

#[derive(Serialize, Debug, Default)]
pub struct ControlResult {
    success: bool,
    error: Option<String>,
    message: Option<String>,
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
    initial_freq: Option<f32>,

    #[serde(rename = "WorkOffsetHz")]
    work_offset_hz: f32,

    #[serde(rename = "CurrentStep")]
    channel_step: u32,

    #[serde(rename = "Points")]
    points: Vec<(f64, f64)>,

    #[serde(rename = "Prediction")]
    prediction: Option<Prediction<f64>>,

    #[serde(rename = "CloseTimestamp")]
    close_timestamp: Option<u128>,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct UpdateConfigValues {
    #[serde(rename = "TargetFreq")]
    target_freq: Option<f32>,

    #[serde(rename = "WorkOffsetHz")]
    work_offset_hz: Option<f32>,
}

pub(super) async fn handle_work(
    State(channels): State<Arc<Mutex<Vec<ChannelState>>>>,
    State(engine): State<AppEngine>,
    State(freqmeter_config): State<Arc<Mutex<AdjustConfig>>>,
    State(select_channel_blocked): State<Arc<Mutex<bool>>>,
) -> impl IntoResponse {
    #[derive(Serialize)]
    struct RezData {
        current_step: u32,
        initial_freq: String,
        current_freq: String,
        points: Vec<(f64, f64)>,
    }

    #[derive(Serialize)]
    struct Model {
        resonators: Vec<RezData>,
        target_freq: String,
        work_offset_hz: String,
    }

    // force release lock
    *select_channel_blocked.lock().await = false;

    let (target_freq, work_offset_hz) = {
        let guard = freqmeter_config.lock().await;
        (guard.target_freq, guard.work_offset_hz)
    };

    RenderHtml(
        Key("work".to_owned()),
        engine,
        Model {
            resonators: channels
                .lock()
                .await
                .iter()
                .map(|r| RezData {
                    current_step: r.current_step,
                    initial_freq: if let Some(initial_freq) = r.initial_freq {
                        format!("{:.2}", initial_freq)
                    } else {
                        "".to_owned()
                    },
                    current_freq: format!("{:.2}", r.current_freq),
                    points: r.points.iter().map(|p| (p.x, p.y)).collect(),
                })
                .collect(),
            target_freq: format!("{:.2}", target_freq),
            work_offset_hz: format!("{:.2}", work_offset_hz),
        },
    )
}

pub(super) async fn handle_stat(State(engine): State<AppEngine>) -> impl IntoResponse {
    RenderHtml(Key("stat".to_owned()), engine, ())
}

pub(super) async fn handle_config(
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

pub(super) async fn handle_update_config(
    State(freqmeter_config): State<Arc<Mutex<AdjustConfig>>>,
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
    }

    (StatusCode::OK, "Done")
}

// Сюда будут поступать команды от веб-интерфейса
pub(super) async fn handle_control(
    Path(path): Path<String>,
    State(config): State<Config>,
    State(channels): State<Arc<Mutex<Vec<ChannelState>>>>,
    State(status_rx): State<tokio::sync::watch::Receiver<laser_precision_adjust::Status>>,
    State(precision_adjust): State<Arc<Mutex<PrecisionAdjust>>>,
    State(select_channel_blocked): State<Arc<Mutex<bool>>>,
    Json(payload): Json<ControlRequest>,
) -> impl IntoResponse {
    let ok_result = Json(ControlResult {
        success: true,
        error: None,
        ..Default::default()
    });
    let status = status_rx.borrow().clone();

    tracing::debug!("Handle control: {}: {:?}", path, payload);

    match path.as_str() {
        "select" => {
            if *select_channel_blocked.lock().await {
                return Json(ControlResult {
                    success: false,
                    error: Some("Операция временно недоступна".to_owned()),
                    ..Default::default()
                })
                .into_response();
            }

            if let Some(ch) = payload.channel {
                tracing::info!("Select channel {}", ch);

                let move_to_pos = channels.lock().await[ch as usize].current_step;

                let mut lock = precision_adjust.lock().await;

                if let Err(e) = lock.select_channel(ch).await {
                    return Json(ControlResult {
                        success: false,
                        error: Some(format!("Не удалось переключить канал: {:?}", e)),
                        ..Default::default()
                    })
                    .into_response();
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
                            ..Default::default()
                        })
                        .into_response();
                    }
                }

                // swtitch delay
                tokio::time::sleep(Duration::from_millis(min(
                    (config.update_interval_ms * 5) as u64,
                    500,
                )))
                .await;

                ok_result.into_response()
            } else {
                Json(ControlResult {
                    success: false,
                    error: Some("Не указано поле 'channel'".to_owned()),
                    ..Default::default()
                })
                .into_response()
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
                                ..Default::default()
                            })
                            .into_response();
                        } else {
                            ok_result.into_response()
                        }
                    }
                    "open" => {
                        if let Err(e) = precision_adjust.lock().await.open_camera().await {
                            return Json(ControlResult {
                                success: false,
                                error: Some(format!("Не удалось открыть камеру: {:?}", e)),
                                ..Default::default()
                            })
                            .into_response();
                        } else {
                            ok_result.into_response()
                        }
                    }
                    "vac" => {
                        if let Err(e) = precision_adjust.lock().await.close_camera(true).await {
                            return Json(ControlResult {
                                success: false,
                                error: Some(format!("Не удалось включить вакуум: {:?}", e)),
                                ..Default::default()
                            })
                            .into_response();
                        } else {
                            ok_result.into_response()
                        }
                    }
                    act => Json(ControlResult {
                        success: false,
                        error: Some(format!("Неизвестная команда: {}", act)),
                        ..Default::default()
                    })
                    .into_response(),
                }
            } else {
                Json(ControlResult {
                    success: false,
                    error: Some("Не указано поле действия 'CameraAction'".to_owned()),
                    ..Default::default()
                })
                .into_response()
            }
        }
        "move" => {
            if *select_channel_blocked.lock().await {
                return Json(ControlResult {
                    success: false,
                    error: Some("Операция временно недоступна".to_owned()),
                    ..Default::default()
                })
                .into_response();
            }

            if let Some(target_pos) = payload.target_position {
                if target_pos < 0 {
                    return Json(ControlResult {
                        success: false,
                        error: Some("Target position < 0".to_owned()),
                        ..Default::default()
                    })
                    .into_response();
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
                        ..Default::default()
                    })
                    .into_response();
                } else {
                    channels.lock().await[status.current_channel as usize].current_step =
                        target_pos as u32;
                    ok_result.into_response()
                }
            } else if let Some(move_offset) = payload.move_offset {
                tracing::info!("Move by {}", move_offset);
                if let Err(e) = precision_adjust.lock().await.step(move_offset).await {
                    return Json(ControlResult {
                        success: false,
                        error: Some(format!("Не сместиться на {} шагов: {:?}", move_offset, e)),
                        ..Default::default()
                    })
                    .into_response();
                } else {
                    channels.lock().await[status.current_channel as usize].current_step =
                        (status.current_step as i32 + move_offset) as u32;
                    ok_result.into_response()
                }
            } else {
                Json(ControlResult {
                    success: false,
                    error: Some("No 'TargetPosition' selected".to_owned()),
                    ..Default::default()
                })
                .into_response()
            }
        }
        "burn" => {
            if *select_channel_blocked.lock().await {
                return Json(ControlResult {
                    success: false,
                    error: Some("Операция временно недоступна".to_owned()),
                    ..Default::default()
                })
                .into_response();
            }

            let autostep = payload.move_offset.unwrap_or(0);

            tracing::info!("Burn with autostep {}", autostep);

            if let Err(e) = precision_adjust.lock().await.burn().await {
                return Json(ControlResult {
                    success: false,
                    error: Some(format!("Не удалось сжечь: {:?}", e)),
                    ..Default::default()
                })
                .into_response();
            }

            if autostep != 0 {
                if let Err(e) = precision_adjust.lock().await.step(autostep).await {
                    return Json(ControlResult {
                        success: false,
                        error: Some(format!(
                            "Не удалось сместиться на {} шагов: {:?}",
                            autostep, e
                        )),
                        ..Default::default()
                    })
                    .into_response();
                }
            }

            ok_result.into_response()
        }
        "scan-all" => {
            {
                let mut guard = select_channel_blocked.lock().await;
                if *guard {
                    return Json(ControlResult {
                        success: false,
                        error: Some("Операция в процессе, подождите".to_owned()),
                        ..Default::default()
                    })
                    .into_response();
                } else {
                    *guard = true;
                }
            }

            let channels_count = channels.lock().await.len();

            // current selected channel
            let current_channel = status.current_channel;

            let stream = async_stream::stream! {
                const POINTS_TO_AVG: usize = 15;
                for i in 0..channels_count {
                    yield ControlResult {
                        success: true,
                        message: Some(format!("Сканирование канала: {}", i + 1)),
                        ..Default::default()
                    };

                    {
                        let mut guard = precision_adjust.lock().await;
                        let res = guard.select_channel(i as u32).await;

                        // swtitch delay
                        tokio::time::sleep(std::time::Duration::from_millis(
                            min((config.update_interval_ms * 5) as u64, 500))
                        ).await;

                        // clear history
                        {
                            let mut lock = channels.lock().await;
                            lock[i].points.clear();
                        }

                        if let Err(e) = res {
                            yield ControlResult {
                                success: false,
                                error: Some(format!("Не удалось переключить канал: {:?}", e)),
                                ..Default::default()
                            };

                            continue;
                        }
                    }

                    // sleep POINTS_TO_AVG times of update
                    tokio::time::sleep(std::time::Duration::from_millis(
                        (config.update_interval_ms * POINTS_TO_AVG as u32) as u64)
                    ).await;

                    // take last points_to_read points and calc avarage frequency -> channel initial_freq
                    {
                        let mut guard = channels.lock().await;
                        let channel = &mut guard[i];
                        let avalable_points_count = if channel.points.len() == 0 { 0 } else { channel.points.len() - 1  };
                        let points_to_read = std::cmp::min(avalable_points_count, POINTS_TO_AVG);
                        if points_to_read < POINTS_TO_AVG / 2 ||
                            (channel.points
                                .iter()
                                .rev()
                                .take(points_to_read)
                                .map(|v| v.y.to_string())
                                .collect::<HashSet<_>>().len() < POINTS_TO_AVG / 5
                        ) {
                            channel.initial_freq = None;
                        } else {
                            channel.points.remove(channel.points.len() - points_to_read);
                            let summ = channel.points
                                .iter()
                                .fold(0.0, |acc, dp| acc + dp.y);
                            channel.initial_freq = Some(summ as f32 / channel.points.len() as f32);
                        }
                    }

                    tokio::time::sleep(std::time::Duration::from_millis(
                        (config.update_interval_ms * 2) as u64)
                    ).await;
                }

                // restore selected channel
                if let Err(e) = precision_adjust.lock().await.select_channel(current_channel).await {
                    yield ControlResult {
                        success: false,
                        error: Some(format!("Не удалось переключить канал: {:?}", e)),
                        ..Default::default()
                    };
                }

                yield ControlResult {
                    success: true,
                    message: Some("Finished".to_owned()),
                    ..Default::default()
                };

                // release lock
                *select_channel_blocked.lock().await = false;
            };

            axum_streams::StreamBodyAs::json_nl(stream).into_response()
        }
        _ => {
            tracing::error!("Unknown command: {}", path);
            Json(ControlResult {
                success: false,
                error: Some("Unknown command".to_owned()),
                ..Default::default()
            })
            .into_response()
        }
    }
}

// Сюда будут поступать запросы на состояние от веб-интерфейса
pub(super) async fn handle_state(
    State(channels): State<Arc<Mutex<Vec<ChannelState>>>>,
    State(config): State<Config>,
    State(freqmeter_config): State<Arc<Mutex<AdjustConfig>>>,
    State(mut status_rx): State<tokio::sync::watch::Receiver<laser_precision_adjust::Status>>,
    State(close_timestamp): State<Arc<Mutex<Option<u128>>>>,
) -> impl IntoResponse {
    let stream = async_stream::stream! {
        loop {
            status_rx.changed().await.ok();

            let status = status_rx.borrow().clone();
            let (freq_target, work_offset_hz) = {
                let guard = freqmeter_config.lock().await;
                (guard.target_freq, guard.work_offset_hz)
            };

            let timestamp = status.since_start.as_millis();
            let (initial_freq, points) = {
                let mut channels = channels.lock().await;
                let channel = channels.get_mut(status.current_channel as usize).unwrap();
                channel.points.push(DataPoint::new(timestamp as f64, (status.current_frequency + work_offset_hz) as f64));
                if channel.points.len() > config.display_points_count {
                    channel.points.remove(0);
                }
                (channel.initial_freq, channel.points.clone())
            };

            let prediction: Option<Prediction<f64> > = points
                .last()
                .map(|p| p.y)
                .map(|y| Prediction{ minimal: y + 0.1, maximal: y + 1.0, median: y + 0.5 });

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
                current_freq: status.current_frequency + work_offset_hz,
                target_freq: freq_target,
                work_offset_hz: freq_target * config.working_offset_ppm / 1_000_000.0,
                channel_step: status.current_step,
                initial_freq,
                points: points.iter().map(|p| (p.x, p.y)).collect(),
                close_timestamp,
                prediction,
            };
        }
    };

    axum_streams::StreamBodyAs::json_nl(stream)
}
