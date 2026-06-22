use dreamwell_state::{state_recheck_schema, RECHECK_SYSTEM_PROMPT, STATE_TARGET_RULES};
use dreamwell_types::{Job, Settings, StateChangeRequest};
use serde_json::json;
use sqlx::SqlitePool;
use tokio::sync::mpsc;

use crate::config;
use crate::db;
use crate::error::{AppError, AppResult};
use crate::game_prompts::build_characters_block;
use crate::game_state::{apply_state_changes, build_state_block};

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

fn max_retries() -> u32 {
    config::GENERATION_MAX_RETRIES
        .load(std::sync::atomic::Ordering::SeqCst)
        .max(1)
}

fn build_recheck_prompt(
    prose: &str,
    state_block: &str,
    characters_block: &str,
    guidance: &str,
) -> Vec<serde_json::Value> {
    let mut user = format!("Current typed state:\n{state_block}\n\nTurn prose to review:\n{prose}");
    if !characters_block.is_empty() {
        user.push_str(&format!("\n\n{characters_block}"));
    }
    if !guidance.trim().is_empty() {
        user.push_str("\n\nGuidance from the player:\n");
        user.push_str(guidance.trim());
    }
    vec![
        json!({
            "role": "system",
            "content": format!("{RECHECK_SYSTEM_PROMPT}\n\n{STATE_TARGET_RULES}"),
        }),
        json!({
            "role": "user",
            "content": user,
        }),
    ]
}

pub async fn enqueue_turn_state_recheck(
    pool: &SqlitePool,
    work_tx: &mpsc::UnboundedSender<()>,
    game_id: i64,
    turn_id: i64,
    guidance_notes: &str,
    settings: &Settings,
) -> AppResult<Job> {
    if settings.model.is_empty() && db::get_game(pool, game_id).await?.model_resolve.is_empty() {
        return Err(AppError::bad_request(
            "Configure an inference model in Settings before rechecking state",
        ));
    }
    let turn = db::get_turn(pool, game_id, turn_id).await?;
    if turn.prose.trim().is_empty() {
        return Err(AppError::bad_request("Turn has no prose to recheck"));
    }
    if db::has_active_turn_job(pool, turn_id).await? {
        return Err(AppError::bad_request(
            "Wait for the current turn job to finish before rechecking state",
        ));
    }

    let job = db::enqueue_game_job(
        pool,
        dreamwell_types::JobType::GameStateRecheck,
        game_id,
        Some(turn_id),
        guidance_notes.to_string(),
    )
    .await?;
    let _ = work_tx.send(());
    Ok(job)
}

pub async fn run_turn_state_recheck_job(
    pool: &SqlitePool,
    job_id: i64,
    game_id: i64,
    turn_id: i64,
    guidance: &str,
    settings: &Settings,
) -> AppResult<()> {
    let inference = db::get_inference_config(pool).await?;
    let game = db::get_game(pool, game_id).await?;
    let turn = db::get_turn(pool, game_id, turn_id).await?;
    if turn.prose.trim().is_empty() {
        db::complete_job(pool, job_id, dreamwell_types::JobStatus::Completed, None).await?;
        return Ok(());
    }

    let detail = db::get_game_detail(pool, game_id).await?;
    let state_block = build_state_block(&detail.state, &detail.actors);
    let characters_block = build_characters_block(&detail.actors);
    let prompt = build_recheck_prompt(&turn.prose, &state_block, &characters_block, guidance);
    let model = crate::game_turn::model_for_phase(
        &game,
        settings,
        crate::game_turn::GameModelPhase::Resolve,
    );
    let token = tokio_util::sync::CancellationToken::new();

    let response: StateRecheckResponse = db::chat_completion_json_for_connection(
        pool,
        &inference,
        &model,
        &prompt,
        0.2,
        settings.top_p,
        recheck_output_tokens(settings),
        Some(&state_recheck_schema()),
        max_retries(),
        &token,
    )
    .await?;

    if response.state_changes.is_empty() {
        db::complete_job(pool, job_id, dreamwell_types::JobStatus::Completed, None).await?;
        return Ok(());
    }

    let state_detail = db::get_game_detail(pool, game_id).await?;
    let applied = apply_state_changes(
        pool,
        game_id,
        turn_id,
        &response.state_changes,
        &state_detail.actors,
        &state_detail.state,
    )
    .await?;

    if applied.is_empty() {
        db::complete_job(pool, job_id, dreamwell_types::JobStatus::Completed, None).await?;
        return Ok(());
    }

    let mut merged = turn.state_changes.clone();
    merged.extend(applied);
    db::update_turn_state_changes(pool, turn_id, &merged).await?;
    db::touch_game(pool, game_id).await?;
    db::complete_job(pool, job_id, dreamwell_types::JobStatus::Completed, None).await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recheck_prompt_includes_state_and_prose() {
        let prompt = build_recheck_prompt(
            "Stress rises as the alarm sounds.",
            "## Alex (pc)\n- stress (resource): 2/5",
            "Characters:\n## Alex (PC)\nA thief",
            "Track alarm clock.",
        );
        let user = prompt[1]["content"].as_str().unwrap();
        assert!(user.contains("Current typed state:"));
        assert!(user.contains("stress (resource)"));
        assert!(user.contains("Stress rises"));
        assert!(user.contains("Track alarm clock."));
        assert!(user.contains("Characters:"));
        assert!(user.contains("Alex (PC)"));
    }

    #[test]
    fn state_recheck_schema_requires_state_changes() {
        let schema = state_recheck_schema();
        let required = schema["required"].as_array().unwrap();
        assert!(required.iter().any(|v| v == "state_changes"));
    }
}
