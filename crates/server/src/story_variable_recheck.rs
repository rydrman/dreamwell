use dreamwell_types::{Job, Settings};
use sqlx::SqlitePool;
use tokio::sync::mpsc;

use crate::db;
use crate::error::{AppError, AppResult};
use crate::variables::strip_variables_for_display;

pub async fn enqueue_beat_variable_recheck(
    pool: &SqlitePool,
    work_tx: &mpsc::UnboundedSender<()>,
    story_id: i64,
    chapter_id: i64,
    beat_id: i64,
    guidance_notes: &str,
    settings: &Settings,
) -> AppResult<Job> {
    if !settings.variables_enabled {
        return Err(AppError::bad_request(
            "Story variables are disabled in settings",
        ));
    }
    if settings.model.is_empty() {
        return Err(AppError::bad_request(
            "Configure an inference model in Settings before rechecking variables",
        ));
    }
    let beat = db::get_beat(pool, story_id, chapter_id, beat_id).await?;
    if strip_variables_for_display(&beat.content, false)
        .trim()
        .is_empty()
    {
        return Err(AppError::bad_request("Beat has no prose to recheck"));
    }
    if db::has_active_beat_job(pool, beat_id).await? {
        return Err(AppError::bad_request(
            "Wait for the current beat job to finish before rechecking variables",
        ));
    }

    let job = db::enqueue_beat_variable_recheck_job(
        pool,
        story_id,
        chapter_id,
        beat_id,
        guidance_notes.to_string(),
    )
    .await?;
    let _ = work_tx.send(());
    Ok(job)
}
