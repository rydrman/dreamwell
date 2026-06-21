use dreamwell_types::{Settings, StateChangeRequest};
use serde_json::json;
use sqlx::SqlitePool;

use crate::chat_state::{apply_state_changes, build_state_block};
use crate::config;
use crate::db;
use crate::error::{AppError, AppResult};
use crate::inference::chat_completion_json;
use crate::story_state::{
    apply_state_changes as apply_story_state_changes, build_state_block as build_story_state_block,
};
use dreamwell_state::{state_recheck_schema, RECHECK_SYSTEM_PROMPT, STATE_TARGET_RULES};

#[derive(Debug, Clone, serde::Deserialize)]
struct StateRecheckResponse {
    #[serde(default)]
    state_changes: Vec<StateChangeRequest>,
}

fn recheck_output_tokens(settings: &Settings) -> i64 {
    if settings.context_tokens > 0 {
        (settings.context_tokens / 8).clamp(256, 768)
    } else {
        512
    }
}

fn build_recheck_prompt(prose: &str, state_block: &str) -> Vec<serde_json::Value> {
    vec![
        json!({
            "role": "system",
            "content": format!("{RECHECK_SYSTEM_PROMPT}\n\n{STATE_TARGET_RULES}"),
        }),
        json!({
            "role": "user",
            "content": format!("Current typed state:\n{state_block}\n\nProse to review:\n{prose}"),
        }),
    ]
}

pub async fn run_chat_state_recheck_job(
    pool: &SqlitePool,
    job_id: i64,
    chat_id: i64,
    message_id: i64,
    settings: &Settings,
) -> AppResult<()> {
    if !settings.variables_enabled {
        return Err(AppError::bad_request("Chat state is disabled in settings"));
    }
    let message = db::get_message(pool, chat_id, message_id).await?;
    let actors = db::list_chat_actors(pool, chat_id).await?;
    let state = db::list_chat_state_entries(pool, chat_id).await?;
    let state_block = build_state_block(&state, &actors);
    let messages = build_recheck_prompt(&message.content, &state_block);

    let response: StateRecheckResponse = chat_completion_json(
        &settings.inference_url,
        &settings.model,
        &messages,
        0.2,
        settings.top_p,
        recheck_output_tokens(settings),
        Some(&state_recheck_schema()),
        config::GENERATION_MAX_RETRIES
            .load(std::sync::atomic::Ordering::SeqCst)
            .max(1),
        &tokio_util::sync::CancellationToken::new(),
    )
    .await?;
    if !response.state_changes.is_empty() {
        let current = db::list_chat_state_entries(pool, chat_id).await?;
        let applied = apply_state_changes(
            pool,
            chat_id,
            message_id,
            &response.state_changes,
            &actors,
            &current,
        )
        .await?;
        let mut merged = message.state_changes.clone();
        merged.extend(applied);
        db::save_message_plan(pool, message_id, &message.reply_beats, &merged).await?;
    }
    db::complete_job(pool, job_id, dreamwell_types::JobStatus::Completed, None).await?;
    Ok(())
}

pub async fn run_story_state_recheck_job(
    pool: &SqlitePool,
    job_id: i64,
    story_id: i64,
    beat_id: i64,
    settings: &Settings,
) -> AppResult<()> {
    if !settings.variables_enabled {
        return Err(AppError::bad_request("Story state is disabled in settings"));
    }
    let beat = db::get_beat_by_id(pool, story_id, beat_id).await?;
    let actors = db::list_story_actors(pool, story_id).await?;
    let state = db::list_story_state_entries(pool, story_id).await?;
    let state_block = build_story_state_block(&state, &actors);
    let messages = build_recheck_prompt(&beat.content, &state_block);

    let response: StateRecheckResponse = chat_completion_json(
        &settings.inference_url,
        &settings.model,
        &messages,
        0.2,
        settings.top_p,
        recheck_output_tokens(settings),
        Some(&state_recheck_schema()),
        config::GENERATION_MAX_RETRIES
            .load(std::sync::atomic::Ordering::SeqCst)
            .max(1),
        &tokio_util::sync::CancellationToken::new(),
    )
    .await?;
    if !response.state_changes.is_empty() {
        let current = db::list_story_state_entries(pool, story_id).await?;
        let applied = apply_story_state_changes(
            pool,
            story_id,
            beat_id,
            &response.state_changes,
            &actors,
            &current,
        )
        .await?;
        let mut merged = beat.state_changes.clone();
        merged.extend(applied);
        db::save_beat_plan(pool, beat_id, &beat.plan_beats, &merged).await?;
    }
    db::complete_job(pool, job_id, dreamwell_types::JobStatus::Completed, None).await?;
    Ok(())
}
