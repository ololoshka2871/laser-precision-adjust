use std::sync::Arc;

use axum::{
    extract::{Path, State},
    response::IntoResponse,
    Json,
};
use axum_template::{Key, RenderHtml};
use laser_precision_adjust::{predict::Predictor, AdjustConfig, Config, IDataPoint};
use serde::Serialize;
use tokio::sync::Mutex;

use crate::{auto_adjust_all::AutoAdjustAllController, AppEngine, ChannelState, DataPoint};

use super::{
    common::format2digits,
    limits::{Limits, RezStatus},
};

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
    script: String,
}

pub(crate) async fn handle_stat_manual(
    State(channels): State<Arc<Mutex<Vec<ChannelState>>>>,
    State(config): State<Config>,
    State(freqmeter_config): State<Arc<Mutex<AdjustConfig>>>,
    State(engine): State<AppEngine>,
) -> impl IntoResponse {
    let (target_freq, working_offset_ppm) = {
        let guard = freqmeter_config.lock().await;
        (guard.target_freq, guard.working_offset_ppm)
    };

    let limits = Limits::from_config(target_freq, &config, working_offset_ppm);

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
            script: "stat_manual".to_owned(),
        },
    )
}

pub(crate) async fn handle_stat_auto(
    State(auto_adjust_all_ctrl): State<Arc<Mutex<AutoAdjustAllController>>>,
    State(config): State<Config>,
    State(freqmeter_config): State<Arc<Mutex<AdjustConfig>>>,
    State(engine): State<AppEngine>,
) -> impl IntoResponse {
    let (target_freq, working_offset_ppm) = {
        let guard = freqmeter_config.lock().await;
        (guard.target_freq, guard.working_offset_ppm)
    };
    let limits = Limits::from_config(target_freq, &config, working_offset_ppm);

    RenderHtml(
        Key("stat".to_owned()),
        engine,
        Model {
            rezonators: {
                let status = auto_adjust_all_ctrl.lock().await.get_status();
                status
                    .rezonator_info
                    .iter()
                    .map(|r| RezData {
                        current_step: r.current_step,
                        initial_freq: format2digits(r.initial_freq),
                        current_freq: format2digits(r.current_freq),
                        status: limits.to_status(r.current_freq),
                        ppm: format2digits(limits.ppm(r.current_freq)),
                    })
                    .collect()
            },
            script: "stat_auto".to_owned(),
        },
    )
}

pub(crate) async fn handle_stat_rez_manual(
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
        let (target_freq, working_offset_ppm) = {
            let guard = freqmeter_config.lock().await;
            (guard.target_freq, guard.working_offset_ppm)
        };
        DrawLimits::new(
            Limits::from_config(target_freq, &config, working_offset_ppm),
            target_freq,
        )
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

pub(crate) async fn handle_stat_rez_auto(
    State(auto_adjust_all_ctrl): State<Arc<Mutex<AutoAdjustAllController>>>,
    State(config): State<Config>,
    State(freqmeter_config): State<Arc<Mutex<AdjustConfig>>>,
    Path(rez_id): Path<u32>,
) -> impl IntoResponse {
    use itertools::Itertools;
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
        let (target_freq, working_offset_ppm) = {
            let guard = freqmeter_config.lock().await;
            (guard.target_freq, guard.working_offset_ppm)
        };
        DrawLimits::new(
            Limits::from_config(target_freq, &config, working_offset_ppm),
            target_freq,
        )
    };

    let fragments = auto_adjust_all_ctrl
        .lock()
        .await
        .get_status()
        .rezonator_info;

    let res_history = &fragments[rez_id as usize].history;

    let boxes = res_history.iter().map(|h| h.boxplt).collect::<Vec<_>>();

    let mut diffs = res_history
        .iter()
        .tuple_windows()
        .filter_map(|(a, b)| {
            if let Some(steps) = a.burn {
                Some(vec![
                    (b.boxplt.median() - a.boxplt.median()) / steps as f32;
                    steps as usize
                ])
            } else {
                None
            }
        })
        .flatten()
        .collect::<Vec<_>>();

    let interval_count = ((diffs.len() as f32).log(10.0) * 3.0 + 1.0).floor() as u32;
    let hystogramm = if interval_count > 1 {
        diffs.sort_by(|a, b| a.partial_cmp(b).unwrap());
        let start = diffs.first().unwrap_or(&0.0);
        let step = diffs
            .last()
            .map(|v| *v - start)
            .map(|v| v / interval_count as f32)
            .unwrap();

        (0..interval_count)
            .map(|interval_n| {
                let start = start + interval_n as f32 * step;
                let end = start + step;

                let count = diffs.iter().filter(|v| **v >= start && **v <= end).count();
                HystogramFragment {
                    start: start as f64,
                    end: end as f64,
                    count,
                }
            })
            .collect::<Vec<_>>()
    } else {
        vec![]
    };

    Json(json!({
            "DisplayBoxes": boxes,
            "Hystogramm": hystogramm,
            "Limits": limits,
    }))
}
