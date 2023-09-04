mod handle_routes;
mod static_files;

use std::net::SocketAddr;

use axum::{extract::FromRef, response::Redirect, routing::get, Router};
use laser_precision_adjust::PrecisionAdjust;
use tower::ServiceBuilder;
use tower_http::trace::TraceLayer;
use tracing_subscriber::prelude::*;

use axum_template::engine::Engine;

use minijinja::Environment;

use crate::handle_routes::{handle_config, handle_stat, handle_work};

pub(crate) type AppEngine = Engine<Environment<'static>>;

#[derive(Clone, FromRef)]
struct AppState {
    engine: AppEngine,
    config: laser_precision_adjust::Config,
    config_file: std::path::PathBuf,
}

#[tokio::main]
async fn main() -> Result<(), std::io::Error> {
    // Enable tracing using Tokio's https://tokio.rs/#tk-lib-tracing
    tracing_subscriber::registry()
        .with(tracing_subscriber::EnvFilter::new(
            std::env::var("RUST_LOG")
                .unwrap_or_else(|_| "laser-precision-adjust-server=debug,tower_http=info".into()),
        ))
        .with(tracing_subscriber::fmt::layer())
        .init();

    log::info!("Loading config...");
    let (config, config_file) = laser_precision_adjust::Config::load();

    /*
    let mut precision_adjust = PrecisionAdjust::with_config(config.clone()).await;

    log::warn!("Testing connections...");
    if let Err(e) = precision_adjust.test_connection().await {
        panic!("Failed to connect to: {:?}", e);
    } else {
        log::info!("Connection successful!");
    }

    let _monitoring = precision_adjust.start_monitoring().await;
    precision_adjust.reset().await.expect("Can't reset laser!");
    */

    // State for our application
    let mut minijinja = Environment::new();
    minijinja
        .add_template("work", include_str!("wwwroot/html/work.html"))
        .unwrap();
    minijinja
        .add_template("stat", include_str!("wwwroot/html/stat.html"))
        .unwrap();
    minijinja
        .add_template("config", include_str!("wwwroot/html/config.html"))
        .unwrap();

    let app_state = AppState {
        engine: Engine::from(minijinja),
        config,
        config_file,
    };

    // Build our application with some routes
    let app = Router::new()
        .route("/", get(|| async { Redirect::permanent("/work") }))
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
    println!("Listening on {}", addr);
    axum_server::bind(addr).serve(app.into_make_service()).await
}
