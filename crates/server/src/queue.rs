use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use dreamwell_types::{structured_output_tokens, Job, JobStatus, JobType};
use futures_util::StreamExt;
use sqlx::SqlitePool;
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;

use crate::chat_prompts::{build_plan_messages, build_prose_messages, chat_plan_schema};
use crate::chat_state::{apply_state_changes, build_state_block};
use crate::config;
use crate::db;
use crate::error::{AppError, AppResult};
use crate::inference::{chat_completion, stream_chat_completion};
use crate::message_followups::{enqueue_chat_followups, ChatGenerationComplete};
use crate::prompts::build_messages_for_inference;
use crate::story_prompts::{
    build_beat_outline_messages, build_beat_prose_continue_messages,
    build_beat_prose_continue_typed_messages, build_beat_prose_messages,
    build_chapter_outline_messages, build_propose_beats_messages, build_propose_chapters_messages,
    build_story_plan_messages, build_story_prose_from_plan_messages, parse_beats_proposal_json,
    parse_chapters_proposal_json, parse_outline_json, story_plan_schema,
};
use crate::story_state::{
    apply_state_changes as apply_story_state_changes, build_state_block as build_story_state_block,
};
use crate::summarize::{
    enqueue_regenerate_summary_for_chat, enqueue_summarize_for_chat, run_summarize_job,
    summarize_job_kind,
};
use crate::thoughts::{parse_thought_blocks, strip_thought_blocks};
use crate::variable_recheck::enqueue_variable_recheck_for_message;

/// Slightly above inference request timeout so hung jobs do not block the queue forever.
const STUCK_JOB_MAX_AGE_SECS: i64 = 920;
const QUEUE_POLL_INTERVAL: Duration = Duration::from_secs(30);
/// Limit how often streaming generation writes partial content (SSE polls every ~250ms).
const STREAM_DB_FLUSH_INTERVAL: Duration = Duration::from_millis(200);

struct StreamDbThrottle {
    last_flush: Instant,
}

impl StreamDbThrottle {
    fn new() -> Self {
        Self {
            last_flush: Instant::now() - STREAM_DB_FLUSH_INTERVAL,
        }
    }

    fn ready(&self) -> bool {
        self.last_flush.elapsed() >= STREAM_DB_FLUSH_INTERVAL
    }

    fn mark_flushed(&mut self) {
        self.last_flush = Instant::now();
    }
}

fn generation_max_retries() -> u32 {
    config::GENERATION_MAX_RETRIES
        .load(std::sync::atomic::Ordering::SeqCst)
        .max(1)
}

async fn wait_for_generation_retry(attempt: u32, token: &CancellationToken) -> bool {
    if token.is_cancelled() {
        return true;
    }
    let secs = 1u64 << (attempt - 2).min(3);
    tokio::select! {
        () = token.cancelled() => true,
        () = tokio::time::sleep(Duration::from_secs(secs)) => token.is_cancelled(),
    }
}

enum ChatGenerationOutcome {
    Success,
    Retryable(String),
    Failed,
    Cancelled,
}

enum BeatProseOutcome {
    Success,
    Retryable(String),
    Failed,
    Cancelled,
}

fn display_generated_text(settings: &dreamwell_types::Settings, text: &str) -> String {
    if settings.thought_blocks_enabled {
        strip_thought_blocks(text)
    } else {
        text.to_string()
    }
}

fn display_beat_prose(settings: &dreamwell_types::Settings, text: &str, streaming: bool) -> String {
    let text = display_generated_text(settings, text);
    if settings.variables_enabled {
        crate::variables::strip_variables_for_display(&text, streaming)
    } else {
        text
    }
}

/// Join existing beat prose with newly generated continuation text.
///
/// Ensures exactly one blank line between the prior text and the new prose,
/// regardless of trailing whitespace on the base or leading whitespace from the model.
fn append_prose_continuation(base: &str, continuation: &str) -> String {
    let base = base.trim_end();
    let continuation = continuation.trim_start();
    match (base.is_empty(), continuation.is_empty()) {
        (true, true) => String::new(),
        (true, false) => continuation.to_string(),
        (false, true) => format!("{base}\n\n"),
        (false, false) => format!("{base}\n\n{continuation}"),
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

    if thought_duration_ms.is_none() && !parsed.thought.is_empty() {
        if let Some(start) = thought_started_at {
            *thought_duration_ms = Some(start.elapsed().as_millis() as i64);
        } else {
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

    pub async fn enqueue_summarize(
        &self,
        pool: &SqlitePool,
        chat_id: i64,
        settings: &dreamwell_types::Settings,
    ) -> AppResult<dreamwell_types::Job> {
        let job = enqueue_summarize_for_chat(pool, &self.work_tx, chat_id, settings).await?;
        self.wake();
        Ok(job)
    }

    pub async fn enqueue_regenerate_summary(
        &self,
        pool: &SqlitePool,
        chat_id: i64,
        marker_id: i64,
        settings: &dreamwell_types::Settings,
    ) -> AppResult<dreamwell_types::Job> {
        let job =
            enqueue_regenerate_summary_for_chat(pool, &self.work_tx, chat_id, marker_id, settings)
                .await?;
        self.wake();
        Ok(job)
    }

    pub async fn enqueue_variable_recheck(
        &self,
        pool: &SqlitePool,
        chat_id: i64,
        message_id: i64,
        settings: &dreamwell_types::Settings,
    ) -> AppResult<dreamwell_types::Job> {
        let job = enqueue_variable_recheck_for_message(
            pool,
            &self.work_tx,
            chat_id,
            message_id,
            settings,
        )
        .await?;
        self.wake();
        Ok(job)
    }

    pub async fn enqueue_story_beat_variable_recheck(
        &self,
        pool: &SqlitePool,
        story_id: i64,
        chapter_id: i64,
        beat_id: i64,
        guidance_notes: &str,
        settings: &dreamwell_types::Settings,
    ) -> AppResult<dreamwell_types::Job> {
        let job = crate::story_variable_recheck::enqueue_beat_variable_recheck(
            pool,
            &self.work_tx,
            story_id,
            chapter_id,
            beat_id,
            guidance_notes,
            settings,
        )
        .await?;
        self.wake();
        Ok(job)
    }

    pub async fn enqueue_story_beat_prose_recheck(
        &self,
        pool: &SqlitePool,
        story_id: i64,
        chapter_id: i64,
        beat_id: i64,
        guidance_notes: &str,
        settings: &dreamwell_types::Settings,
    ) -> AppResult<dreamwell_types::Job> {
        let job = crate::story_beat_prose_recheck::enqueue_beat_prose_recheck(
            pool,
            &self.work_tx,
            story_id,
            chapter_id,
            beat_id,
            guidance_notes,
            settings,
        )
        .await?;
        self.wake();
        Ok(job)
    }

    pub async fn enqueue_game_prose_recheck(
        &self,
        pool: &SqlitePool,
        game_id: i64,
        turn_id: i64,
        guidance_notes: &str,
        settings: &dreamwell_types::Settings,
    ) -> AppResult<dreamwell_types::Job> {
        let job = crate::game_prose_recheck::enqueue_turn_prose_recheck(
            pool,
            &self.work_tx,
            game_id,
            turn_id,
            guidance_notes,
            settings,
        )
        .await?;
        self.wake();
        Ok(job)
    }

    pub async fn enqueue_game_state_recheck(
        &self,
        pool: &SqlitePool,
        game_id: i64,
        turn_id: i64,
        guidance_notes: &str,
        settings: &dreamwell_types::Settings,
    ) -> AppResult<dreamwell_types::Job> {
        let job = crate::game_state_recheck::enqueue_turn_state_recheck(
            pool,
            &self.work_tx,
            game_id,
            turn_id,
            guidance_notes,
            settings,
        )
        .await?;
        self.wake();
        Ok(job)
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
            let mut poll = tokio::time::interval(QUEUE_POLL_INTERVAL);
            poll.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);

            loop {
                tokio::select! {
                    msg = work_rx.recv() => {
                        if msg.is_none() {
                            break;
                        }
                    }
                    _ = poll.tick() => {
                        if let Ok(running_ids) = db::list_running_job_ids(&pool).await {
                            let orphaned: Vec<i64> = match active_tokens.lock() {
                                Ok(tokens) => running_ids
                                    .into_iter()
                                    .filter(|id| !tokens.contains_key(id))
                                    .collect(),
                                Err(err) => {
                                    tracing::error!(%err, "token map poisoned during queue poll");
                                    vec![]
                                }
                            };
                            if !orphaned.is_empty() {
                                match db::requeue_jobs_by_id(&pool, &orphaned).await {
                                    Ok(requeued) if requeued > 0 => {
                                        tracing::warn!(requeued, "requeued orphaned running jobs");
                                    }
                                    Ok(_) => {}
                                    Err(err) => {
                                        tracing::error!(%err, "requeue_jobs_by_id failed");
                                    }
                                }
                            }
                        }
                        match db::requeue_stuck_jobs(&pool, STUCK_JOB_MAX_AGE_SECS).await {
                            Ok(requeued) if requeued > 0 => {
                                tracing::warn!(requeued, "requeued stuck running jobs");
                            }
                            Ok(_) => {}
                            Err(err) => tracing::error!(%err, "requeue_stuck_jobs failed"),
                        }
                    }
                }

                loop {
                    let ids = match db::claim_jobs(&pool, 8).await {
                        Ok(ids) => ids,
                        Err(err) => {
                            tracing::error!(%err, "claim_jobs failed");
                            tokio::time::sleep(Duration::from_millis(250)).await;
                            continue;
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
                            run_job_guarded(&pool, job_id, token, work_tx.clone()).await;
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

async fn run_job_guarded(
    pool: &SqlitePool,
    job_id: i64,
    token: CancellationToken,
    work_tx: mpsc::UnboundedSender<()>,
) {
    let run_result = run_job(pool, job_id, token, &work_tx).await;
    if let Err(err) = run_result {
        tracing::error!(job_id, %err, "job failed");
        if let Ok(job) = db::get_job(pool, job_id).await {
            if job.status == JobStatus::Running {
                let _ = fail_job(pool, job_id, &job, &err.to_string()).await;
            }
        }
    } else if let Ok(job) = db::get_job(pool, job_id).await {
        if job.status == JobStatus::Running {
            tracing::warn!(
                job_id,
                job_type = ?job.job_type,
                "job handler returned Ok but job is still running; completing"
            );
            let _ = db::complete_job(pool, job_id, JobStatus::Completed, None).await;
        }
    }
}

async fn run_job(
    pool: &SqlitePool,
    job_id: i64,
    token: CancellationToken,
    work_tx: &mpsc::UnboundedSender<()>,
) -> AppResult<()> {
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
        JobType::ChatMessage => run_chat_job(pool, job_id, &job, &settings, token, work_tx).await,
        JobType::ChatSummarize => {
            run_summarize_job_handler(pool, job_id, &job, &settings, token).await
        }
        JobType::ChatVariableRecheck => {
            run_variable_recheck_job_handler(pool, job_id, &job, &settings, token).await
        }
        JobType::StoryChapterOutline => {
            run_story_chapter_outline(pool, job_id, &job, &settings, token).await
        }
        JobType::StoryProposeChapters => {
            run_story_propose_chapters(pool, job_id, &job, &settings, token).await
        }
        JobType::StoryBeatOutline => {
            run_story_beat_outline(pool, job_id, &job, &settings, token).await
        }
        JobType::StoryProposeBeats => {
            run_story_propose_beats(pool, job_id, &job, &settings, token).await
        }
        JobType::StoryBeatProse | JobType::StoryBeatProseContinue => {
            run_story_beat_prose(pool, job_id, &job, &settings, token).await
        }
        JobType::StoryBeatMechanical => {
            run_story_beat_mechanical_handler(pool, job_id, &job, &settings, token).await
        }
        JobType::StoryChapterSummarize => {
            run_story_chapter_summarize_handler(pool, job_id, &job, &settings, token).await
        }
        JobType::StoryBeatVariableRecheck => {
            run_story_beat_variable_recheck_handler(pool, job_id, &job, &settings, token).await
        }
        JobType::StoryBeatProseRecheck => {
            run_story_beat_prose_recheck_handler(pool, job_id, &job, &settings, token).await
        }
        JobType::GameTurnStructuredAgent
        | JobType::GameSceneSummarize
        | JobType::GameProseRecheck
        | JobType::GameStateRecheck => {
            crate::game_turn::run_game_job(pool, job_id, &job, &settings, token, work_tx).await
        }
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
                    db::set_thought_in_progress(pool, message_id, false).await?;
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
        JobType::StoryBeatProseContinue => {}
        JobType::ChatSummarize => {
            remove_summarize_marker(pool, job).await?;
        }
        JobType::ChatVariableRecheck
        | JobType::StoryChapterOutline
        | JobType::StoryProposeChapters
        | JobType::StoryBeatOutline
        | JobType::StoryProposeBeats
        | JobType::StoryChapterSummarize
        | JobType::StoryBeatMechanical
        | JobType::StoryBeatVariableRecheck
        | JobType::StoryBeatProseRecheck
        | JobType::GameProseRecheck
        | JobType::GameStateRecheck
        | JobType::GameTurnStructuredAgent
        | JobType::GameSceneSummarize => {}
    }
    Ok(())
}

async fn remove_summarize_marker(pool: &SqlitePool, job: &dreamwell_types::Job) -> AppResult<()> {
    if let Some(message_id) = job.message_id {
        db::delete_messages(pool, &[message_id]).await?;
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
                db::fail_chat_message_generation(pool, message_id, message).await?;
            }
        }
        JobType::StoryBeatProse => {
            if let Some(beat_id) = job.beat_id {
                db::update_beat_content(pool, beat_id, &format!("[Generation failed: {message}]"))
                    .await?;
            }
        }
        JobType::StoryBeatProseContinue => {}
        JobType::ChatSummarize => {
            remove_summarize_marker(pool, job).await?;
        }
        JobType::ChatVariableRecheck
        | JobType::StoryChapterOutline
        | JobType::StoryProposeChapters
        | JobType::StoryBeatOutline
        | JobType::StoryProposeBeats
        | JobType::StoryChapterSummarize
        | JobType::StoryBeatMechanical
        | JobType::StoryBeatVariableRecheck
        | JobType::StoryBeatProseRecheck
        | JobType::GameTurnStructuredAgent => {
            if let Some(turn_id) = job.turn_id {
                let _ = db::update_turn_phase(pool, turn_id, "failed").await;
            }
        }
        JobType::GameSceneSummarize | JobType::GameProseRecheck | JobType::GameStateRecheck => {}
    }
    Ok(())
}

async fn run_variable_recheck_job_handler(
    pool: &SqlitePool,
    job_id: i64,
    job: &dreamwell_types::Job,
    settings: &dreamwell_types::Settings,
    token: CancellationToken,
) -> AppResult<()> {
    if token.is_cancelled() {
        return cancel_job_record(pool, job).await;
    }
    let chat_id = job
        .chat_id
        .ok_or_else(|| AppError::internal("variable recheck job missing chat_id"))?;
    let message_id = job
        .message_id
        .ok_or_else(|| AppError::internal("variable recheck job missing message_id"))?;

    match crate::state_recheck::run_chat_state_recheck_job(
        pool, job_id, chat_id, message_id, settings,
    )
    .await
    {
        Ok(()) => Ok(()),
        Err(err) => {
            fail_job(pool, job_id, job, &err.to_string()).await?;
            Ok(())
        }
    }
}

async fn run_summarize_job_handler(
    pool: &SqlitePool,
    job_id: i64,
    job: &dreamwell_types::Job,
    settings: &dreamwell_types::Settings,
    token: CancellationToken,
) -> AppResult<()> {
    if token.is_cancelled() {
        return cancel_job_record(pool, job).await;
    }
    let chat_id = job
        .chat_id
        .ok_or_else(|| AppError::internal("summarize job missing chat_id"))?;
    let marker_id = job
        .message_id
        .ok_or_else(|| AppError::internal("summarize job missing message_id"))?;

    let kind = summarize_job_kind(&job.guidance_notes);
    match run_summarize_job(pool, job_id, chat_id, marker_id, settings, kind).await {
        Ok(()) => Ok(()),
        Err(err) => {
            fail_job(pool, job_id, job, &err.to_string()).await?;
            Ok(())
        }
    }
}

async fn run_chat_job(
    pool: &SqlitePool,
    job_id: i64,
    job: &dreamwell_types::Job,
    settings: &dreamwell_types::Settings,
    token: CancellationToken,
    work_tx: &mpsc::UnboundedSender<()>,
) -> AppResult<()> {
    let chat_id = job
        .chat_id
        .ok_or_else(|| AppError::internal("chat job missing chat_id"))?;
    let message_id = job
        .message_id
        .ok_or_else(|| AppError::internal("chat job missing message_id"))?;

    let existing = db::get_message_generation_snapshot(pool, message_id).await?;
    if !existing.content.is_empty() && !existing.thought_in_progress {
        tracing::warn!(
            job_id,
            message_id,
            "message already has generated content; completing job without re-running"
        );
        db::complete_job(pool, job_id, JobStatus::Completed, None).await?;
        return Ok(());
    }

    let chat = db::get_chat(pool, chat_id).await?;
    let messages =
        build_messages_for_inference(pool, chat_id, &chat.summary, chat.character_id, settings)
            .await?;

    let max_attempts = generation_max_retries();
    let mut last_error = "model returned no text".to_string();

    for attempt in 1..=max_attempts {
        if token.is_cancelled() {
            return cancel_job_record(pool, job).await;
        }

        if attempt > 1 {
            tracing::warn!(
                job_id,
                attempt,
                max_attempts,
                error = %last_error,
                "retrying chat generation"
            );
            db::update_message_content(pool, message_id, "").await?;
            db::clear_message_thoughts(pool, message_id).await?;
            if wait_for_generation_retry(attempt, &token).await {
                return cancel_job_record(pool, job).await;
            }
        }

        match run_chat_generation_attempt(
            pool, job_id, chat_id, message_id, settings, &messages, &token,
        )
        .await?
        {
            ChatGenerationOutcome::Success => {
                db::complete_job(pool, job_id, JobStatus::Completed, None).await?;
                enqueue_chat_followups(&ChatGenerationComplete {
                    pool,
                    work_tx,
                    chat_id,
                    settings,
                })
                .await?;
                return Ok(());
            }
            ChatGenerationOutcome::Retryable(message) => {
                last_error = message;
                if attempt == max_attempts {
                    db::fail_chat_message_generation(pool, message_id, &last_error).await?;
                    db::complete_job(pool, job_id, JobStatus::Failed, Some(last_error)).await?;
                    return Ok(());
                }
            }
            ChatGenerationOutcome::Failed => return Ok(()),
            ChatGenerationOutcome::Cancelled => return cancel_job_record(pool, job).await,
        }
    }

    Ok(())
}

async fn run_chat_generation_attempt(
    pool: &SqlitePool,
    job_id: i64,
    chat_id: i64,
    message_id: i64,
    settings: &dreamwell_types::Settings,
    messages: &[serde_json::Value],
    token: &CancellationToken,
) -> AppResult<ChatGenerationOutcome> {
    if settings.variables_enabled {
        return run_chat_typed_generation_attempt(
            pool, job_id, chat_id, message_id, settings, messages, token,
        )
        .await;
    }
    run_chat_legacy_generation_attempt(pool, job_id, chat_id, message_id, settings, messages, token)
        .await
}

async fn run_chat_typed_generation_attempt(
    pool: &SqlitePool,
    job_id: i64,
    chat_id: i64,
    message_id: i64,
    settings: &dreamwell_types::Settings,
    _messages: &[serde_json::Value],
    token: &CancellationToken,
) -> AppResult<ChatGenerationOutcome> {
    let inference = db::get_inference_config(pool).await?;
    let chat = db::get_chat(pool, chat_id).await?;
    let character = db::get_character(pool, chat.character_id).await.ok();

    db::update_message_generation_phase(pool, message_id, "plan").await?;
    let plan_messages =
        build_plan_messages(pool, chat_id, &chat.summary, chat.character_id, settings).await?;

    let plan: dreamwell_types::PlanPhaseResponse = match db::chat_completion_json_for_connection(
        pool,
        &inference,
        &settings.model,
        &plan_messages,
        0.4,
        settings.top_p,
        structured_output_tokens(settings),
        Some(&chat_plan_schema()),
        generation_max_retries(),
        token,
    )
    .await
    {
        Ok(v) => v,
        Err(err) => return Ok(ChatGenerationOutcome::Retryable(err.to_string())),
    };

    if token.is_cancelled() {
        return Ok(ChatGenerationOutcome::Cancelled);
    }

    let actors = db::list_chat_actors(pool, chat_id).await?;
    let current = db::list_chat_state_entries(pool, chat_id).await?;
    let applied = apply_state_changes(
        pool,
        chat_id,
        message_id,
        &plan.state_changes,
        &actors,
        &current,
    )
    .await?;
    db::save_message_plan(pool, message_id, &plan.beats, &applied).await?;

    let state = db::list_chat_state_entries(pool, chat_id).await?;
    let state_block = build_state_block(&state, &actors);
    let prose_messages = build_prose_messages(
        pool,
        chat_id,
        &chat.summary,
        &plan.beats,
        &state_block,
        settings,
        character.as_ref(),
    )
    .await?;

    let mut stream = match stream_chat_completion(
        &inference,
        &settings.model,
        &prose_messages,
        settings.temperature,
        settings.top_p,
        settings.max_tokens,
    )
    .await
    {
        Ok(stream) => stream,
        Err(err) => return Ok(ChatGenerationOutcome::Retryable(err.to_string())),
    };

    let mut accumulated = String::new();
    let mut db_throttle = StreamDbThrottle::new();
    while let Some(token_result) = stream.next().await {
        if token.is_cancelled() {
            return Ok(ChatGenerationOutcome::Cancelled);
        }
        match token_result {
            Ok(piece) => {
                accumulated.push_str(&piece);
                if db_throttle.ready() {
                    db::update_message_content(pool, message_id, &accumulated).await?;
                    db::touch_chat(pool, chat_id).await?;
                    db_throttle.mark_flushed();
                }
            }
            Err(err) => {
                if accumulated.is_empty() {
                    return Ok(ChatGenerationOutcome::Retryable(err.to_string()));
                }
                db::complete_job(pool, job_id, JobStatus::Failed, Some(err.to_string())).await?;
                return Ok(ChatGenerationOutcome::Failed);
            }
        }
    }

    if token.is_cancelled() {
        return Ok(ChatGenerationOutcome::Cancelled);
    }

    if accumulated.trim().is_empty() {
        return Ok(ChatGenerationOutcome::Retryable(
            "model returned no text".to_string(),
        ));
    }

    db::finalize_message_typed_generation(
        pool,
        message_id,
        &accumulated,
        "",
        None,
        false,
        &plan.beats,
        &applied,
    )
    .await?;

    Ok(ChatGenerationOutcome::Success)
}

async fn run_chat_legacy_generation_attempt(
    pool: &SqlitePool,
    job_id: i64,
    chat_id: i64,
    message_id: i64,
    settings: &dreamwell_types::Settings,
    messages: &[serde_json::Value],
    token: &CancellationToken,
) -> AppResult<ChatGenerationOutcome> {
    let inference = db::get_inference_config(pool).await?;
    let mut stream = match stream_chat_completion(
        &inference,
        &settings.model,
        messages,
        settings.temperature,
        settings.top_p,
        settings.max_tokens,
    )
    .await
    {
        Ok(stream) => stream,
        Err(err) => return Ok(ChatGenerationOutcome::Retryable(err.to_string())),
    };

    let mut accumulated = String::new();
    let mut thought_started_at: Option<Instant> = None;
    let mut thought_duration_ms: Option<i64> = None;
    let mut db_throttle = StreamDbThrottle::new();
    while let Some(token_result) = stream.next().await {
        if token.is_cancelled() {
            return Ok(ChatGenerationOutcome::Cancelled);
        }
        match token_result {
            Ok(piece) => {
                accumulated.push_str(&piece);
                if db_throttle.ready() {
                    if settings.thought_blocks_enabled {
                        let parsed = parse_thought_blocks(&accumulated);
                        let (duration_ms, in_progress) = thought_timing(
                            &parsed,
                            &mut thought_started_at,
                            &mut thought_duration_ms,
                        );
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
                    db_throttle.mark_flushed();
                }
            }
            Err(err) => {
                if accumulated.is_empty() {
                    return Ok(ChatGenerationOutcome::Retryable(err.to_string()));
                }
                db::complete_job(pool, job_id, JobStatus::Failed, Some(err.to_string())).await?;
                if settings.thought_blocks_enabled {
                    let parsed = parse_thought_blocks(&accumulated);
                    db::update_message_generation(
                        pool,
                        message_id,
                        &parsed.reply,
                        &parsed.thought,
                        thought_duration_ms.filter(|_| !parsed.thought.is_empty()),
                        false,
                    )
                    .await?;
                } else {
                    db::set_thought_in_progress(pool, message_id, false).await?;
                }
                return Ok(ChatGenerationOutcome::Failed);
            }
        }
    }

    if token.is_cancelled() {
        return Ok(ChatGenerationOutcome::Cancelled);
    }

    if accumulated.trim().is_empty() {
        return Ok(ChatGenerationOutcome::Retryable(
            "model returned no text".to_string(),
        ));
    }

    let (processed, thought_content, thought_duration_ms, _thought_in_progress) =
        if settings.thought_blocks_enabled {
            let parsed = parse_thought_blocks(&accumulated);
            let (duration_ms, in_progress) =
                thought_timing(&parsed, &mut thought_started_at, &mut thought_duration_ms);
            let final_duration = if parsed.thought.is_empty() {
                None
            } else if in_progress {
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

    let processed = if processed.is_empty() && thought_content.is_empty() {
        display_generated_text(settings, &accumulated)
    } else {
        processed
    };

    let visible_text = processed.clone();

    if visible_text.trim().is_empty() {
        return Ok(ChatGenerationOutcome::Retryable(
            "model returned no visible text".to_string(),
        ));
    }

    if settings.thought_blocks_enabled {
        db::finalize_message_generation(
            pool,
            message_id,
            &processed,
            &thought_content,
            thought_duration_ms,
            false,
            &[],
        )
        .await?;
    }

    Ok(ChatGenerationOutcome::Success)
}

async fn run_story_propose_chapters(
    pool: &SqlitePool,
    job_id: i64,
    job: &dreamwell_types::Job,
    settings: &dreamwell_types::Settings,
    token: CancellationToken,
) -> AppResult<()> {
    let inference = db::get_inference_config(pool).await?;
    let story_id = job
        .story_id
        .ok_or_else(|| AppError::internal("story job missing story_id"))?;

    let detail = db::get_story_detail(pool, story_id).await?;
    let messages =
        build_propose_chapters_messages(&detail.story, &detail.chapters, &job.guidance_notes);

    let max_attempts = generation_max_retries();
    let mut last_error = "generation failed".to_string();

    for attempt in 1..=max_attempts {
        if token.is_cancelled() {
            return cancel_job_record(pool, job).await;
        }
        if attempt > 1 {
            tracing::warn!(
                job_id,
                attempt,
                max_attempts,
                error = %last_error,
                "retrying chapter proposal generation"
            );
            if wait_for_generation_retry(attempt, &token).await {
                return cancel_job_record(pool, job).await;
            }
        }

        let response = tokio::select! {
            () = token.cancelled() => {
                return cancel_job_record(pool, job).await;
            }
            result = chat_completion(
                &inference,
                &settings.model,
                &messages,
                0.7,
                settings.top_p,
                2048,
            ) => result,
        };

        match response {
            Ok(text) => {
                let text = display_generated_text(settings, &text);
                if text.trim().is_empty() {
                    last_error = "model returned no text".to_string();
                } else if let Some(chapters) = parse_chapters_proposal_json(&text) {
                    db::apply_chapter_proposal(pool, story_id, &chapters).await?;
                    db::complete_job(pool, job_id, JobStatus::Completed, None).await?;
                    return Ok(());
                } else {
                    last_error = "Failed to parse chapter proposal JSON".to_string();
                }
            }
            Err(err) => last_error = err.to_string(),
        }

        if attempt == max_attempts {
            fail_job(pool, job_id, job, &last_error).await?;
        }
    }
    Ok(())
}

async fn run_story_propose_beats(
    pool: &SqlitePool,
    job_id: i64,
    job: &dreamwell_types::Job,
    settings: &dreamwell_types::Settings,
    token: CancellationToken,
) -> AppResult<()> {
    let inference = db::get_inference_config(pool).await?;
    let story_id = job
        .story_id
        .ok_or_else(|| AppError::internal("story job missing story_id"))?;
    let chapter_id = job
        .chapter_id
        .ok_or_else(|| AppError::internal("beat proposal job missing chapter_id"))?;

    let detail = db::get_story_detail(pool, story_id).await?;
    let chapter = detail
        .chapters
        .iter()
        .find(|c| c.id == chapter_id)
        .ok_or_else(|| AppError::internal("chapter not found"))?;

    let messages = build_propose_beats_messages(
        &detail.story,
        &detail.chapters,
        chapter,
        &job.guidance_notes,
    );

    let max_attempts = generation_max_retries();
    let mut last_error = "generation failed".to_string();

    for attempt in 1..=max_attempts {
        if token.is_cancelled() {
            return cancel_job_record(pool, job).await;
        }
        if attempt > 1 {
            tracing::warn!(
                job_id,
                attempt,
                max_attempts,
                error = %last_error,
                "retrying beat proposal generation"
            );
            if wait_for_generation_retry(attempt, &token).await {
                return cancel_job_record(pool, job).await;
            }
        }

        let response = tokio::select! {
            () = token.cancelled() => {
                return cancel_job_record(pool, job).await;
            }
            result = chat_completion(
                &inference,
                &settings.model,
                &messages,
                0.7,
                settings.top_p,
                2048,
            ) => result,
        };

        match response {
            Ok(text) => {
                let text = display_generated_text(settings, &text);
                if text.trim().is_empty() {
                    last_error = "model returned no text".to_string();
                } else if let Some(beats) = parse_beats_proposal_json(&text) {
                    db::apply_beat_proposal(pool, story_id, chapter_id, &beats).await?;
                    db::complete_job(pool, job_id, JobStatus::Completed, None).await?;
                    return Ok(());
                } else {
                    last_error = "Failed to parse beat proposal JSON".to_string();
                }
            }
            Err(err) => last_error = err.to_string(),
        }

        if attempt == max_attempts {
            fail_job(pool, job_id, job, &last_error).await?;
        }
    }
    Ok(())
}

async fn run_story_chapter_outline(
    pool: &SqlitePool,
    job_id: i64,
    job: &dreamwell_types::Job,
    settings: &dreamwell_types::Settings,
    token: CancellationToken,
) -> AppResult<()> {
    let inference = db::get_inference_config(pool).await?;
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

    let max_attempts = generation_max_retries();
    let mut last_error = "generation failed".to_string();

    for attempt in 1..=max_attempts {
        if token.is_cancelled() {
            return cancel_job_record(pool, job).await;
        }
        if attempt > 1 {
            tracing::warn!(
                job_id,
                attempt,
                max_attempts,
                error = %last_error,
                "retrying chapter outline generation"
            );
            if wait_for_generation_retry(attempt, &token).await {
                return cancel_job_record(pool, job).await;
            }
        }

        let response = tokio::select! {
            () = token.cancelled() => {
                return cancel_job_record(pool, job).await;
            }
            result = chat_completion(
                &inference,
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
                if text.trim().is_empty() {
                    last_error = "model returned no text".to_string();
                } else if let Some((title, synopsis)) = parse_outline_json(&text) {
                    db::update_chapter_outline(pool, chapter_id, &title, &synopsis).await?;
                    db::touch_story(pool, story_id).await?;
                    db::complete_job(pool, job_id, JobStatus::Completed, None).await?;
                    return Ok(());
                } else {
                    last_error = "Failed to parse chapter outline JSON".to_string();
                }
            }
            Err(err) => last_error = err.to_string(),
        }

        if attempt == max_attempts {
            fail_job(pool, job_id, job, &last_error).await?;
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
    let inference = db::get_inference_config(pool).await?;
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

    let max_attempts = generation_max_retries();
    let mut last_error = "generation failed".to_string();

    for attempt in 1..=max_attempts {
        if token.is_cancelled() {
            return cancel_job_record(pool, job).await;
        }
        if attempt > 1 {
            tracing::warn!(
                job_id,
                attempt,
                max_attempts,
                error = %last_error,
                "retrying beat outline generation"
            );
            if wait_for_generation_retry(attempt, &token).await {
                return cancel_job_record(pool, job).await;
            }
        }

        let response = tokio::select! {
            () = token.cancelled() => {
                return cancel_job_record(pool, job).await;
            }
            result = chat_completion(
                &inference,
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
                if text.trim().is_empty() {
                    last_error = "model returned no text".to_string();
                } else if let Some((title, synopsis)) = parse_outline_json(&text) {
                    db::update_beat_outline(pool, beat_id, &title, &synopsis).await?;
                    db::touch_story(pool, story_id).await?;
                    db::complete_job(pool, job_id, JobStatus::Completed, None).await?;
                    return Ok(());
                } else {
                    last_error = "Failed to parse beat outline JSON".to_string();
                }
            }
            Err(err) => last_error = err.to_string(),
        }

        if attempt == max_attempts {
            fail_job(pool, job_id, job, &last_error).await?;
        }
    }
    Ok(())
}

async fn run_story_typed_beat_prose(
    pool: &SqlitePool,
    job_id: i64,
    job: &dreamwell_types::Job,
    settings: &dreamwell_types::Settings,
    token: &CancellationToken,
) -> AppResult<()> {
    let inference = db::get_inference_config(pool).await?;
    let story_id = job
        .story_id
        .ok_or_else(|| AppError::internal("missing story_id"))?;
    let chapter_id = job
        .chapter_id
        .ok_or_else(|| AppError::internal("missing chapter_id"))?;
    let beat_id = job
        .beat_id
        .ok_or_else(|| AppError::internal("missing beat_id"))?;

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

    let state_block = build_story_state_block(&detail.state, &detail.actors);
    let plan_messages = build_story_plan_messages(
        &detail.story,
        &detail.chapters,
        chapter,
        beat,
        &state_block,
        &job.guidance_notes,
    );

    let plan: dreamwell_types::PlanPhaseResponse = db::chat_completion_json_for_connection(
        pool,
        &inference,
        &settings.model,
        &plan_messages,
        0.4,
        settings.top_p,
        structured_output_tokens(settings),
        Some(&story_plan_schema()),
        generation_max_retries(),
        token,
    )
    .await?;

    if token.is_cancelled() {
        return cancel_job_record(pool, job).await;
    }

    let applied = apply_story_state_changes(
        pool,
        story_id,
        beat_id,
        &plan.state_changes,
        &detail.actors,
        &detail.state,
    )
    .await?;
    db::save_beat_plan(pool, beat_id, &plan.beats, &applied).await?;

    let state = db::list_story_state_entries(pool, story_id).await?;
    let actors = db::list_story_actors(pool, story_id).await?;
    let state_block = build_story_state_block(&state, &actors);
    let prose_messages = build_story_prose_from_plan_messages(
        &detail.story,
        chapter,
        beat,
        &plan.beats,
        &state_block,
        &job.guidance_notes,
        &actors,
    );

    let mut stream = stream_chat_completion(
        &inference,
        &settings.model,
        &prose_messages,
        settings.temperature,
        settings.top_p,
        settings.max_tokens,
    )
    .await?;

    let mut accumulated = String::new();
    let mut db_throttle = StreamDbThrottle::new();
    while let Some(token_result) = stream.next().await {
        if token.is_cancelled() {
            return cancel_job_record(pool, job).await;
        }
        match token_result {
            Ok(piece) => {
                accumulated.push_str(&piece);
                if db_throttle.ready() {
                    db::update_beat_content(pool, beat_id, &accumulated).await?;
                    db::touch_story(pool, story_id).await?;
                    db_throttle.mark_flushed();
                }
            }
            Err(err) => {
                db::complete_job(pool, job_id, JobStatus::Failed, Some(err.to_string())).await?;
                return Ok(());
            }
        }
    }

    if accumulated.trim().is_empty() {
        fail_job(pool, job_id, job, "model returned no text").await?;
        return Ok(());
    }

    db::update_beat_content(pool, beat_id, &accumulated).await?;
    db::complete_job(pool, job_id, JobStatus::Completed, None).await?;
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
    let continuing = job.job_type == JobType::StoryBeatProseContinue;

    if settings.variables_enabled && !continuing {
        return run_story_typed_beat_prose(pool, job_id, job, settings, &token).await;
    }

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

    let append_base = continuing.then(|| beat.content.clone());
    let existing_variable_updates = if continuing {
        beat.variable_updates.clone()
    } else {
        Vec::new()
    };

    let variables = if settings.variables_enabled {
        crate::story_variables::variables_for_beat_generation(
            pool,
            &detail.chapters,
            story_id,
            chapter.sort_order,
            beat.sort_order,
        )
        .await?
    } else {
        Vec::new()
    };

    let messages = if continuing {
        if settings.variables_enabled {
            let state_block = build_story_state_block(&detail.state, &detail.actors);
            build_beat_prose_continue_typed_messages(
                &detail.story,
                &detail.chapters,
                chapter,
                beat,
                &job.guidance_notes,
                &state_block,
            )
        } else {
            build_beat_prose_continue_messages(
                &detail.story,
                &detail.chapters,
                chapter,
                beat,
                &job.guidance_notes,
                &variables,
                false,
            )
        }
    } else {
        build_beat_prose_messages(
            &detail.story,
            &detail.chapters,
            chapter,
            beat,
            &job.guidance_notes,
            &variables,
            false,
        )
    };

    let max_attempts = generation_max_retries();
    let mut last_error = "model returned no text".to_string();

    for attempt in 1..=max_attempts {
        if token.is_cancelled() {
            return cancel_job_record(pool, job).await;
        }

        if attempt > 1 {
            tracing::warn!(
                job_id,
                attempt,
                max_attempts,
                error = %last_error,
                "retrying beat prose generation"
            );
            let restore = append_base.as_deref().unwrap_or("");
            db::update_beat_content(pool, beat_id, restore).await?;
            if wait_for_generation_retry(attempt, &token).await {
                return cancel_job_record(pool, job).await;
            }
        }

        match run_beat_prose_generation_attempt(
            pool,
            job_id,
            story_id,
            chapter.sort_order,
            beat_id,
            settings,
            &messages,
            append_base.as_deref(),
            &existing_variable_updates,
            &token,
        )
        .await?
        {
            BeatProseOutcome::Success => {
                db::complete_job(pool, job_id, JobStatus::Completed, None).await?;
                return Ok(());
            }
            BeatProseOutcome::Retryable(message) => {
                last_error = message;
                if attempt == max_attempts {
                    if let Some(base) = &append_base {
                        db::update_beat_content(pool, beat_id, base).await?;
                    }
                    fail_job(pool, job_id, job, &last_error).await?;
                    return Ok(());
                }
            }
            BeatProseOutcome::Failed => return Ok(()),
            BeatProseOutcome::Cancelled => return cancel_job_record(pool, job).await,
        }
    }

    Ok(())
}

#[allow(clippy::too_many_arguments)]
async fn run_beat_prose_generation_attempt(
    pool: &SqlitePool,
    job_id: i64,
    story_id: i64,
    chapter_order: i64,
    beat_id: i64,
    settings: &dreamwell_types::Settings,
    messages: &[serde_json::Value],
    append_base: Option<&str>,
    existing_variable_updates: &[dreamwell_types::BeatVariableUpdate],
    token: &CancellationToken,
) -> AppResult<BeatProseOutcome> {
    let inference = db::get_inference_config(pool).await?;
    let base = append_base.unwrap_or("");
    let mut stream = match stream_chat_completion(
        &inference,
        &settings.model,
        messages,
        settings.temperature,
        settings.top_p,
        settings.max_tokens,
    )
    .await
    {
        Ok(stream) => stream,
        Err(err) => return Ok(BeatProseOutcome::Retryable(err.to_string())),
    };

    let mut accumulated = String::new();
    let mut db_throttle = StreamDbThrottle::new();
    while let Some(token_result) = stream.next().await {
        if token.is_cancelled() {
            return Ok(BeatProseOutcome::Cancelled);
        }
        match token_result {
            Ok(piece) => {
                accumulated.push_str(&piece);
                if db_throttle.ready() {
                    let new_display = display_beat_prose(settings, &accumulated, true);
                    let display = if append_base.is_some() {
                        append_prose_continuation(base, &new_display)
                    } else {
                        new_display
                    };
                    db::update_beat_content(pool, beat_id, &display).await?;
                    db::touch_story(pool, story_id).await?;
                    db_throttle.mark_flushed();
                }
            }
            Err(err) => {
                if accumulated.is_empty() {
                    return Ok(BeatProseOutcome::Retryable(err.to_string()));
                }
                if let Some(base) = append_base {
                    db::update_beat_content(pool, beat_id, base).await?;
                }
                db::complete_job(pool, job_id, JobStatus::Failed, Some(err.to_string())).await?;
                return Ok(BeatProseOutcome::Failed);
            }
        }
    }

    if token.is_cancelled() {
        return Ok(BeatProseOutcome::Cancelled);
    }

    let new_display = display_beat_prose(settings, &accumulated, false);
    if new_display.trim().is_empty() {
        return Ok(BeatProseOutcome::Retryable(
            "model returned no text".to_string(),
        ));
    }
    let display = if append_base.is_some() {
        append_prose_continuation(base, &new_display)
    } else {
        new_display
    };

    if settings.variables_enabled {
        db::finalize_beat_prose(
            pool,
            story_id,
            chapter_order,
            beat_id,
            &display,
            existing_variable_updates,
        )
        .await?;
    } else {
        db::finalize_beat_prose(pool, story_id, chapter_order, beat_id, &display, &[]).await?;
    }
    db::touch_story(pool, story_id).await?;

    Ok(BeatProseOutcome::Success)
}

async fn run_story_chapter_summarize_handler(
    pool: &SqlitePool,
    job_id: i64,
    job: &dreamwell_types::Job,
    settings: &dreamwell_types::Settings,
    token: CancellationToken,
) -> AppResult<()> {
    if token.is_cancelled() {
        return cancel_job_record(pool, job).await;
    }
    let story_id = job
        .story_id
        .ok_or_else(|| AppError::internal("summarize job missing story_id"))?;
    let chapter_id = job
        .chapter_id
        .ok_or_else(|| AppError::internal("summarize job missing chapter_id"))?;
    match crate::story_summarize::run_story_chapter_summarize_job(
        pool, job_id, story_id, chapter_id, settings,
    )
    .await
    {
        Ok(()) => Ok(()),
        Err(err) => fail_job(pool, job_id, job, &err.to_string()).await,
    }
}

async fn run_story_beat_variable_recheck_handler(
    pool: &SqlitePool,
    job_id: i64,
    job: &dreamwell_types::Job,
    settings: &dreamwell_types::Settings,
    token: CancellationToken,
) -> AppResult<()> {
    if token.is_cancelled() {
        return cancel_job_record(pool, job).await;
    }
    let story_id = job
        .story_id
        .ok_or_else(|| AppError::internal("recheck job missing story_id"))?;
    let beat_id = job
        .beat_id
        .ok_or_else(|| AppError::internal("recheck job missing beat_id"))?;
    match crate::state_recheck::run_story_state_recheck_job(
        pool, job_id, story_id, beat_id, settings,
    )
    .await
    {
        Ok(()) => Ok(()),
        Err(err) => fail_job(pool, job_id, job, &err.to_string()).await,
    }
}

async fn run_story_beat_mechanical_handler(
    pool: &SqlitePool,
    job_id: i64,
    job: &dreamwell_types::Job,
    settings: &dreamwell_types::Settings,
    token: CancellationToken,
) -> AppResult<()> {
    if token.is_cancelled() {
        return cancel_job_record(pool, job).await;
    }
    let story_id = job
        .story_id
        .ok_or_else(|| AppError::internal("mechanical job missing story_id"))?;
    let chapter_id = job
        .chapter_id
        .ok_or_else(|| AppError::internal("mechanical job missing chapter_id"))?;
    let beat_id = job
        .beat_id
        .ok_or_else(|| AppError::internal("mechanical job missing beat_id"))?;
    match crate::story_beat_mechanical::run_story_beat_mechanical_job(
        pool,
        job_id,
        story_id,
        chapter_id,
        beat_id,
        &job.guidance_notes,
        settings,
    )
    .await
    {
        Ok(()) => Ok(()),
        Err(err) => fail_job(pool, job_id, job, &err.to_string()).await,
    }
}

async fn run_story_beat_prose_recheck_handler(
    pool: &SqlitePool,
    job_id: i64,
    job: &dreamwell_types::Job,
    settings: &dreamwell_types::Settings,
    token: CancellationToken,
) -> AppResult<()> {
    if token.is_cancelled() {
        return cancel_job_record(pool, job).await;
    }
    let story_id = job
        .story_id
        .ok_or_else(|| AppError::internal("prose recheck job missing story_id"))?;
    let chapter_id = job
        .chapter_id
        .ok_or_else(|| AppError::internal("prose recheck job missing chapter_id"))?;
    let beat_id = job
        .beat_id
        .ok_or_else(|| AppError::internal("prose recheck job missing beat_id"))?;
    let detail = db::get_story_detail(pool, story_id).await?;
    let chapter = detail
        .chapters
        .iter()
        .find(|c| c.id == chapter_id)
        .ok_or_else(|| AppError::internal("chapter not found"))?;
    match crate::story_beat_prose_recheck::run_beat_prose_recheck_job(
        pool,
        job_id,
        story_id,
        chapter_id,
        beat_id,
        chapter.sort_order,
        &job.guidance_notes,
        settings,
    )
    .await
    {
        Ok(()) => Ok(()),
        Err(err) => fail_job(pool, job_id, job, &err.to_string()).await,
    }
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

pub async fn enqueue_game_generation(
    queue: &JobQueue,
    job: dreamwell_types::Job,
) -> AppResult<dreamwell_types::Job> {
    queue.wake();
    Ok(job)
}

#[cfg(test)]
mod tests {
    use super::append_prose_continuation;

    #[test]
    fn append_prose_continuation_inserts_single_blank_line() {
        assert_eq!(
            append_prose_continuation("She walked in.", "She bought bread."),
            "She walked in.\n\nShe bought bread."
        );
    }

    #[test]
    fn append_prose_continuation_normalizes_trailing_and_leading_whitespace() {
        assert_eq!(
            append_prose_continuation("She walked in.\n\n\n", "\n\nShe bought bread."),
            "She walked in.\n\nShe bought bread."
        );
    }

    #[test]
    fn append_prose_continuation_handles_empty_continuation() {
        assert_eq!(
            append_prose_continuation("She walked in.", ""),
            "She walked in.\n\n"
        );
        assert_eq!(
            append_prose_continuation("", "She bought bread."),
            "She bought bread."
        );
    }
}
