use std::net::SocketAddr;

use axum::{
    extract::{FromRef, Path},
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

type AppEngine = Engine<Environment<'static>>;

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
                .unwrap_or_else(|_| "laser-precision-adjust-server=debug,tower_http=debug".into()),
        ))
        .with(tracing_subscriber::fmt::layer())
        .init();

    // State for our application
    let mut jinja = Environment::new();
    jinja
        .add_template("/:name", "<h1>Hello Minijinja!</h1><p>{{name}}</p>")
        .unwrap();

    let app = Router::new()
        // Here we setup the routes. Note: No macros
        .route("/", get(index))
        .with_state(AppState {
            engine: Engine::from(jinja),
        })
        // Using tower to add tracing layer
        .layer(ServiceBuilder::new().layer(TraceLayer::new_for_http()));

    // In practice: Use graceful shutdown.
    // Note that Axum has great examples for a log of practical scenarios,
    // including graceful shutdown (https://github.com/tokio-rs/axum/tree/main/examples)
    let addr = SocketAddr::from(([0, 0, 0, 0], 3000));
    println!("listening on {}", addr);
    axum_server::bind(addr)
        .serve(app.into_make_service())
        .await
        .unwrap();
    Ok(())
}

#[derive(Debug, Serialize)]
pub struct Person {
    name: String,
}

async fn index(_engine: AppEngine, Key(_key): Key, Path(_name): Path<String>) -> impl IntoResponse {
    /*
    let person = Person { name };

    RenderHtml(Key("index.html".to_owned()), engine, person)
    */

    "Hello World".into_response()
}
