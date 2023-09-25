mod auto_adjust_controller;
mod handle_routes;
mod predict;
mod static_files;

use std::{net::SocketAddr, sync::Arc};

use axum::{
    extract::FromRef,
    response::Redirect,
    routing::{get, post},
    Router,
};
use laser_precision_adjust::PrecisionAdjust;

use tokio::sync::Mutex;
use tower::ServiceBuilder;
use tower_http::trace::TraceLayer;
use tracing_subscriber::prelude::*;

use axum_template::engine::Engine;

use minijinja::Environment;

use crate::handle_routes::{
    handle_config, handle_control, handle_stat, handle_state, handle_update_config, handle_work,
};

pub(crate) type AppEngine = Engine<Environment<'static>>;

pub trait IDataPoint<T> {
    fn x(&self) -> T;
    fn y(&self) -> T;
}

#[derive(Clone, Copy)]
pub struct DataPoint<T> {
    x: T,
    y: T,
}

impl<T: num_traits::Float> DataPoint<T> {
    pub fn new(x: T, y: T) -> Self {
        Self { x, y }
    }

    pub fn nan() -> Self {
        Self {
            x: T::nan(),
            y: T::nan(),
        }
    }
}

impl<T: num_traits::Float> IDataPoint<T> for DataPoint<T> {
    fn x(&self) -> T {
        self.x
    }

    fn y(&self) -> T {
        self.y
    }
}

#[derive(Clone)]
struct ChannelState {
    current_step: u32,
    initial_freq: Option<f32>,
    current_freq: f32,

    points: Vec<DataPoint<f64>>,
}

#[derive(Clone)]
pub struct AdjustConfig {
    target_freq: f32,
    work_offset_hz: f32,
}

#[derive(Clone, FromRef)]
struct AppState {
    engine: AppEngine,
    config: laser_precision_adjust::Config,
    config_file: std::path::PathBuf,

    freqmeter_config: Arc<Mutex<AdjustConfig>>,
    status_rx: tokio::sync::watch::Receiver<laser_precision_adjust::Status>,

    precision_adjust: Arc<Mutex<PrecisionAdjust>>,
    channels: Arc<Mutex<Vec<ChannelState>>>,
    close_timestamp: Arc<Mutex<Option<u128>>>,
    select_channel_blocked: Arc<Mutex<bool>>,

    predictor: Arc<Mutex<predict::Predictor<f64>>>,
    auto_adjust_ctrl: Arc<Mutex<auto_adjust_controller::AutoAdjestController>>,
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

    let mut precision_adjust = PrecisionAdjust::with_config(config.clone()).await;

    tracing::warn!("Testing connections...");
    if let Err(e) = precision_adjust.test_connection().await {
        panic!("Failed to connect to: {:?}", e);
    } else {
        tracing::info!("Connection successful!");
    }

    let status_rx = precision_adjust.start_monitoring(emulate_freq).await;
    precision_adjust.reset().await.expect("Can't reset laser!");

    let freqmeter_config = Arc::new(Mutex::new(AdjustConfig {
        target_freq: config.target_freq_center,
        work_offset_hz: config.freqmeter_offset,
    }));

    let predictor = predict::Predictor::new(
        status_rx.clone(),
        config.forecast_config,
        config.resonator_placement.len(),
        (config.cooldown_time_ms / config.update_interval_ms) as usize,
        freqmeter_config.clone(),
    );

    let auto_adjust_controller = auto_adjust_controller::AutoAdjestController::new(
        config.auto_adjust_limits,
        config.update_interval_ms,
        config.working_offset_ppm,
    );

    // State for our application
    let mut minijinja = Environment::new();
    minijinja
        .add_template("work", include_str!("wwwroot/html/work.jinja"))
        .unwrap();
    minijinja
        .add_template("stat", include_str!("wwwroot/html/stat.jinja"))
        .unwrap();
    minijinja
        .add_template("config", include_str!("wwwroot/html/config.jinja"))
        .unwrap();

    let app_state = AppState {
        channels: Arc::new(Mutex::new(vec![
            ChannelState {
                current_step: 0,
                initial_freq: None,
                current_freq: 0.0,
                points: vec![],
            };
            config.resonator_placement.len()
        ])),
        freqmeter_config: freqmeter_config,
        engine: Engine::from(minijinja),
        config,
        config_file,
        status_rx,
        precision_adjust: Arc::new(Mutex::new(precision_adjust)),
        close_timestamp: Arc::new(Mutex::new(None)),
        select_channel_blocked: Arc::new(Mutex::new(false)),

        predictor: Arc::new(Mutex::new(predictor)),
        auto_adjust_ctrl: Arc::new(Mutex::new(auto_adjust_controller)),
    };

    // Build our application with some routes
    let app = Router::new()
        .route("/", get(|| async { Redirect::permanent("/work") }))
        .route("/control/:action", post(handle_control))
        .route("/state", get(handle_state))
        .route("/work", get(handle_work))
        .route("/stat", get(handle_stat))
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
