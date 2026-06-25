use dreamwell_types::Settings;
use sqlx::SqlitePool;

use crate::config;
use crate::db;
use crate::error::{AppError, AppResult};
use crate::model_fallback::chat_completion_with_connection_fallback;
use crate::story_prompts::build_beat_mechanical_messages;
use crate::summarize::process_summarize_output;

fn max_retries() -> u32 {
    config::GENERATION_MAX_RETRIES
        .load(std::sync::atomic::Ordering::SeqCst)
        .max(1)
}

#[allow(clippy::too_many_arguments)]
pub async fn run_story_beat_mechanical_job(
    pool: &SqlitePool,
    job_id: i64,
    story_id: i64,
    chapter_id: i64,
    beat_id: i64,
    guidance: &str,
    settings: &Settings,
) -> AppResult<()> {
    let detail = db::get_story_detail(pool, story_id).await?;
    let chapter = detail
        .chapters
        .iter()
        .find(|c| c.id == chapter_id)
        .ok_or_else(|| AppError::internal("chapter not found"))?;
    let beat = chapter
        .beats
        .iter()
        .find(|b| b.id == beat_id)
        .ok_or_else(|| AppError::internal("beat not found"))?;

    let messages =
        build_beat_mechanical_messages(&detail.story, &detail.chapters, chapter, beat, guidance);

    let max_attempts = max_retries();
    let mut raw = None;

    for attempt in 1..=max_attempts {
        match chat_completion_with_connection_fallback(
            pool,
            settings,
            &messages,
            Some(job_id),
            None,
            None,
        )
        .await
        {
            Ok(response) => {
                raw = Some(response);
                break;
            }
            Err(err) => {
                let last_error = err.to_string();
                if attempt == max_attempts {
                    return Err(AppError::inference(last_error));
                }
                tracing::warn!(
                    attempt,
                    max_attempts,
                    error = %last_error,
                    "retrying beat mechanical generation"
                );
            }
        }
    }

    let mechanical =
        process_summarize_output(&raw.unwrap_or_default()).map_err(AppError::bad_request)?;
    if mechanical.trim().is_empty() {
        return Err(AppError::inference("model returned no mechanical plan"));
    }

    db::update_beat_mechanical(pool, beat_id, &mechanical).await?;
    db::touch_story(pool, story_id).await?;
    db::complete_job(pool, job_id, dreamwell_types::JobStatus::Completed, None).await?;
    Ok(())
}
