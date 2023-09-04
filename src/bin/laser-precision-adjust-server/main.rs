mod static_files;

use std::net::SocketAddr;

use axum::{
    extract::{FromRef, State},
    response::{IntoResponse, Redirect},
    routing::get,
    Router,
};
use serde::Serialize;
use tower::ServiceBuilder;
use tower_http::trace::TraceLayer;
use tracing_subscriber::prelude::*;

use axum_template::{engine::Engine, Key, RenderHtml};

use minijinja::Environment;

pub(crate) type AppEngine = Engine<Environment<'static>>;

#[derive(Clone, FromRef)]
struct AppState {
    engine: AppEngine,
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

    let app = Router::new()
        .route("/", get(|| async { Redirect::permanent("/work") }))
        .route("/work", get(handle_work))
        .route("/stat", get(handle_stat))
        .route("/config", get(handle_config))
        .route("/static/:path/:file", get(static_files::handle_static))
        .route("/lib/*path", get(static_files::handle_lib))
        .with_state(AppState {
            engine: Engine::from(minijinja),
        })
        // Using tower to add tracing layer
        .layer(ServiceBuilder::new().layer(TraceLayer::new_for_http()));

    // In practice: Use graceful shutdown.
    // Note that Axum has great examples for a log of practical scenarios,
    // including graceful shutdown (https://github.com/tokio-rs/axum/tree/main/examples)
    let addr = SocketAddr::from(([0, 0, 0, 0], 3000));
    println!("Listening on {}", addr);
    axum_server::bind(addr).serve(app.into_make_service()).await
}

#[derive(Debug, Serialize)]
pub struct Person {
    name: String,
}

async fn handle_work(State(engine): State<AppEngine>) -> impl IntoResponse {
    RenderHtml(Key("work".to_owned()), engine, ())
}

async fn handle_stat(State(engine): State<AppEngine>) -> impl IntoResponse {
    RenderHtml(Key("stat".to_owned()), engine, ())
}

async fn handle_config(State(engine): State<AppEngine>) -> impl IntoResponse {
    RenderHtml(Key("config".to_owned()), engine, ())
}
