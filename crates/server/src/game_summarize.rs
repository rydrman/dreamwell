use dreamwell_types::Settings;
use sqlx::SqlitePool;

use crate::config;
use crate::db;
use crate::error::{AppError, AppResult};
use crate::game_prompts::build_scene_summarize_messages;
use crate::inference::chat_completion;
use crate::summarize::process_summarize_output;

fn summarize_output_tokens(settings: &Settings) -> i64 {
    if settings.context_tokens > 0 {
        (settings.context_tokens / 8).clamp(256, 512)
    } else {
        384
    }
}

fn max_retries() -> u32 {
    config::GENERATION_MAX_RETRIES
        .load(std::sync::atomic::Ordering::SeqCst)
        .max(1)
}

pub async fn run_game_scene_summarize_job(
    pool: &SqlitePool,
    job_id: i64,
    game_id: i64,
    settings: &Settings,
) -> AppResult<()> {
    let detail = db::get_game_detail(pool, game_id).await?;
    let scene = detail
        .scenes
        .first()
        .ok_or_else(|| AppError::internal("no scene found"))?;

    let messages = build_scene_summarize_messages(&detail);
    let max_attempts = max_retries();
    let mut raw = None;

    for attempt in 1..=max_attempts {
        match chat_completion(
            &settings.inference_url,
            &settings.model,
            &messages,
            0.3,
            settings.top_p,
            summarize_output_tokens(settings),
        )
        .await
        {
            Ok(response) => {
                raw = Some(response);
                break;
            }
            Err(err) => {
                if attempt == max_attempts {
                    return Err(AppError::inference(err.to_string()));
                }
            }
        }
    }

    let summary =
        process_summarize_output(&raw.unwrap_or_default()).map_err(AppError::bad_request)?;
    if summary.trim().is_empty() {
        return Err(AppError::inference("model returned no summary"));
    }

    db::update_scene_summary(pool, scene.id, &summary).await?;
    db::touch_game(pool, game_id).await?;
    db::complete_job(pool, job_id, dreamwell_types::JobStatus::Completed, None).await?;
    Ok(())
}

pub async fn maybe_enqueue_scene_summarize(pool: &SqlitePool, game_id: i64) -> AppResult<()> {
    let detail = db::get_game_detail(pool, game_id).await?;
    let turn_count = detail.turns.len() as i64;
    if turn_count < 3 {
        return Ok(());
    }
    let scene = match detail.scenes.first() {
        Some(s) if !s.summary_valid && turn_count - s.start_turn >= 3 => s,
        _ => return Ok(()),
    };
    let active = db::get_active_game_job(pool, game_id).await?;
    if active.is_some() {
        return Ok(());
    }
    db::enqueue_game_job(
        pool,
        dreamwell_types::JobType::GameSceneSummarize,
        game_id,
        None,
        String::new(),
    )
    .await?;
    let _ = scene;
    Ok(())
}
