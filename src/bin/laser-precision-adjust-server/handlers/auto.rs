use std::{io::Cursor, sync::Arc, time::SystemTime};

use axum::{
    extract::{Path, State},
    http::StatusCode,
    response::IntoResponse,
    Json,
};
use axum_template::{Key, RenderHtml};
use laser_precision_adjust::{AdjustConfig, Config};
use serde::Serialize;
use tokio::sync::Mutex;

use crate::{
    auto_adjust_all::AutoAdjustAllController,
    handlers::{common::format2digits, limits::Limits},
    AppEngine, ChannelState,
};

pub(crate) async fn handle_auto_adjust(
    State(engine): State<AppEngine>,
    State(config): State<Config>,
    State(freqmeter_config): State<Arc<Mutex<AdjustConfig>>>,
    State(channels): State<Arc<Mutex<Vec<ChannelState>>>>,
    State(auto_adjust_all_ctrl): State<Arc<Mutex<AutoAdjustAllController>>>,
) -> impl IntoResponse {
    #[derive(Serialize)]
    struct Model {
        target_freq: String,
        work_offset_hz: String,
        rezonators: Vec<u32>,
        stage: String,
        precision_hz: f32,
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
        precision_hz: config.target_freq_center * config.working_offset_ppm / 1_000_000.0,
    };

    RenderHtml(Key("auto".to_owned()), engine, model)
}

pub(crate) async fn handle_auto_adjust_status(
    State(auto_adjust_all_ctrl): State<Arc<Mutex<AutoAdjustAllController>>>,
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
        None => Json(crate::handlers::handle_control::ControlResult::error(
            "Autoadjust not active".to_owned(),
        ))
        .into_response(),
    }
}

// Генерация Excell отчета
pub(crate) async fn handle_generate_report_excel(
    State(auto_adjust_all_ctrl): State<Arc<Mutex<AutoAdjustAllController>>>,
    State(config): State<Config>,
    State(freqmeter_config): State<Arc<Mutex<AdjustConfig>>>,
    Path(part_id): Path<String>,
) -> impl IntoResponse {
    use super::into_body::IntoBody;

    const ROW_OFFSET: usize = 12;
    let report_template_xlsx = include_bytes!("report.xlsx");

    if let Ok(mut book) =
        umya_spreadsheet::reader::xlsx::read_reader(Cursor::new(report_template_xlsx), true)
    {
        let sheet = book.get_sheet_by_name_mut("report").unwrap();

        {
            // date
            use chrono::{DateTime, Local};

            let system_time = SystemTime::now();
            let datetime: DateTime<Local> = system_time.into();
            let date = datetime.format("%d.%m.%Y %T").to_string();
            let time = datetime.format("%T").to_string();
            sheet.get_cell_value_mut("F2").set_value(date);
            sheet.get_cell_value_mut("G2").set_value(time);
        }

        // Обозначение партии
        sheet.get_cell_value_mut("C4").set_value(part_id.clone());

        // Диопазон
        let (freq_target, work_offset_hz) = {
            let guard = freqmeter_config.lock().await;
            (guard.target_freq, guard.work_offset_hz)
        };

        sheet
            .get_cell_value_mut("C7")
            .set_value(format2digits(freq_target));

        // ppm
        sheet
            .get_cell_value_mut("E7")
            .set_value(format2digits(config.working_offset_ppm));

        // min-max
        let limits = Limits::from_config(freq_target, &config);
        sheet
            .get_cell_value_mut("G7")
            .set_value(format2digits(limits.lower_limit));
        sheet
            .get_cell_value_mut("H7")
            .set_value(format2digits(limits.upper_limit));

        // поправка частотомера
        sheet
            .get_cell_value_mut("C8")
            .set_value(format2digits(work_offset_hz));

        // таблица
        let report = auto_adjust_all_ctrl.lock().await.get_status();
        if !report.rezonator_info.is_empty() {
            for (i, r) in report.rezonator_info.iter().enumerate() {
                let row = ROW_OFFSET + i; // row in table

                let current_freq = r.current_freq;
                sheet
                    .get_cell_value_mut(format!("C{row}"))
                    .set_value(format2digits(current_freq));

                let start = format2digits(r.initial_freq);
                sheet.get_cell_value_mut(format!("B{row}")).set_value(start);

                let ppm = format2digits(limits.ppm(current_freq));
                sheet.get_cell_value_mut(format!("D{row}")).set_value(ppm);

                let ok = limits.to_status_icon(current_freq).to_owned();
                sheet.get_cell_value_mut(format!("E{row}")).set_value(ok);
            }
        } else {
            // clear table
            for row in ROW_OFFSET..ROW_OFFSET + config.resonator_placement.len() {
                for col in ['B', 'C', 'D', 'E'] {
                    sheet
                        .get_cell_value_mut(format!("{col}{row}"))
                        .set_value("-");
                }
            }
        }

        let mut buf = vec![];
        match umya_spreadsheet::writer::xlsx::write_writer(&book, Cursor::new(&mut buf)) {
            Ok(_) => {
                let filename = format!("attachment; filename=\"{}\".xlsx", part_id);
                let headers = [
                    (
                        axum::http::header::CONTENT_TYPE,
                        "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet", // https://developer.mozilla.org/en-US/docs/Web/HTTP/Basics_of_HTTP/MIME_types/Common_types
                    ),
                    (axum::http::header::CONTENT_DISPOSITION, filename.as_str()),
                ];
                (headers, buf.into_body()).into_response()
            }
            Err(e) => {
                let err = format!("Failed to generate report: {}", e);
                (StatusCode::INTERNAL_SERVER_ERROR, err).into_response()
            }
        }
    } else {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            "Failed to load report template",
        )
            .into_response()
    }
}
