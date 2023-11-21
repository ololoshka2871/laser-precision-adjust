use std::{cmp::min, collections::HashSet, sync::Arc, time::Duration};

use axum::{
    extract::{Path, State},
    response::IntoResponse,
    Json,
};

use laser_precision_adjust::{
    box_plot::BoxPlot, predict::Predictor, Config, IDataPoint, PrecisionAdjust2,
};

use serde::{Deserialize, Serialize};
use tokio::sync::Mutex;

use crate::{
    auto_adjust_all::AutoAdjustAllController,
    auto_adjust_single_controller::AutoAdjustSingleController, AdjustConfig, ChannelState,
};

#[derive(Deserialize, Debug)]
pub struct ControlRequest {
    #[serde(rename = "Channel", skip_serializing_if = "Option::is_none")]
    channel: Option<u32>,

    #[serde(rename = "CameraAction", skip_serializing_if = "Option::is_none")]
    camera_action: Option<String>,

    #[serde(rename = "TargetPosition", skip_serializing_if = "Option::is_none")]
    target_position: Option<i32>,

    #[serde(rename = "MoveOffset", skip_serializing_if = "Option::is_none")]
    move_offset: Option<i32>,
}

#[derive(Serialize, Debug, Default)]
pub struct ControlResult {
    success: bool,
    error: Option<String>,
    message: Option<String>,
}

impl ControlResult {
    pub fn new(success: bool, error: Option<String>, message: Option<String>) -> Self {
        Self {
            success,
            error,
            message,
        }
    }

    pub fn success(message: Option<String>) -> Self {
        Self::new(true, None, message)
    }

    pub fn error(err_message: String) -> Self {
        Self::new(false, Some(err_message), None)
    }
}

// Сюда будут поступать команды от веб-интерфейса
pub(crate) async fn handle_control(
    Path(path): Path<String>,
    State(config): State<Config>,
    State(channels): State<Arc<Mutex<Vec<ChannelState>>>>,
    State(status_rx): State<tokio::sync::watch::Receiver<laser_precision_adjust::Status>>,
    State(precision_adjust): State<Arc<Mutex<PrecisionAdjust2>>>,
    State(select_channel_blocked): State<Arc<Mutex<bool>>>,
    State(auto_adjust_ctrl): State<Arc<Mutex<AutoAdjustSingleController>>>,
    State(predictor): State<Arc<Mutex<Predictor<f64>>>>,
    State(freqmeter_config): State<Arc<Mutex<AdjustConfig>>>,
    State(auto_adjust_all_ctrl): State<Arc<Mutex<AutoAdjustAllController>>>,
    Json(payload): Json<ControlRequest>,
) -> impl IntoResponse {
    const POINTS_TO_AVG: usize = 15;

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
                return Json(ControlResult::error(
                    "Операция временно недоступна".to_owned(),
                ))
                .into_response();
            }

            if let Some(ch) = payload.channel {
                tracing::info!("Select channel {}", ch);

                let move_to_pos = channels.lock().await[ch as usize].current_step;

                let mut guard = precision_adjust.lock().await;

                if let Err(e) = guard.select_channel(ch).await {
                    return Json(ControlResult::error(format!(
                        "Не удалось переключить канал: {:?}",
                        e
                    )))
                    .into_response();
                }
                if move_to_pos != 0 {
                    tracing::info!("Restore position {}", move_to_pos);
                    if let Err(e) = guard.step(move_to_pos as i32).await {
                        return Json(ControlResult::error(format!(
                            "Не удалось перейти к позиции {}: {:?}",
                            move_to_pos, e
                        )))
                        .into_response();
                    }
                }

                // switch delay
                tokio::time::sleep(Duration::from_millis(min(
                    (config.update_interval_ms * 5) as u64,
                    500,
                )))
                .await;

                ok_result.into_response()
            } else {
                Json(ControlResult::error("Не указано поле 'channel'".to_owned())).into_response()
            }
        }
        "camera" => {
            if let Some(action) = payload.camera_action {
                tracing::info!("Camera action: {}", action);
                match action.as_str() {
                    "close" => {
                        let mut guard = precision_adjust.lock().await;

                        if let Err(e) = guard.close_camera(false).await {
                            return Json(ControlResult::error(format!(
                                "Не удалось закрыть камеру: {:?}",
                                e
                            )))
                            .into_response();
                        } else {
                            if let Err(e) = guard.reset().await {
                                return Json(ControlResult::error(format!(
                                    "Не удалось сбросить состояние: {:?}",
                                    e
                                )))
                                .into_response();
                            } else {
                                ok_result.into_response()
                            }
                        }
                    }
                    "open" => {
                        if let Err(e) = precision_adjust.lock().await.open_camera().await {
                            return Json(ControlResult::error(format!(
                                "Не удалось открыть камеру: {:?}",
                                e
                            )))
                            .into_response();
                        } else {
                            {
                                let mut guard = predictor.lock().await;
                                if let Err(e) = guard.save(config.data_log_file).await {
                                    tracing::error!("Save fragments error: {}", e);
                                }
                                guard.reset().await;
                            }
                            ok_result.into_response()
                        }
                    }
                    "vac" => {
                        if let Err(e) = precision_adjust.lock().await.close_camera(true).await {
                            return Json(ControlResult::error(format!(
                                "Не удалось включить вакуум: {:?}",
                                e
                            )))
                            .into_response();
                        } else {
                            ok_result.into_response()
                        }
                    }
                    act => Json(ControlResult::error(format!(
                        "Неизвестная команда: {}",
                        act
                    )))
                    .into_response(),
                }
            } else {
                Json(ControlResult::error(
                    "Не указано поле действия 'CameraAction'".to_owned(),
                ))
                .into_response()
            }
        }
        "move" => {
            if *select_channel_blocked.lock().await {
                return Json(ControlResult::error(
                    "Операция временно недоступна".to_owned(),
                ))
                .into_response();
            }

            if let Some(target_pos) = payload.target_position {
                if target_pos < 0 {
                    return Json(ControlResult::error("Target position < 0".to_owned()))
                        .into_response();
                }

                let offset = target_pos - status.current_step as i32;

                tracing::info!("Move to {}", target_pos);
                if let Err(e) = precision_adjust.lock().await.step(offset).await {
                    return Json(ControlResult::error(format!(
                        "Не удалось перейти к позиции {}: {:?}",
                        target_pos, e
                    )))
                    .into_response();
                } else {
                    channels.lock().await[status.current_channel as usize].current_step =
                        target_pos as u32;
                    ok_result.into_response()
                }
            } else if let Some(move_offset) = payload.move_offset {
                tracing::info!("Move by {}", move_offset);
                if let Err(e) = precision_adjust.lock().await.step(move_offset).await {
                    return Json(ControlResult::error(format!(
                        "Не сместиться на {} шагов: {:?}",
                        move_offset, e
                    )))
                    .into_response();
                } else {
                    channels.lock().await[status.current_channel as usize].current_step =
                        (status.current_step as i32 + move_offset) as u32;
                    ok_result.into_response()
                }
            } else {
                Json(ControlResult::error(
                    "No 'TargetPosition' selected".to_owned(),
                ))
                .into_response()
            }
        }
        "burn" => {
            if *select_channel_blocked.lock().await {
                return Json(ControlResult::error(
                    "Операция временно недоступна".to_owned(),
                ))
                .into_response();
            }

            let autostep = payload.move_offset.unwrap_or(0);

            tracing::info!("Burn with autostep {}", autostep);

            if let Err(e) = precision_adjust.lock().await.burn(false).await {
                return Json(ControlResult::error(format!("Не удалось сжечь: {:?}", e)))
                    .into_response();
            }

            if autostep != 0 {
                if let Err(e) = precision_adjust.lock().await.step(autostep).await {
                    return Json(ControlResult::error(format!(
                        "Не удалось сместиться на {} шагов: {:?}",
                        autostep, e
                    )))
                    .into_response();
                }
            }

            ok_result.into_response()
        }
        "scan-all" => {
            if let Err(e) = try_block_interface(&select_channel_blocked).await {
                return e.into_response();
            }

            let channels_count = channels.lock().await.len();

            // current selected channel
            let current_channel = status.current_channel;

            let stream = async_stream::stream! {
                for i in 0..channels_count {
                    yield ControlResult::success(Some(format!("Сканирование канала: {}", i + 1)));

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
                            yield ControlResult::error(format!("Не удалось переключить канал: {:?}", e));

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
                        if points_to_read < POINTS_TO_AVG / 3 ||
                            (channel.points[(channel.points.len() - points_to_read)..]
                                .iter()
                                .map(|v| v.y().to_string())
                                .collect::<HashSet<_>>().len() < POINTS_TO_AVG / 5
                        ) {
                            channel.initial_freq = channel.points.last().map(|p| p.y() as f32);
                        } else {
                            channel.points.remove(channel.points.len() - points_to_read);
                            let median = BoxPlot::new(&channel.points.iter().map(|p| p.y()).collect::<Vec<_>>()).median();
                            channel.initial_freq = Some(median as f32);
                        }
                    }

                    tokio::time::sleep(std::time::Duration::from_millis(
                        (config.update_interval_ms * 2) as u64)
                    ).await;
                }

                // restore selected channel
                if let Err(e) = precision_adjust.lock().await.select_channel(current_channel).await {
                    yield ControlResult::error(format!("Не удалось переключить канал: {:?}", e));
                }

                yield ControlResult::success(Some("Finished".to_owned()));

                // release lock
                *select_channel_blocked.lock().await = false;
            };

            axum_streams::StreamBodyAs::json_nl(stream).into_response()
        }
        "auto-adjust" => {
            if let Err(_) = try_block_interface(&select_channel_blocked).await {
                return match auto_adjust_ctrl.lock().await.cancel().await {
                    Ok(()) => {
                        // unblock interface
                        *select_channel_blocked.lock().await = false;

                        Json(ControlResult::success(Some(
                            "Настройка отменена".to_owned(),
                        )))
                    }
                    Err(e) => Json(ControlResult::error(format!("Неизвестная ошибка: {e}"))),
                }
                .into_response();
            }

            // update start freq
            {
                let mut guard = channels.lock().await;
                let channel = &mut guard[status.current_channel as usize];
                let avalable_points_count = if channel.points.len() == 0 {
                    0
                } else {
                    channel.points.len() - 1
                };
                let points_to_read = std::cmp::min(avalable_points_count, POINTS_TO_AVG);
                if points_to_read < POINTS_TO_AVG / 2
                    || (channel.points[(channel.points.len() - points_to_read)..]
                        .iter()
                        .map(|v| v.y().to_string())
                        .collect::<HashSet<_>>()
                        .len()
                        < POINTS_TO_AVG / 5)
                {
                    channel.initial_freq = channel.points.last().map(|p| p.y() as f32);
                } else {
                    channel.points.remove(channel.points.len() - points_to_read);
                    let median =
                        BoxPlot::new(&channel.points.iter().map(|p| p.y()).collect::<Vec<_>>())
                            .median();
                    channel.initial_freq = Some(median as f32);
                }
            }

            if let Ok(mut status_channel) = auto_adjust_ctrl
                .lock()
                .await
                .try_start(
                    status.current_channel,
                    predictor.clone(),
                    precision_adjust.clone(),
                    freqmeter_config.lock().await.target_freq,
                )
                .await
            {
                let stream = async_stream::stream! {
                    while let Some(msg) = status_channel.recv().await {
                        use crate::auto_adjust_single_controller::AutoAdjustSingleStateReport;
                        yield match msg {
                            AutoAdjustSingleStateReport::Progress(msg) => {
                                ControlResult::success(Some(msg))
                            },

                            AutoAdjustSingleStateReport::Error(e) => {
                                ControlResult::error(e)

                            },
                            AutoAdjustSingleStateReport::Finished(msg) => {
                                 ControlResult::success(Some(msg))
                            },
                        };
                    }

                    // unblock interface
                    *select_channel_blocked.lock().await = false;
                };

                axum_streams::StreamBodyAs::json_nl(stream).into_response()
            } else {
                Json(ControlResult::error("Невозможное состояние!".to_owned())).into_response()
            }
        }
        "adjust-all" => {
            let mut guard = auto_adjust_all_ctrl.lock().await;
            match guard.adjust(freqmeter_config.lock().await.target_freq).await {
                Ok(_) => Json(ControlResult::success(Some(
                    "Автонастройка начата.".to_owned(),
                )))
                .into_response(),
                Err(crate::auto_adjust_all::Error::AdjustInProgress) => {
                    if let Err(e) = guard.cancel() {
                        Json(ControlResult::error(format!(
                            "Не удалось отменить автонастройку: {e:?}"
                        )))
                        .into_response()
                    } else {
                        Json(ControlResult::success(Some(
                            "Автонастройка отменена.".to_owned(),
                        )))
                        .into_response()
                    }
                }
                Err(e) => Json(ControlResult::error(format!(
                    "Не удалось начать автонастройку: {e:?}"
                )))
                .into_response(),
            }
        }
        _ => {
            tracing::error!("Unknown command: {}", path);
            Json(ControlResult::error("Unknown command".to_owned())).into_response()
        }
    }
}

async fn try_block_interface(
    select_channel_blocked: &Mutex<bool>,
) -> Result<(), Json<ControlResult>> {
    let mut guard = select_channel_blocked.lock().await;
    if *guard {
        return Err(Json(ControlResult {
            success: false,
            error: Some("Операция в процессе, подождите".to_owned()),
            ..Default::default()
        }));
    } else {
        *guard = true;
    }
    Ok(())
}
