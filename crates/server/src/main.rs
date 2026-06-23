mod build_info;
mod character_import;
mod chat_prompts;
mod chat_state;
mod config;
mod db;
mod error;
mod game_mechanics;
mod game_prompts;
mod game_prose_recheck;
mod game_resolution;
mod game_state;
mod game_state_recheck;
mod game_summarize;
mod game_tools;
mod game_turn;
mod game_turn_agent;
mod inference;
mod message_followups;
mod prompts;
mod queue;
mod routes;
mod scenario_db;
mod scenario_import;
mod state_recheck;
mod story_beat_mechanical;
mod story_beat_prose_recheck;
mod story_prompts;
mod story_state;
mod story_summarize;
mod story_variable_recheck;
mod story_variables;
mod summarize;
mod thoughts;
mod tool_stream;
mod variable_recheck;
mod variable_state;
mod variables;

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
    let requeued = db::requeue_stale_jobs(&pool)
        .await
        .expect("requeue stale jobs");
    if requeued > 0 {
        tracing::info!("requeued {requeued} stale jobs after restart");
    }
    let pending = db::count_queued_jobs(&pool)
        .await
        .expect("count queued jobs");
    let queue = JobQueue::new(pool.clone());
    if requeued > 0 || pending > 0 {
        tracing::info!("waking generation queue ({pending} queued job(s))");
        queue.wake();
    }

    let shutdown_pool = pool.clone();
    let state = AppState {
        pool,
        queue,
        sse_poll_interval_ms: config.sse_poll_interval_ms,
    };

    let mut api = Router::new()
        .route(
            "/health",
            axum::routing::get(|| async {
                axum::Json(HealthResponse {
                    status: "ok".to_string(),
                    git_sha: Some(build_info::GIT_SHA.to_string()),
                })
            }),
        )
        .nest("/characters", routes::characters::router())
        .nest("/chats", routes::chats::router())
        .nest("/jobs", routes::jobs::router())
        .nest("/stories", routes::stories::router())
        .nest("/games", routes::games::router())
        .nest("/scenarios", routes::scenarios::router())
        .nest("/settings", routes::settings::router());

    if std::env::var("DREAMWELL_E2E").is_ok_and(|v| v == "1") {
        api = api.nest("/e2e", routes::e2e_seed::router());
    }

    let api = api.with_state(state);

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

    let shutdown = async {
        let ctrl_c = async {
            tokio::signal::ctrl_c()
                .await
                .expect("failed to install Ctrl+C handler");
        };

        #[cfg(unix)]
        let terminate = async {
            match tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate()) {
                Ok(mut stream) => stream.recv().await,
                Err(err) => {
                    tracing::warn!(%err, "failed to install SIGTERM handler");
                    std::future::pending::<Option<()>>().await
                }
            }
        };

        #[cfg(not(unix))]
        let terminate = std::future::pending::<Option<()>>();

        tokio::select! {
            () = ctrl_c => {},
            _ = terminate => {},
        }
        tracing::info!("shutdown signal received");
    };

    axum::serve(listener, app)
        .with_graceful_shutdown(shutdown)
        .await
        .unwrap();
    shutdown_pool.close().await;
}
