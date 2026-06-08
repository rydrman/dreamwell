use std::sync::Arc;

use dreamwell_types::JobStatus;
use futures_util::StreamExt;
use sqlx::SqlitePool;
use tokio::sync::Notify;

use crate::db;
use crate::error::AppResult;
use crate::facts::{apply_fact_updates, extract_facts_from_text};
use crate::inference::stream_chat_completion;
use crate::prompts::build_messages_for_inference;
use crate::summarize::maybe_summarize_chat;

#[derive(Clone)]
pub struct JobQueue {
    pool: SqlitePool,
    notify: Arc<Notify>,
}

impl JobQueue {
    pub fn new(pool: SqlitePool) -> Self {
        let queue = Self {
            pool: pool.clone(),
            notify: Arc::new(Notify::new()),
        };
        queue.spawn_loop();
        queue
    }

    pub fn wake(&self) {
        self.notify.notify_one();
    }

    fn spawn_loop(&self) {
        let pool = self.pool.clone();
        let notify = self.notify.clone();
        tokio::spawn(async move {
            loop {
                if let Ok(ids) = db::claim_jobs(&pool, 8).await {
                    for job_id in ids {
                        let pool = pool.clone();
                        let notify = notify.clone();
                        tokio::spawn(async move {
                            let _ = run_job(&pool, job_id).await;
                            notify.notify_one();
                        });
                    }
                }
                notify.notified().await;
            }
        });
    }
}

async fn run_job(pool: &SqlitePool, job_id: i64) -> AppResult<()> {
    let job = db::get_job(pool, job_id).await?;
    if job.status != JobStatus::Running {
        return Ok(());
    }

    let settings = db::get_settings(pool).await?;
    if settings.model.is_empty() {
        db::complete_job(
            pool,
            job_id,
            JobStatus::Failed,
            Some("No model selected in settings".to_string()),
        )
        .await?;
        db::update_message_content(
            pool,
            job.message_id,
            "[Generation failed: no model selected]",
        )
        .await?;
        return Ok(());
    }

    let chat = db::get_chat(pool, job.chat_id).await?;
    let messages = build_messages_for_inference(
        pool,
        job.chat_id,
        &chat.summary,
        chat.character_id,
        &settings,
    )
    .await?;

    let mut stream = stream_chat_completion(
        &settings.inference_url,
        &settings.model,
        &messages,
        settings.temperature,
        settings.top_p,
        settings.max_tokens,
    )
    .await?;

    let mut accumulated = String::new();
    while let Some(token) = stream.next().await {
        match token {
            Ok(piece) => {
                accumulated.push_str(&piece);
                db::update_message_content(pool, job.message_id, &accumulated).await?;
                db::touch_chat(pool, job.chat_id).await?;
            }
            Err(err) => {
                db::complete_job(pool, job_id, JobStatus::Failed, Some(err.to_string())).await?;
                if accumulated.is_empty() {
                    db::update_message_content(
                        pool,
                        job.message_id,
                        &format!("[Generation failed: {err}]"),
                    )
                    .await?;
                }
                return Ok(());
            }
        }
    }

    let (cleaned, updates) = extract_facts_from_text(&accumulated);
    if settings.facts_enabled && !updates.is_empty() {
        db::update_message_content(pool, job.message_id, &cleaned).await?;
        apply_fact_updates(pool, job.chat_id, &updates).await?;
    }

    db::complete_job(pool, job_id, JobStatus::Completed, None).await?;
    maybe_summarize_chat(pool, job.chat_id, &settings).await?;
    Ok(())
}

pub async fn enqueue_generation(
    pool: &SqlitePool,
    queue: &JobQueue,
    chat_id: i64,
    message_id: i64,
) -> AppResult<dreamwell_types::Job> {
    let job = db::enqueue_job(pool, chat_id, message_id).await?;
    queue.wake();
    Ok(job)
}
