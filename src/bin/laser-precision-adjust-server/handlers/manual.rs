use std::{borrow::Borrow, sync::Arc, time::SystemTime};

use axum::{
    extract::{Path, State},
    response::IntoResponse,
};
use axum_template::{Key, RenderHtml};
use laser_precision_adjust::{
    box_plot::BoxPlot, predict::Predictor, Config, DataPoint, IDataPoint,
};

use num_traits::Float;
use serde::{Deserialize, Serialize};

use tokio::sync::Mutex;

use crate::{
    auto_adjust_single_controller::AutoAdjustSingleController,
    handlers::{common::format2digits, limits::Limits},
    AdjustConfig, AppEngine, ChannelState,
};

use super::limits::RezStatus;

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

pub(crate) async fn handle_work(
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

    let (target_freq, work_offset_hz, working_offset_ppm) = {
        let guard = freqmeter_config.lock().await;
        (
            guard.target_freq,
            guard.work_offset_hz,
            guard.working_offset_ppm,
        )
    };

    let limits = Limits::from_config(target_freq, &config, working_offset_ppm);

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

// Сюда будут поступать запросы на состояние от веб-интерфейса
pub(crate) async fn handle_state(
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
            let (target_freq, working_offset_ppm) = {
                let guard = freqmeter_config.lock().await;
                (
                    guard.target_freq,
                    guard.working_offset_ppm,
                )
            };

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
            let limits = Limits::from_config(target_freq, &config, working_offset_ppm);

            yield StateResult {
                timestamp,
                seleced_channel: status.current_channel,
                current_freq: status.current_frequency,
                target_freq: target_freq,
                work_offset_hz: target_freq * working_offset_ppm / 1_000_000.0,
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

// генерация отчета
pub(crate) async fn handle_generate_report(
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

    let (freq_target, work_offset_hz, working_offset_ppm) = {
        let guard = freqmeter_config.lock().await;
        (
            guard.target_freq,
            guard.work_offset_hz,
            guard.working_offset_ppm,
        )
    };

    let limits = Limits::from_config(freq_target, &config, working_offset_ppm);

    let model = Model {
        part_id: part_id.clone(),
        date: {
            let system_time = SystemTime::now();
            let datetime: DateTime<Local> = system_time.into();
            datetime.format("%d.%m.%Y %T").to_string()
        },

        freq_target: format2digits(freq_target),
        ppm: format2digits(working_offset_ppm),
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
