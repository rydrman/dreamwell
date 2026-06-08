mod character_import;
mod config;
mod db;
mod error;
mod facts;
mod inference;
mod prompts;
mod queue;
mod routes;
mod story_prompts;
mod summarize;

use std::net::SocketAddr;

use axum::Router;
use dreamwell_types::HealthResponse;
use tower_http::{
    cors::CorsLayer,
    services::{ServeDir, ServeFile},
    trace::TraceLayer,
};
use tracing_subscriber::EnvFilter;

use crate::config::Config;
use crate::queue::JobQueue;
use crate::routes::AppState;

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::from_default_env().add_directive("dreamwell_server=info".parse().unwrap()),
        )
        .init();

    let config = Config::from_env();
    let pool = db::connect(&config.database_url)
        .await
        .expect("database connection");
    let queue = JobQueue::new(pool.clone());

    let state = AppState {
        pool,
        queue,
        sse_poll_interval_ms: config.sse_poll_interval_ms,
    };

    let api = Router::new()
        .route(
            "/health",
            axum::routing::get(|| async {
                axum::Json(HealthResponse {
                    status: "ok".to_string(),
                })
            }),
        )
        .nest("/characters", routes::characters::router())
        .nest("/chats", routes::chats::router())
        .nest("/stories", routes::stories::router())
        .nest("/settings", routes::settings::router())
        .with_state(state);

    let index = config.static_dir.join("index.html");
    let static_service = ServeDir::new(&config.static_dir).not_found_service(ServeFile::new(index));

    let app = Router::new()
        .nest("/api", api)
        .fallback_service(static_service)
        .layer(CorsLayer::permissive())
        .layer(TraceLayer::new_for_http());

    let addr: SocketAddr = format!("{}:{}", config.host, config.port)
        .parse()
        .expect("valid listen address");
    tracing::info!("listening on {addr}");
    let listener = tokio::net::TcpListener::bind(addr).await.unwrap();
    axum::serve(listener, app).await.unwrap();
}
