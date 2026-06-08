use axum::{
    extract::{Path, State},
    routing::{get, post},
    Json, Router,
};
use dreamwell_types::{Job, QueueStatus};

use crate::db;
use crate::error::AppResult;
use crate::routes::AppState;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/", get(list_jobs))
        .route("/:id/cancel", post(cancel_job))
}

async fn list_jobs(State(state): State<AppState>) -> AppResult<Json<QueueStatus>> {
    let (running, queued) = db::list_queue(&state.pool).await?;
    Ok(Json(QueueStatus { running, queued }))
}

async fn cancel_job(State(state): State<AppState>, Path(id): Path<i64>) -> AppResult<Json<Job>> {
    Ok(Json(state.queue.cancel_job(&state.pool, id).await?))
}
