#![feature(async_iterator)]

mod auto_adjust_all;
mod auto_adjust_single_controller;
mod far_long_iterator;
mod handle_routes;
mod into_body;
mod static_files;

use std::{net::SocketAddr, sync::Arc};

use axum::{
    extract::FromRef,
    response::Redirect,
    routing::{get, post},
    Router,
};
use laser_precision_adjust::{predict::Predictor, AdjustConfig, DataPoint, PrecisionAdjust2};

use tokio::sync::Mutex;
use tower::ServiceBuilder;
use tower_http::trace::TraceLayer;
use tracing_subscriber::prelude::*;

use axum_template::engine::Engine;

use minijinja::Environment;

use crate::handle_routes::{
    handle_auto_adjust, handle_auto_adjust_status, handle_config, handle_control,
    handle_generate_report, handle_stat_manual, handle_stat_rez_manual, handle_state, handle_update_config,
    handle_work, handle_generate_report_excel,
};

pub(crate) type AppEngine = Engine<Environment<'static>>;

#[derive(Clone)]
struct ChannelState {
    current_step: u32,
    initial_freq: Option<f32>,

    points: Vec<DataPoint<f64>>,
}

#[derive(Clone, FromRef)]
struct AppState {
    engine: AppEngine,
    config: laser_precision_adjust::Config,
    config_file: std::path::PathBuf,

    freqmeter_config: Arc<Mutex<AdjustConfig>>,
    status_rx: tokio::sync::watch::Receiver<laser_precision_adjust::Status>,

    precision_adjust: Arc<Mutex<PrecisionAdjust2>>,
    channels: Arc<Mutex<Vec<ChannelState>>>,
    close_timestamp: Arc<Mutex<Option<u128>>>,
    select_channel_blocked: Arc<Mutex<bool>>,

    predictor: Arc<Mutex<Predictor<f64>>>,
    auto_adjust_ctrl: Arc<Mutex<auto_adjust_single_controller::AutoAdjustSingleController>>,
    auto_adjust_all_ctrl: Arc<Mutex<auto_adjust_all::AutoAdjustAllController>>,
}

#[tokio::main]
async fn main() -> Result<(), std::io::Error> {
    // Enable tracing using Tokio's https://tokio.rs/#tk-lib-tracing
    tracing_subscriber::registry()
        .with(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "laser_precision_adjust_server=debug,tower_http=info".into()),
        )
        .with(tracing_subscriber::fmt::layer().with_target(false))
        .init();

    let emulate_freq = std::env::var("EMULATE_FREQ")
        .map(|v| v.parse::<f32>().unwrap_or_default())
        .ok();
    if let Some(f) = &emulate_freq {
        tracing::warn!("Emulating frequency: {}", f);
    }

    tracing::info!("Loading config...");
    let (config, config_file) = laser_precision_adjust::Config::load();

    let laser_controller = Arc::new(Mutex::new(laser_precision_adjust::LaserController::new(
        config.laser_control_port.clone(),
        std::time::Duration::from_millis(config.port_timeout_ms),
        config.resonator_placement.clone(),
        config.axis_config.clone(),
        config.total_vertical_steps,
        config.burn_laser_pump_power,
        config.burn_laser_power,
        config.burn_laser_frequency,
        config.burn_laser_feedrate,
        config.soft_mode_s_multiplier,
    )));

    let laser_setup_controller = Arc::new(Mutex::new(
        laser_precision_adjust::LaserSetupController::new(
            config.laser_setup_port.clone(),
            config.resonator_placement.len() as u32,
            std::time::Duration::from_millis(config.port_timeout_ms),
            config.freq_meter_i2c_addr,
            std::time::Duration::from_millis(config.update_interval_ms as u64),
            config.freqmeter_offset,
            config.i2c_commands.clone(),
            emulate_freq,
        ),
    ));

    let mut precision_adjust = PrecisionAdjust2::new(
        laser_setup_controller.clone(),
        laser_controller.clone(),
        config.switch_channel_delay_ms,
    )
    .await;
    tracing::warn!("Testing connections...");
    if let Err(e) = precision_adjust.test_connection().await {
        panic!("Failed to connect to: {:?}", e);
    } else {
        tracing::info!("Connection successful!");
    }

    let status_rx = precision_adjust.subscribe_status();

    let precision_adjust = Arc::new(Mutex::new(precision_adjust));

    let freqmeter_config = Arc::new(Mutex::new(AdjustConfig {
        target_freq: config.target_freq_center,
        work_offset_hz: config.freqmeter_offset,
    }));

    let predictor = Predictor::new(
        status_rx.clone(),
        config.forecast_config,
        config.resonator_placement.len(),
        (config.cooldown_time_ms / config.update_interval_ms) as usize,
    );

    let auto_adjust_controller = auto_adjust_single_controller::AutoAdjustSingleController::new(
        config.auto_adjust_limits,
        config.update_interval_ms,
        config.working_offset_ppm,
    );

    let auto_adjust_all_controller = auto_adjust_all::AutoAdjustAllController::new(
        config.resonator_placement.len(),
        laser_controller,
        laser_setup_controller,
        precision_adjust.clone(),
        config.auto_adjust_limits,
        std::time::Duration::from_millis(config.update_interval_ms as u64),
        config.working_offset_ppm,
        config.forecast_config,
        config.auto_adjust_limits.fast_forward_step_limit,
        config.switch_channel_delay_ms,
    );

    // State for our application
    let mut minijinja = Environment::new();
    minijinja
        .add_template("work", include_str!("wwwroot/html/work.jinja"))
        .unwrap();
    minijinja
        .add_template("auto", include_str!("wwwroot/html/auto.jinja"))
        .unwrap();
    minijinja
        .add_template("stat_manual", include_str!("wwwroot/html/stat_manual.jinja"))
        .unwrap();
    minijinja
        .add_template("config", include_str!("wwwroot/html/config.jinja"))
        .unwrap();
    minijinja
        .add_template("report.html", include_str!("wwwroot/html/report.jinja"))
        .unwrap();

    let app_state = AppState {
        channels: Arc::new(Mutex::new(vec![
            ChannelState {
                current_step: 0,
                initial_freq: None,
                points: vec![],
            };
            config.resonator_placement.len()
        ])),
        freqmeter_config: freqmeter_config,
        engine: Engine::from(minijinja),
        config,
        config_file,
        status_rx,
        precision_adjust: precision_adjust,
        close_timestamp: Arc::new(Mutex::new(None)),
        select_channel_blocked: Arc::new(Mutex::new(false)),

        predictor: Arc::new(Mutex::new(predictor)),
        auto_adjust_ctrl: Arc::new(Mutex::new(auto_adjust_controller)),
        auto_adjust_all_ctrl: Arc::new(Mutex::new(auto_adjust_all_controller)),
    };

    // Build our application with some routes
    let app = Router::new()
        .route("/", get(|| async { Redirect::permanent("/work") }))
        .route("/control/:action", post(handle_control))
        .route("/state", get(handle_state))
        .route("/work", get(handle_work))
        .route("/auto", get(handle_auto_adjust))
        .route("/auto_status", get(handle_auto_adjust_status))
        .route("/stat_manual", get(handle_stat_manual))
        .route("/stat_manual/:rez_id", get(handle_stat_rez_manual))
        .route("/report/:part_id", get(handle_generate_report))
        .route("/report2/:part_id", get(handle_generate_report_excel))
        .route("/config", get(handle_config).patch(handle_update_config))
        .route("/static/:path/:file", get(static_files::handle_static))
        .route("/lib/*path", get(static_files::handle_lib))
        .with_state(app_state)
        // Using tower to add tracing layer
        .layer(ServiceBuilder::new().layer(TraceLayer::new_for_http()));

    // In practice: Use graceful shutdown.
    // Note that Axum has great examples for a log of practical scenarios,
    // including graceful shutdown (https://github.com/tokio-rs/axum/tree/main/examples)
    let addr = SocketAddr::from(([0, 0, 0, 0], 3000));
    tracing::info!("Listening on {}", addr);
    axum_server::bind(addr).serve(app.into_make_service()).await
}
