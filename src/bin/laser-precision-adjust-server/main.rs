//mod static_files;

use std::net::SocketAddr;

use axum::{
    extract::{FromRef, State},
    response::IntoResponse,
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
        .add_template("index", include_str!("www/html/index.html"))
        .unwrap();

    let app = Router::new()
        // Here we setup the routes. Note: No macros
        .route("/", get(handle_index))
        //.route("/static/:path/:file", get(static_files::handle_static))
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

async fn handle_index(State(engine): State<AppEngine>) -> impl IntoResponse {
    RenderHtml(Key("index".to_owned()), engine, ())
}
