mod handle_routes;
mod static_files;

use std::{net::SocketAddr, sync::Arc};

use axum::{
    extract::FromRef,
    response::Redirect,
    routing::{get, post},
    Router,
};
use laser_precision_adjust::PrecisionAdjust;
use serde::Serialize;
use tokio::sync::Mutex;
use tower::ServiceBuilder;
use tower_http::trace::TraceLayer;
use tracing_subscriber::prelude::*;

use axum_template::engine::Engine;

use minijinja::Environment;

use crate::handle_routes::{handle_config, handle_control, handle_stat, handle_state, handle_work};

pub(crate) type AppEngine = Engine<Environment<'static>>;

#[derive(Clone, Serialize)]
struct ChannelState {
    current_step: u32,
    initial_freq: f32,
    current_freq: f32,

    points: Vec<(u128, f32)>,
}

#[derive(Clone, FromRef)]
struct AppState {
    engine: AppEngine,
    config: laser_precision_adjust::Config,
    config_file: std::path::PathBuf,

    adjust_target: Arc<Mutex<f32>>,
    status_rx: tokio::sync::watch::Receiver<laser_precision_adjust::Status>,

    precision_adjust: Arc<Mutex<PrecisionAdjust>>,
    channels: Arc<Mutex<Vec<ChannelState>>>,
    close_timestamp: Arc<Mutex<Option<u128>>>,
    select_channel_blocked: Arc<Mutex<bool>>,
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
                initial_freq: 0.0,
                current_freq: 0.0,
                points: vec![],
            };
            config.resonator_placement.len()
        ])),
        adjust_target: Arc::new(Mutex::new(config.target_freq_center)),
        engine: Engine::from(minijinja),
        config,
        config_file,
        status_rx,
        precision_adjust: Arc::new(Mutex::new(precision_adjust)),
        close_timestamp: Arc::new(Mutex::new(None)),
        select_channel_blocked: Arc::new(Mutex::new(false)),
    };

    // Build our application with some routes
    let app = Router::new()
        .route("/", get(|| async { Redirect::permanent("/work") }))
        .route("/control/:action", post(handle_control))
        .route("/state", get(handle_state))
        .route("/work", get(handle_work))
        .route("/stat", get(handle_stat))
        .route("/config", get(handle_config))
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
