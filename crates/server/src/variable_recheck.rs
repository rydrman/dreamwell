use dreamwell_types::{Job, JobType, MessageRole, Settings};
use sqlx::SqlitePool;
use tokio::sync::mpsc;

use crate::db;
use crate::error::{AppError, AppResult};

pub async fn enqueue_variable_recheck_for_message(
    pool: &SqlitePool,
    work_tx: &mpsc::UnboundedSender<()>,
    chat_id: i64,
    message_id: i64,
    settings: &Settings,
) -> AppResult<Job> {
    if !settings.variables_enabled {
        return Err(AppError::bad_request(
            "Chat variables are disabled in settings",
        ));
    }
    if settings.model.is_empty() {
        return Err(AppError::bad_request(
            "Configure an inference model in Settings before rechecking variables",
        ));
    }
    if !db::is_last_message(pool, chat_id, message_id).await? {
        return Err(AppError::bad_request(
            "Only the latest message can be rechecked",
        ));
    }
    if db::has_active_variable_recheck_job(pool, message_id).await? {
        return Err(AppError::bad_request(
            "A variable recheck is already in progress for this message",
        ));
    }

    let message = db::get_message(pool, chat_id, message_id).await?;
    if message.role != MessageRole::Assistant {
        return Err(AppError::bad_request(
            "Only assistant messages can be rechecked",
        ));
    }
    if message.is_summary {
        return Err(AppError::bad_request("Cannot recheck summary messages"));
    }
    if message.content.trim().is_empty() {
        return Err(AppError::bad_request("Message has no content to recheck"));
    }

    for job in db::list_active_jobs_for_message(pool, message_id).await? {
        if job.job_type == JobType::ChatMessage {
            return Err(AppError::bad_request(
                "Wait for generation to finish before rechecking variables",
            ));
        }
    }

    let job = db::enqueue_variable_recheck_job(pool, chat_id, message_id).await?;
    let _ = work_tx.send(());
    Ok(job)
}
