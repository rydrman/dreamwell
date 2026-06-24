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

#[derive(Serialize)]
struct SeedGameRunningResponse {
    game_id: i64,
    expected_content: String,
}

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/seed-chat-running", post(seed_chat_running))
        .route("/complete-chat-job/:chat_id", post(complete_chat_job))
        .route("/seed-game-running", post(seed_game_running))
        .route("/complete-game-job/:game_id", post(complete_game_job))
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

async fn seed_game_running(
    State(state): State<AppState>,
) -> AppResult<Json<SeedGameRunningResponse>> {
    let detail = db::create_game(
        &state.pool,
        dreamwell_types::GameCreate {
            title: "E2E Game".into(),
            ..Default::default()
        },
    )
    .await?;
    let game_id = detail.game.id;
    let turn_id = sqlx::query_scalar::<_, i64>(
        "INSERT INTO game_turns (game_id, sort_order, player_action, phase, prose, created_at, updated_at) VALUES (?1, 0, 'I pick the lock.', 'prose', '', ?2, ?2) RETURNING id",
    )
    .bind(game_id)
    .bind(chrono::Utc::now().to_rfc3339())
    .fetch_one(&state.pool)
    .await?;
    let job = db::enqueue_game_job(
        &state.pool,
        dreamwell_types::JobType::GameTurnStructuredAgent,
        game_id,
        Some(turn_id),
        String::new(),
    )
    .await?;
    sqlx::query("UPDATE generation_jobs SET status = 'running' WHERE id = ?1")
        .bind(job.id)
        .execute(&state.pool)
        .await?;

    Ok(Json(SeedGameRunningResponse {
        game_id,
        expected_content: "The lock clicks open under your touch.".into(),
    }))
}

async fn complete_game_job(
    State(state): State<AppState>,
    Path(game_id): Path<i64>,
) -> AppResult<Json<OkResponse>> {
    let active = db::get_active_game_job(&state.pool, game_id).await?;
    let Some(job) = active else {
        return Ok(Json(OkResponse { ok: true }));
    };
    if let Some(turn_id) = job.turn_id {
        sqlx::query(
            "UPDATE game_turns SET prose = ?1, phase = 'done', updated_at = ?2 WHERE id = ?3",
        )
        .bind("The lock clicks open under your touch.")
        .bind(chrono::Utc::now().to_rfc3339())
        .bind(turn_id)
        .execute(&state.pool)
        .await?;
    }
    db::complete_job(&state.pool, job.id, JobStatus::Completed, None).await?;
    db::touch_game(&state.pool, game_id).await?;
    Ok(Json(OkResponse { ok: true }))
}
