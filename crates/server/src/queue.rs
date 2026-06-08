use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::Instant;

use dreamwell_types::{Job, JobStatus, JobType};
use futures_util::StreamExt;
use sqlx::SqlitePool;
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;

use crate::db;
use crate::error::{AppError, AppResult};
use crate::facts::{apply_fact_updates, extract_facts_from_text};
use crate::inference::{chat_completion, stream_chat_completion};
use crate::prompts::build_messages_for_inference;
use crate::story_prompts::{
    build_beat_outline_messages, build_beat_prose_messages, build_chapter_outline_messages,
    parse_outline_json,
};
use crate::summarize::maybe_summarize_chat;
use crate::thoughts::{parse_thought_blocks, strip_thought_blocks};

fn display_generated_text(settings: &dreamwell_types::Settings, text: &str) -> String {
    if settings.thought_blocks_enabled {
        strip_thought_blocks(text)
    } else {
        text.to_string()
    }
}

fn thought_timing(
    parsed: &crate::thoughts::ParsedThoughts,
    thought_started_at: &mut Option<Instant>,
    thought_duration_ms: &mut Option<i64>,
) -> (Option<i64>, bool) {
    if !parsed.thought_complete {
        if thought_started_at.is_none() {
            *thought_started_at = Some(Instant::now());
        }
        return (*thought_duration_ms, true);
    }

    if thought_duration_ms.is_none() {
        if let Some(start) = thought_started_at {
            *thought_duration_ms = Some(start.elapsed().as_millis() as i64);
        } else if !parsed.thought.is_empty() {
            *thought_duration_ms = Some(0);
        }
    }

    (*thought_duration_ms, false)
}

#[derive(Clone)]
pub struct JobQueue {
    pool: SqlitePool,
    work_tx: mpsc::UnboundedSender<()>,
    active_tokens: Arc<Mutex<HashMap<i64, CancellationToken>>>,
}

impl JobQueue {
    pub fn new(pool: SqlitePool) -> Self {
        let (work_tx, work_rx) = mpsc::unbounded_channel();
        let queue = Self {
            pool: pool.clone(),
            work_tx,
            active_tokens: Arc::new(Mutex::new(HashMap::new())),
        };
        queue.spawn_loop(work_rx);
        queue.wake();
        queue
    }

    pub fn wake(&self) {
        let _ = self.work_tx.send(());
    }

    pub async fn cancel_job(&self, pool: &SqlitePool, job_id: i64) -> AppResult<Job> {
        let job = db::get_job(pool, job_id).await?;
        match job.status {
            JobStatus::Queued | JobStatus::Running => {
                if job.status == JobStatus::Running {
                    if let Some(token) = self
                        .active_tokens
                        .lock()
                        .map_err(|_| AppError::internal("token map poisoned"))?
                        .get(&job_id)
                    {
                        token.cancel();
                    }
                }
                cancel_job_record(pool, &job).await?;
                self.wake();
            }
            _ => {
                return Err(AppError::bad_request("Job is not active"));
            }
        }
        db::get_job(pool, job_id).await
    }

    fn spawn_loop(&self, mut work_rx: mpsc::UnboundedReceiver<()>) {
        let pool = self.pool.clone();
        let work_tx = self.work_tx.clone();
        let active_tokens = self.active_tokens.clone();
        tokio::spawn(async move {
            tracing::info!("generation queue worker started");
            while work_rx.recv().await.is_some() {
                loop {
                    let ids = match db::claim_jobs(&pool, 8).await {
                        Ok(ids) => ids,
                        Err(err) => {
                            tracing::error!(%err, "claim_jobs failed");
                            break;
                        }
                    };
                    if ids.is_empty() {
                        break;
                    }
                    for job_id in ids {
                        let pool = pool.clone();
                        let work_tx = work_tx.clone();
                        let active_tokens = active_tokens.clone();
                        let token = CancellationToken::new();
                        match active_tokens.lock() {
                            Ok(mut tokens) => tokens.insert(job_id, token.clone()),
                            Err(err) => {
                                tracing::error!(%err, "token map poisoned while enqueueing job");
                                continue;
                            }
                        };
                        tokio::spawn(async move {
                            run_job_guarded(&pool, job_id, token).await;
                            if let Ok(mut tokens) = active_tokens.lock() {
                                tokens.remove(&job_id);
                            }
                            let _ = work_tx.send(());
                        });
                    }
                }
            }
            tracing::warn!("generation queue worker stopped");
        });
    }
}

async fn run_job_guarded(pool: &SqlitePool, job_id: i64, token: CancellationToken) {
    if let Err(err) = run_job(pool, job_id, token).await {
        tracing::error!(job_id, %err, "job failed");
        if let Ok(job) = db::get_job(pool, job_id).await {
            if job.status == JobStatus::Running {
                let _ = fail_job(pool, job_id, &job, &err.to_string()).await;
            }
        }
    }
}

async fn run_job(pool: &SqlitePool, job_id: i64, token: CancellationToken) -> AppResult<()> {
    let job = db::get_job(pool, job_id).await?;
    if job.status != JobStatus::Running {
        return Ok(());
    }

    if token.is_cancelled() {
        return cancel_job_record(pool, &job).await;
    }

    let settings = db::get_settings(pool).await?;
    if settings.model.is_empty() {
        return fail_job(pool, job_id, &job, "No model selected in settings").await;
    }

    match job.job_type {
        JobType::ChatMessage => run_chat_job(pool, job_id, &job, &settings, token).await,
        JobType::StoryChapterOutline => {
            run_story_chapter_outline(pool, job_id, &job, &settings, token).await
        }
        JobType::StoryBeatOutline => {
            run_story_beat_outline(pool, job_id, &job, &settings, token).await
        }
        JobType::StoryBeatProse => run_story_beat_prose(pool, job_id, &job, &settings, token).await,
    }
}

async fn cancel_job_record(pool: &SqlitePool, job: &Job) -> AppResult<()> {
    let current = db::get_job(pool, job.id).await?;
    if current.status == JobStatus::Cancelled {
        return Ok(());
    }
    if !matches!(current.status, JobStatus::Queued | JobStatus::Running) {
        return Ok(());
    }

    db::complete_job(pool, job.id, JobStatus::Cancelled, None).await?;
    match job.job_type {
        JobType::ChatMessage => {
            if let (Some(chat_id), Some(message_id)) = (job.chat_id, job.message_id) {
                let messages = db::list_messages(pool, chat_id).await?;
                if let Some(message) = messages.iter().find(|m| m.id == message_id) {
                    if message.content.is_empty() {
                        db::update_message_content(pool, message_id, "[Generation cancelled]")
                            .await?;
                    }
                }
            }
        }
        JobType::StoryBeatProse => {
            if let (Some(story_id), Some(beat_id)) = (job.story_id, job.beat_id) {
                if let Ok(detail) = db::get_story_detail(pool, story_id).await {
                    let beat = detail
                        .chapters
                        .iter()
                        .flat_map(|c| c.beats.iter())
                        .find(|b| b.id == beat_id);
                    if beat.is_some_and(|b| b.content.is_empty()) {
                        db::update_beat_content(pool, beat_id, "[Generation cancelled]").await?;
                    }
                }
            }
        }
        JobType::StoryChapterOutline | JobType::StoryBeatOutline => {}
    }
    Ok(())
}

async fn fail_job(
    pool: &SqlitePool,
    job_id: i64,
    job: &dreamwell_types::Job,
    message: &str,
) -> AppResult<()> {
    db::complete_job(pool, job_id, JobStatus::Failed, Some(message.to_string())).await?;
    match job.job_type {
        JobType::ChatMessage => {
            if let Some(message_id) = job.message_id {
                db::update_message_content(
                    pool,
                    message_id,
                    &format!("[Generation failed: {message}]"),
                )
                .await?;
            }
        }
        JobType::StoryBeatProse => {
            if let Some(beat_id) = job.beat_id {
                db::update_beat_content(pool, beat_id, &format!("[Generation failed: {message}]"))
                    .await?;
            }
        }
        JobType::StoryChapterOutline | JobType::StoryBeatOutline => {}
    }
    Ok(())
}

async fn run_chat_job(
    pool: &SqlitePool,
    job_id: i64,
    job: &dreamwell_types::Job,
    settings: &dreamwell_types::Settings,
    token: CancellationToken,
) -> AppResult<()> {
    let chat_id = job
        .chat_id
        .ok_or_else(|| AppError::internal("chat job missing chat_id"))?;
    let message_id = job
        .message_id
        .ok_or_else(|| AppError::internal("chat job missing message_id"))?;

    let chat = db::get_chat(pool, chat_id).await?;
    let messages =
        build_messages_for_inference(pool, chat_id, &chat.summary, chat.character_id, settings)
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
    let mut thought_started_at: Option<Instant> = None;
    let mut thought_duration_ms: Option<i64> = None;
    while let Some(token_result) = stream.next().await {
        if token.is_cancelled() {
            return cancel_job_record(pool, job).await;
        }
        match token_result {
            Ok(piece) => {
                accumulated.push_str(&piece);
                if settings.thought_blocks_enabled {
                    let parsed = parse_thought_blocks(&accumulated);
                    let (duration_ms, in_progress) =
                        thought_timing(&parsed, &mut thought_started_at, &mut thought_duration_ms);
                    db::update_message_generation(
                        pool,
                        message_id,
                        &parsed.reply,
                        &parsed.thought,
                        duration_ms,
                        in_progress,
                    )
                    .await?;
                } else {
                    db::update_message_content(pool, message_id, &accumulated).await?;
                }
                db::touch_chat(pool, chat_id).await?;
            }
            Err(err) => {
                db::complete_job(pool, job_id, JobStatus::Failed, Some(err.to_string())).await?;
                if accumulated.is_empty() {
                    db::update_message_content(
                        pool,
                        message_id,
                        &format!("[Generation failed: {err}]"),
                    )
                    .await?;
                }
                return Ok(());
            }
        }
    }

    if token.is_cancelled() {
        return cancel_job_record(pool, job).await;
    }

    let (processed, thought_content, thought_duration_ms, thought_in_progress) =
        if settings.thought_blocks_enabled {
            let parsed = parse_thought_blocks(&accumulated);
            let (duration_ms, in_progress) =
                thought_timing(&parsed, &mut thought_started_at, &mut thought_duration_ms);
            let final_duration = if in_progress {
                duration_ms
                    .or_else(|| thought_started_at.map(|start| start.elapsed().as_millis() as i64))
            } else {
                duration_ms
            };
            (parsed.reply, parsed.thought, final_duration, false)
        } else {
            (
                display_generated_text(settings, &accumulated),
                String::new(),
                None,
                false,
            )
        };

    let (cleaned, updates) = extract_facts_from_text(&processed);
    if settings.facts_enabled && !updates.is_empty() {
        if settings.thought_blocks_enabled {
            db::update_message_generation(
                pool,
                message_id,
                &cleaned,
                &thought_content,
                thought_duration_ms,
                thought_in_progress,
            )
            .await?;
        } else {
            db::update_message_content(pool, message_id, &cleaned).await?;
        }
        apply_fact_updates(pool, chat_id, &updates).await?;
    } else if settings.thought_blocks_enabled
        && (thought_in_progress || thought_duration_ms.is_some() || !thought_content.is_empty())
    {
        db::update_message_generation(
            pool,
            message_id,
            &processed,
            &thought_content,
            thought_duration_ms,
            thought_in_progress,
        )
        .await?;
    }

    db::complete_job(pool, job_id, JobStatus::Completed, None).await?;
    maybe_summarize_chat(pool, chat_id, settings).await?;
    Ok(())
}

async fn run_story_chapter_outline(
    pool: &SqlitePool,
    job_id: i64,
    job: &dreamwell_types::Job,
    settings: &dreamwell_types::Settings,
    token: CancellationToken,
) -> AppResult<()> {
    let story_id = job
        .story_id
        .ok_or_else(|| AppError::internal("story job missing story_id"))?;
    let chapter_id = job
        .chapter_id
        .ok_or_else(|| AppError::internal("chapter job missing chapter_id"))?;

    let detail = db::get_story_detail(pool, story_id).await?;
    let chapter = detail
        .chapters
        .iter()
        .find(|c| c.id == chapter_id)
        .ok_or_else(|| AppError::internal("chapter not found"))?;

    let messages = build_chapter_outline_messages(
        &detail.story,
        &detail.chapters,
        chapter.sort_order,
        &job.guidance_notes,
    );

    let response = tokio::select! {
        () = token.cancelled() => {
            return cancel_job_record(pool, job).await;
        }
        result = chat_completion(
            &settings.inference_url,
            &settings.model,
            &messages,
            0.7,
            settings.top_p,
            512,
        ) => result,
    };

    match response {
        Ok(text) => {
            let text = display_generated_text(settings, &text);
            if let Some((title, synopsis)) = parse_outline_json(&text) {
                db::update_chapter_outline(pool, chapter_id, &title, &synopsis).await?;
                db::touch_story(pool, story_id).await?;
                db::complete_job(pool, job_id, JobStatus::Completed, None).await?;
            } else {
                fail_job(pool, job_id, job, "Failed to parse chapter outline JSON").await?;
            }
        }
        Err(err) => {
            fail_job(pool, job_id, job, &err.to_string()).await?;
        }
    }
    Ok(())
}

async fn run_story_beat_outline(
    pool: &SqlitePool,
    job_id: i64,
    job: &dreamwell_types::Job,
    settings: &dreamwell_types::Settings,
    token: CancellationToken,
) -> AppResult<()> {
    let story_id = job
        .story_id
        .ok_or_else(|| AppError::internal("story job missing story_id"))?;
    let chapter_id = job
        .chapter_id
        .ok_or_else(|| AppError::internal("beat job missing chapter_id"))?;
    let beat_id = job
        .beat_id
        .ok_or_else(|| AppError::internal("beat job missing beat_id"))?;

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

    let messages = build_beat_outline_messages(
        &detail.story,
        &detail.chapters,
        chapter,
        beat.sort_order,
        &job.guidance_notes,
    );

    let response = tokio::select! {
        () = token.cancelled() => {
            return cancel_job_record(pool, job).await;
        }
        result = chat_completion(
            &settings.inference_url,
            &settings.model,
            &messages,
            0.7,
            settings.top_p,
            512,
        ) => result,
    };

    match response {
        Ok(text) => {
            let text = display_generated_text(settings, &text);
            if let Some((title, synopsis)) = parse_outline_json(&text) {
                db::update_beat_outline(pool, beat_id, &title, &synopsis).await?;
                db::touch_story(pool, story_id).await?;
                db::complete_job(pool, job_id, JobStatus::Completed, None).await?;
            } else {
                fail_job(pool, job_id, job, "Failed to parse beat outline JSON").await?;
            }
        }
        Err(err) => {
            fail_job(pool, job_id, job, &err.to_string()).await?;
        }
    }
    Ok(())
}

async fn run_story_beat_prose(
    pool: &SqlitePool,
    job_id: i64,
    job: &dreamwell_types::Job,
    settings: &dreamwell_types::Settings,
    token: CancellationToken,
) -> AppResult<()> {
    let story_id = job
        .story_id
        .ok_or_else(|| AppError::internal("story job missing story_id"))?;
    let chapter_id = job
        .chapter_id
        .ok_or_else(|| AppError::internal("prose job missing chapter_id"))?;
    let beat_id = job
        .beat_id
        .ok_or_else(|| AppError::internal("prose job missing beat_id"))?;

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

    let messages = build_beat_prose_messages(
        &detail.story,
        &detail.chapters,
        chapter,
        beat,
        &job.guidance_notes,
    );

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
    while let Some(token_result) = stream.next().await {
        if token.is_cancelled() {
            return cancel_job_record(pool, job).await;
        }
        match token_result {
            Ok(piece) => {
                accumulated.push_str(&piece);
                let display = display_generated_text(settings, &accumulated);
                db::update_beat_content(pool, beat_id, &display).await?;
                db::touch_story(pool, story_id).await?;
            }
            Err(err) => {
                db::complete_job(pool, job_id, JobStatus::Failed, Some(err.to_string())).await?;
                if accumulated.is_empty() {
                    db::update_beat_content(pool, beat_id, &format!("[Generation failed: {err}]"))
                        .await?;
                }
                return Ok(());
            }
        }
    }

    if token.is_cancelled() {
        return cancel_job_record(pool, job).await;
    }

    db::complete_job(pool, job_id, JobStatus::Completed, None).await?;
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

pub async fn enqueue_story_generation(
    queue: &JobQueue,
    job: dreamwell_types::Job,
) -> AppResult<dreamwell_types::Job> {
    queue.wake();
    Ok(job)
}
