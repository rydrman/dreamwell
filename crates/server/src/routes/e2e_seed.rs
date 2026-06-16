//! Dev/e2e-only helpers for deterministic tab-resume tests.

use axum::{
    extract::{Path, State},
    routing::post,
    Json, Router,
};
use dreamwell_types::{CharacterCreate, JobStatus, MessageRole, OkResponse};
use serde::Serialize;

use crate::db;
use crate::error::AppResult;
use crate::routes::AppState;

#[derive(Serialize)]
struct SeedChatRunningResponse {
    chat_id: i64,
    expected_content: String,
}

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/seed-chat-running", post(seed_chat_running))
        .route("/complete-chat-job/:chat_id", post(complete_chat_job))
}

async fn seed_chat_running(
    State(state): State<AppState>,
) -> AppResult<Json<SeedChatRunningResponse>> {
    let character = db::create_character(
        &state.pool,
        CharacterCreate {
            name: "E2E".into(),
            description: String::new(),
            personality: String::new(),
            scenario: String::new(),
            first_message: String::new(),
            example_dialogue: String::new(),
            system_prompt: String::new(),
            avatar_url: None,
        },
    )
    .await?;
    let chat = db::create_chat(&state.pool, "E2E chat".into(), character.id).await?;
    let message = db::insert_message(
        &state.pool,
        chat.id,
        MessageRole::Assistant,
        String::new(),
        false,
    )
    .await?;
    let job = db::enqueue_job(&state.pool, chat.id, message.id).await?;
    sqlx::query("UPDATE generation_jobs SET status = 'running' WHERE id = ?1")
        .bind(job.id)
        .execute(&state.pool)
        .await?;

    Ok(Json(SeedChatRunningResponse {
        chat_id: chat.id,
        expected_content: "Completed after background".into(),
    }))
}

async fn complete_chat_job(
    State(state): State<AppState>,
    Path(chat_id): Path<i64>,
) -> AppResult<Json<OkResponse>> {
    let active = db::get_active_job(&state.pool, chat_id).await?;
    let Some(job) = active else {
        return Ok(Json(OkResponse { ok: true }));
    };
    if let Some(message_id) = job.message_id {
        sqlx::query("UPDATE messages SET content = ?1 WHERE id = ?2")
            .bind("Completed after background")
            .bind(message_id)
            .execute(&state.pool)
            .await?;
    }
    db::complete_job(&state.pool, job.id, JobStatus::Completed, None).await?;
    Ok(Json(OkResponse { ok: true }))
}
