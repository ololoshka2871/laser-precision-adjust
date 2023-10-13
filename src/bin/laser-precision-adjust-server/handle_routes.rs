use std::{
    borrow::Borrow,
    cmp::min,
    collections::HashSet,
    sync::Arc,
    time::{Duration, SystemTime},
};

use axum::{
    extract::{Path, State},
    http::StatusCode,
    response::IntoResponse,
    Json,
};
use axum_template::{Key, RenderHtml};
use laser_precision_adjust::{
    box_plot::BoxPlot, predict::Predictor, Config, DataPoint, IDataPoint, PrecisionAdjust2,
};

use num_traits::Float;
use serde::{Deserialize, Serialize};

use tokio::sync::Mutex;

use crate::{
    auto_adjust_single_controller::AutoAdjustSingleController, AdjustConfig, AppEngine,
    ChannelState,
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

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct Prediction {
    pub start_offset: usize,

    pub minimal: f64,

    pub maximal: f64,

    pub median: f64,
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

    #[serde(rename = "InitialFreq", skip_serializing_if = "Option::is_none")]
    initial_freq: Option<f32>,

    #[serde(rename = "WorkOffsetHz")]
    work_offset_hz: f32,

    #[serde(rename = "CurrentStep")]
    channel_step: u32,

    #[serde(rename = "Points")]
    points: Vec<(f64, f64)>,

    #[serde(rename = "Prediction", skip_serializing_if = "Option::is_none")]
    prediction: Option<Prediction>,

    #[serde(rename = "CloseTimestamp", skip_serializing_if = "Option::is_none")]
    close_timestamp: Option<u128>,

    #[serde(rename = "Aproximations")]
    aproximations: Vec<Vec<(f64, f64)>>,

    #[serde(rename = "IsAutoAdjustBusy")]
    is_auto_adjust_busy: bool,

    #[serde(rename = "StatusCode")]
    status_code: RezStatus,

    #[serde(rename = "RestartMarker")]
    restart_marker: bool,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct UpdateConfigValues {
    #[serde(rename = "TargetFreq", skip_serializing_if = "Option::is_none")]
    target_freq: Option<f32>,

    #[serde(rename = "WorkOffsetHz", skip_serializing_if = "Option::is_none")]
    work_offset_hz: Option<f32>,
}

#[derive(Serialize, Deserialize, Clone, Copy, Debug)]
enum RezStatus {
    #[serde(rename = "unknown")]
    Unknown,
    #[serde(rename = "upper")]
    UpperBound,
    #[serde(rename = "ok")]
    Ok,
    #[serde(rename = "lower")]
    LowerBound,
    #[serde(rename = "lowerest")]
    LowerLimit,
}

struct Limits {
    upper_limit: f32,
    lower_limit: f32,
    ultra_low_limit: f32,
}

impl Limits {
    pub fn to_status(&self, f: f32) -> RezStatus {
        if f.is_nan() || f == 0.0 {
            RezStatus::Unknown
        } else if f < self.ultra_low_limit {
            RezStatus::LowerLimit
        } else if f < self.lower_limit {
            RezStatus::LowerBound
        } else if f > self.upper_limit {
            RezStatus::UpperBound
        } else {
            RezStatus::Ok
        }
    }

    pub fn to_status_icon(&self, f: f32) -> &'static str {
        if f.is_nan() || f == 0.0 {
            "-"
        } else if f < self.ultra_low_limit {
            "▼"
        } else if f < self.lower_limit {
            "▽"
        } else if f > self.upper_limit {
            "▲"
        } else {
            "◇"
        }
    }

    pub fn ppm(&self, f: f32) -> f32 {
        let f_center = (self.lower_limit + self.upper_limit) / 2.0;
        (f - f_center) / f_center * 1_000_000.0
    }

    pub fn from_config(target: f32, config: &Config) -> Self {
        let ppm2hz = target * config.working_offset_ppm / 1_000_000.0;
        Self {
            upper_limit: target + ppm2hz,
            lower_limit: target - ppm2hz,
            ultra_low_limit: target - config.auto_adjust_limits.min_freq_offset,
        }
    }
}

pub(super) async fn handle_work(
    State(channels): State<Arc<Mutex<Vec<ChannelState>>>>,
    State(engine): State<AppEngine>,
    State(freqmeter_config): State<Arc<Mutex<AdjustConfig>>>,
    State(select_channel_blocked): State<Arc<Mutex<bool>>>,
    State(config): State<Config>,
) -> impl IntoResponse {
    #[derive(Serialize)]
    struct RezData {
        current_step: u32,
        initial_freq: String,
        current_freq: String,
        points: Vec<(f64, f64)>,
        status: RezStatus,
    }

    #[derive(Serialize)]
    struct Model {
        rezonators: Vec<RezData>,
        target_freq: String,
        work_offset_hz: String,
    }

    // force release lock
    *select_channel_blocked.lock().await = false;

    let (target_freq, work_offset_hz) = {
        let guard = freqmeter_config.lock().await;
        (guard.target_freq, guard.work_offset_hz)
    };

    let limits = Limits::from_config(target_freq, &config);

    RenderHtml(
        Key("work".to_owned()),
        engine,
        Model {
            rezonators: channels
                .lock()
                .await
                .iter()
                .map(|r| {
                    let current_freq = r.points.last().cloned().unwrap_or_default().y() as f32;
                    RezData {
                        current_step: r.current_step,
                        initial_freq: r
                            .initial_freq
                            .map(|f| format2digits(f))
                            .unwrap_or("0".to_owned()),
                        current_freq: format2digits(current_freq),
                        points: r.points.iter().map(|p| (p.x(), p.y())).collect(),
                        status: limits.to_status(current_freq),
                    }
                })
                .collect(),
            target_freq: format!("{:.2}", target_freq),
            work_offset_hz: format!("{:+.2}", work_offset_hz),
        },
    )
}

pub(super) async fn handle_stat(
    State(channels): State<Arc<Mutex<Vec<ChannelState>>>>,
    State(config): State<Config>,
    State(freqmeter_config): State<Arc<Mutex<AdjustConfig>>>,
    State(engine): State<AppEngine>,
) -> impl IntoResponse {
    #[derive(Serialize)]
    struct RezData {
        current_step: u32,
        initial_freq: String,
        current_freq: String,
        status: RezStatus,
        ppm: String,
    }

    #[derive(Serialize)]
    struct Model {
        rezonators: Vec<RezData>,
    }

    // maybe?
    // _precision_adjust.lock().await.select_channel(None);

    let limits = Limits::from_config(freqmeter_config.lock().await.target_freq, &config);

    RenderHtml(
        Key("stat".to_owned()),
        engine,
        Model {
            rezonators: channels
                .lock()
                .await
                .iter()
                .map(|r| {
                    let current_freq = r.points.last().cloned().unwrap_or_default().y() as f32;
                    RezData {
                        current_step: r.current_step,
                        initial_freq: r
                            .initial_freq
                            .map(|f| format2digits(f))
                            .unwrap_or("0".to_owned()),
                        current_freq: format2digits(current_freq),
                        status: limits.to_status(current_freq),
                        ppm: format2digits(limits.ppm(current_freq)),
                    }
                })
                .collect(),
        },
    )
}

pub(super) async fn handle_stat_rez(
    State(predictor): State<Arc<Mutex<Predictor<f64>>>>,
    State(config): State<Config>,
    State(freqmeter_config): State<Arc<Mutex<AdjustConfig>>>,
    Path(rez_id): Path<u32>,
) -> impl IntoResponse {
    use serde_json::json;

    #[derive(Serialize)]
    struct DisplayFragment {
        points: Vec<DataPoint<f64>>,
        color_code_rgba: String,
    }

    #[derive(Serialize)]
    struct HystogramFragment {
        start: f64,
        end: f64,
        count: usize,
    }

    #[derive(Serialize)]
    struct DrawLimits {
        #[serde(rename = "UpperLimit")]
        upper_limit: f32,
        #[serde(rename = "LowerLimit")]
        lower_limit: f32,
        #[serde(rename = "Target")]
        target: f32,
    }

    impl DrawLimits {
        pub fn new(l: Limits, target: f32) -> Self {
            Self {
                upper_limit: l.upper_limit,
                lower_limit: l.lower_limit,
                target,
            }
        }
    }

    let limits = {
        let target = freqmeter_config.lock().await.target_freq;
        DrawLimits::new(Limits::from_config(target, &config), target)
    };

    let fragments = predictor.lock().await.get_fragments(rez_id, None).await;

    let mut display_fragments: Vec<DisplayFragment> = vec![];
    let mut adj_values: Vec<f64> = vec![];
    for (i, fragment) in fragments.iter().enumerate() {
        let opacity = 0.25 + ((1.0 - 0.25) / fragments.len() as f32 * (i + 1) as f32);
        display_fragments.push(DisplayFragment {
            points: fragment.points().to_vec(),
            color_code_rgba: format!("rgba(103, 145, 102, {opacity:.2})"),
        });

        adj_values.push(if let Some((a, _)) = fragment.aprox_coeffs() {
            fragment.points()[fragment.minimum_index()].y() + a - fragment.points()[0].y()
        } else {
            let box_plot = fragment.box_plot();
            box_plot.upper_bound() - fragment.points()[0].y()
        });
    }

    let interval_count = ((adj_values.len() as f64).log(10.0) * 3.0 + 1.0).floor() as u32;
    let hystogramm = if interval_count > 1 {
        adj_values.sort_by(|a, b| a.partial_cmp(b).unwrap());
        let start = adj_values.first().unwrap_or(&0.0);
        let step = adj_values
            .last()
            .map(|v| *v - start)
            .map(|v| v / interval_count as f64)
            .unwrap();

        (0..interval_count)
            .map(|interval_n| {
                let start = start + interval_n as f64 * step;
                let end = start + step;

                let count = adj_values
                    .iter()
                    .filter(|v| **v >= start && **v <= end)
                    .count();
                HystogramFragment { start, end, count }
            })
            .collect::<Vec<_>>()
    } else {
        vec![]
    };

    Json(json!({
            "DisplayFragments": display_fragments,
            "Hystogramm": hystogramm,
            "Limits": limits,
    }))
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

// Сюда будут поступать команды от веб-интерфейса
pub(super) async fn handle_control(
    Path(path): Path<String>,
    State(config): State<Config>,
    State(channels): State<Arc<Mutex<Vec<ChannelState>>>>,
    State(status_rx): State<tokio::sync::watch::Receiver<laser_precision_adjust::Status>>,
    State(precision_adjust): State<Arc<Mutex<PrecisionAdjust2>>>,
    State(select_channel_blocked): State<Arc<Mutex<bool>>>,
    State(auto_adjust_ctrl): State<Arc<Mutex<AutoAdjustSingleController>>>,
    State(predictor): State<Arc<Mutex<Predictor<f64>>>>,
    State(freqmeter_config): State<Arc<Mutex<AdjustConfig>>>,
    State(auto_adjust_all_ctrl): State<Arc<Mutex<crate::auto_adjust_all::AutoAdjustAllController>>>,
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

            if let Err(e) = precision_adjust.lock().await.burn().await {
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
                    let median = BoxPlot::new(
                        &channel.points[..channel.points.len() - points_to_read]
                            .iter()
                            .map(|p| p.y())
                            .collect::<Vec<_>>(),
                    )
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
            match guard.adjust(freqmeter_config.lock().await.target_freq) {
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

// Сюда будут поступать запросы на состояние от веб-интерфейса
pub(super) async fn handle_state(
    State(channels): State<Arc<Mutex<Vec<ChannelState>>>>,
    State(config): State<Config>,
    State(freqmeter_config): State<Arc<Mutex<AdjustConfig>>>,
    State(mut status_rx): State<tokio::sync::watch::Receiver<laser_precision_adjust::Status>>,
    State(close_timestamp): State<Arc<Mutex<Option<u128>>>>,
    State(predictor): State<Arc<Mutex<Predictor<f64>>>>,
    State(auto_adjust_ctrl): State<Arc<Mutex<AutoAdjustSingleController>>>,
) -> impl IntoResponse {
    const MAX_POINTS: usize = 100;

    let mut counter = 0;

    let stream = async_stream::stream! {
        loop {
            counter += 1;

            status_rx.changed().await.ok();

            let status = status_rx.borrow().clone();
            let freq_target = freqmeter_config.lock().await.target_freq;

            let timestamp = status.since_start.as_millis();
            let (initial_freq, points) = {
                let mut channels = channels.lock().await;
                let channel = channels.get_mut(status.current_channel as usize).unwrap();
                channel.points.push(DataPoint::new(timestamp as f64, status.current_frequency as f64));
                if channel.points.len() > config.display_points_count {
                    channel.points.remove(0);
                }
                (channel.initial_freq, channel.points.clone())
            };

            const MEDIAN_LEN: usize = 5;
            let (aproximations, mut prediction) = if points.len() > MEDIAN_LEN {
                let median = BoxPlot::new(&points[(points.len() - MEDIAN_LEN)..].iter().map(|p| p.y()).collect::<Vec<_>>()).median();
                get_prediction(predictor.lock().await.borrow(),
                               status.current_channel,
                               median,
                               points.len() - MEDIAN_LEN,
                               points.first().unwrap().x()
                              ).await
            } else {
                (vec![], None)
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

            let is_auto_adjust_busy = auto_adjust_ctrl
                .lock()
                .await
                .current_state()
                .await != crate::auto_adjust_single_controller::State::Idle;

            let points = if points.len() < config.display_points_count {
                let mut p = vec![DataPoint::<f64>::nan(); config.display_points_count as usize];
                p[config.display_points_count - points.len()..].copy_from_slice(&points);
                p
            } else {
                points
            };

            // update start offset
            prediction.as_mut().map(|p| p.start_offset = config.display_points_count - MEDIAN_LEN);

            // status code
            let limits = Limits::from_config(freq_target, &config);

            yield StateResult {
                timestamp,
                seleced_channel: status.current_channel,
                current_freq: status.current_frequency,
                target_freq: freq_target,
                work_offset_hz: freq_target * config.working_offset_ppm / 1_000_000.0,
                channel_step: status.current_step,
                initial_freq,
                points: points.iter().map(|p| (p.x(), p.y())).collect(),
                close_timestamp,
                prediction,
                aproximations,
                is_auto_adjust_busy,
                status_code: limits.to_status(status.current_frequency),
                restart_marker: counter == MAX_POINTS
            };

            if counter > MAX_POINTS {
                break;
            }
        }
    };

    axum_streams::StreamBodyAs::json_nl(stream)
}

pub(super) async fn handle_auto_adjust(
    State(engine): State<AppEngine>,
    State(freqmeter_config): State<Arc<Mutex<AdjustConfig>>>,
    State(channels): State<Arc<Mutex<Vec<ChannelState>>>>,
    State(auto_adjust_all_ctrl): State<Arc<Mutex<crate::auto_adjust_all::AutoAdjustAllController>>>,
) -> impl IntoResponse {
    #[derive(Serialize)]
    struct Model {
        target_freq: String,
        work_offset_hz: String,
        rezonators: Vec<u32>,
        stage: String,
    }

    let (target_freq, work_offset_hz) = {
        let guard = freqmeter_config.lock().await;
        (guard.target_freq, guard.work_offset_hz)
    };

    let model = Model {
        target_freq: format!("{:.2}", target_freq),
        work_offset_hz: format!("{:+.2}", work_offset_hz),
        rezonators: vec![0; channels.lock().await.len()],
        stage: auto_adjust_all_ctrl
            .lock()
            .await
            .get_status()
            .status
            .to_string(),
    };

    RenderHtml(Key("auto".to_owned()), engine, model)
}

pub(super) async fn handle_auto_adjust_status(
    State(auto_adjust_all_ctrl): State<Arc<Mutex<crate::auto_adjust_all::AutoAdjustAllController>>>,
) -> impl IntoResponse {
    use crate::auto_adjust_all::ProgressReport;

    const MAX_STEPS: usize = 100;

    #[derive(Serialize)]
    struct AutoAdjustStatusReport {
        progress_string: String,
        report: ProgressReport,
        reset_marker: bool,
    }

    match auto_adjust_all_ctrl.lock().await.subscribe() {
        Some(mut rx) => {
            let mut counter = 0;

            let stream = async_stream::stream! {
                loop {
                    counter += 1;
                    let reset_marker = counter == MAX_STEPS;

                    if let Err(e) = rx.changed().await {
                        tracing::error!("Autoadjust status break: {}", e);
                        break;
                    }

                    let report = rx.borrow().clone();
                    yield AutoAdjustStatusReport {
                        progress_string: report.status.to_string(),
                        report,
                        reset_marker,
                    };

                    if reset_marker {
                        break;
                    }
                }
            };
            axum_streams::StreamBodyAs::json_nl(stream).into_response()
        }
        None => Json(ControlResult::error("Autoadjust not active".to_owned())).into_response(),
    }
}

// генерация отчета
pub(super) async fn handle_generate_report(
    State(engine): State<AppEngine>,
    State(channels): State<Arc<Mutex<Vec<ChannelState>>>>,
    State(config): State<Config>,
    State(freqmeter_config): State<Arc<Mutex<AdjustConfig>>>,
    Path(part_id): Path<String>,
) -> impl IntoResponse {
    use chrono::{DateTime, Local};

    #[derive(Serialize)]
    struct RezInfo {
        start: String,
        end: String,
        ppm: String,
        ok: String,
    }

    #[derive(Serialize)]
    struct Model {
        part_id: String,
        date: String,

        freq_target: String,
        ppm: String,
        f_min: String,
        f_max: String,
        work_offset_hz: String,

        rezonators: Vec<RezInfo>,
    }

    let (freq_target, work_offset_hz) = {
        let guard = freqmeter_config.lock().await;
        (guard.target_freq, guard.work_offset_hz)
    };

    let limits = Limits::from_config(freq_target, &config);

    let model = Model {
        part_id: part_id.clone(),
        date: {
            let system_time = SystemTime::now();
            let datetime: DateTime<Local> = system_time.into();
            datetime.format("%d.%m.%Y %T").to_string()
        },

        freq_target: format2digits(freq_target),
        ppm: format2digits(config.working_offset_ppm),
        f_min: format2digits(limits.lower_limit),
        f_max: format2digits(limits.upper_limit),
        work_offset_hz: format2digits(work_offset_hz),

        rezonators: channels
            .lock()
            .await
            .iter()
            .map(|r| {
                let current_freq = r.points.last().cloned().unwrap_or_default().y() as f32;
                RezInfo {
                    start: r
                        .initial_freq
                        .map(|f| format2digits(f))
                        .unwrap_or("-".to_owned()),
                    end: format2digits(current_freq),
                    ppm: format2digits(limits.ppm(current_freq)),
                    ok: limits.to_status_icon(current_freq).to_owned(),
                }
            })
            .collect(),
    };

    RenderHtml(Key("report.html".to_owned()), engine, model).into_response()
}

//-----------------------------------------------------------------------------

async fn get_prediction<T>(
    predictor: &Predictor<T>,
    channel: u32,
    f_start: T,
    start_offset: usize,
    start_timeestamp: f64,
) -> (Vec<Vec<(T, T)>>, Option<Prediction>)
where
    T: Float + num_traits::FromPrimitive + csaps::Real + nalgebra::RealField + Serialize + 'static,
{
    let prediction = predictor
        .get_prediction(channel, f_start)
        .await
        .map(|pr| unsafe {
            Prediction {
                start_offset,
                maximal: pr.maximal.to_f64().unwrap_unchecked(),
                minimal: pr.minimal.to_f64().unwrap_unchecked(),
                median: pr.median.to_f64().unwrap_unchecked(),
            }
        });

    let fragments = predictor
        .get_fragments(channel, Some(start_timeestamp))
        .await
        .iter()
        .map(|fragment| {
            fragment
                .evaluate()
                .into_iter()
                .map(|p| (p.x(), p.y()))
                .collect()
        })
        .collect();

    (fragments, prediction)
}

fn format2digits(v: f32) -> String {
    format!("{:.2}", v)
}
