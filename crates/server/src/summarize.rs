use chrono::{DateTime, Duration, Utc};
use dreamwell_types::{
    estimate_token_count, prompt_token_budget, Character, Job, Message, MessageRole, Settings,
};
use serde_json::json;
use sqlx::SqlitePool;
use tokio::sync::mpsc;

use crate::config;
use crate::db;
use crate::error::{AppError, AppResult};
use crate::model_fallback::{chat_completion_with_connection_fallback, has_inference_provider};
use crate::prompts::estimate_static_prompt_tokens;
use crate::thoughts::parse_thought_blocks;

const SUMMARIZE_PLACEHOLDER: &str = "Summarizing earlier messages…";
pub const SUMMARIZE_FORCE_MARKER: &str = "force";
pub const SUMMARIZE_REGENERATE_MARKER: &str = "regenerate";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SummarizeJobKind {
    Auto,
    Force,
    Regenerate,
}

pub fn summarize_job_kind(guidance_notes: &str) -> SummarizeJobKind {
    if guidance_notes == SUMMARIZE_FORCE_MARKER {
        SummarizeJobKind::Force
    } else if guidance_notes == SUMMARIZE_REGENERATE_MARKER {
        SummarizeJobKind::Regenerate
    } else {
        SummarizeJobKind::Auto
    }
}

pub fn is_active_for_context(message: &Message) -> bool {
    !message.is_summary && message.role != MessageRole::System && !message.in_summary
}

fn is_stale_summary_marker(content: &str) -> bool {
    content.starts_with("[Summarization failed:")
        || content.starts_with("[Summarization cancelled]")
}

pub async fn cleanup_stale_summary_markers(pool: &SqlitePool, chat_id: i64) -> AppResult<()> {
    let messages = db::list_messages(pool, chat_id).await?;
    let stale_ids: Vec<i64> = messages
        .iter()
        .filter(|message| message.is_summary && is_stale_summary_marker(&message.content))
        .map(|message| message.id)
        .collect();
    if !stale_ids.is_empty() {
        db::delete_messages(pool, &stale_ids).await?;
    }
    Ok(())
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SummarizePlan {
    pub to_summarize_ids: Vec<i64>,
    pub anchor_before: DateTime<Utc>,
    pub summarized_count: usize,
}

pub fn plan_summarization(
    settings: &Settings,
    chat_summary: &str,
    messages: &[Message],
    character: Option<&Character>,
    force: bool,
) -> Option<SummarizePlan> {
    if !force && !settings.summarize_enabled {
        return None;
    }

    let non_system: Vec<&Message> = messages
        .iter()
        .filter(|m| is_active_for_context(m))
        .collect();

    let min_keep = settings.summarize_keep_recent.max(2) as usize;
    let min_total = if force {
        min_keep
    } else {
        settings.summarize_after_messages.max(4) as usize
    };
    if non_system.len() <= min_total {
        return None;
    }

    let keep = if settings.summarize_adaptive && settings.context_tokens > 0 {
        adaptive_keep_count(settings, chat_summary, &non_system, character, min_keep)
    } else {
        settings.summarize_keep_recent.max(2) as usize
    };

    let split_at = non_system.len().saturating_sub(keep);
    if split_at == 0 {
        return None;
    }

    if !force
        && (!settings.summarize_adaptive || settings.context_tokens <= 0)
        && (non_system.len() as i64 <= settings.summarize_after_messages)
    {
        return None;
    }

    let to_summarize: Vec<i64> = non_system[..split_at].iter().map(|m| m.id).collect();
    let anchor_before = non_system[split_at].created_at;

    Some(SummarizePlan {
        to_summarize_ids: to_summarize,
        anchor_before,
        summarized_count: split_at,
    })
}

fn adaptive_keep_count(
    settings: &Settings,
    chat_summary: &str,
    messages: &[&Message],
    character: Option<&Character>,
    min_keep: usize,
) -> usize {
    let prompt_budget = prompt_token_budget(settings.context_tokens, settings.max_tokens);
    if prompt_budget <= 0 {
        return min_keep;
    }

    let reserved_system = (prompt_budget / 4).clamp(256, 1024);
    let reserved_summary = (prompt_budget / 8).clamp(128, 512);
    let static_tokens = estimate_static_prompt_tokens(settings, character);
    let summary_tokens = estimate_token_count(chat_summary);
    let available_history =
        (prompt_budget - reserved_system - reserved_summary - static_tokens - summary_tokens)
            .max(0);

    let mut keep = 0usize;
    let mut used = 0i64;
    for msg in messages.iter().rev() {
        let tokens =
            estimate_token_count(&msg.content) + estimate_token_count(&msg.thought_content);
        if keep < min_keep || used + tokens <= available_history {
            used += tokens;
            keep += 1;
        } else {
            break;
        }
    }
    keep.max(min_keep).min(messages.len())
}

const SUMMARIZE_SYSTEM_PROMPT: &str =
    "Summarize the following roleplay conversation. Preserve key plot points, relationships, character voice, and established facts. Be concise but complete. Output only the summary text. Do not use thinking, reasoning, or thought tags.";
const REGENERATE_SYSTEM_PROMPT: &str =
    "Regenerate the conversation summary using the previous summary and the messages still in the chat. Preserve key plot points, relationships, character voice, and established facts. Be concise but complete. Output only the summary text. Do not use thinking, reasoning, or thought tags.";
const SUMMARIZE_SYSTEM_OVERHEAD_TOKENS: i64 = 80;
const SUMMARIZE_USER_WRAPPER_TOKENS: i64 = 48;
const SUMMARIZE_INPUT_BUDGET_FLOOR: i64 = 256;
const SUMMARIZE_INPUT_BUDGET_FALLBACK: i64 = 2048;

pub fn summary_output_tokens(settings: &Settings) -> i64 {
    if settings.context_tokens > 0 {
        (settings.context_tokens / 8).clamp(256, 1024).max(256)
    } else {
        512
    }
}

pub fn summarize_input_token_budget(settings: &Settings, previous_summary: &str) -> i64 {
    let output = summary_output_tokens(settings);
    let prompt_budget = if settings.context_tokens > 0 {
        prompt_token_budget(settings.context_tokens, output)
    } else {
        SUMMARIZE_INPUT_BUDGET_FALLBACK + output
    };
    let summary_tokens = estimate_token_count(previous_summary);
    (prompt_budget
        - SUMMARIZE_SYSTEM_OVERHEAD_TOKENS
        - SUMMARIZE_USER_WRAPPER_TOKENS
        - summary_tokens)
        .max(SUMMARIZE_INPUT_BUDGET_FLOOR)
}

fn message_token_count(message: &Message) -> i64 {
    estimate_token_count(&message.content) + estimate_token_count(&message.thought_content)
}

fn format_transcript_line(message: &Message) -> String {
    let role = match message.role {
        MessageRole::User => "user",
        MessageRole::Assistant => "assistant",
        MessageRole::System => "system",
    };
    format!("{role}: {}", message.content)
}

fn build_transcript(messages: &[&Message]) -> String {
    messages
        .iter()
        .map(|message| format_transcript_line(message))
        .collect::<Vec<_>>()
        .join("\n")
}

/// Oldest messages first, capped to fit the summarize inference input budget.
pub fn select_oldest_batch_for_budget(
    candidates: &[&Message],
    previous_summary: &str,
    settings: &Settings,
) -> Vec<i64> {
    if candidates.is_empty() {
        return Vec::new();
    }

    let budget = summarize_input_token_budget(settings, previous_summary);
    let mut used = 0i64;
    let mut batch = Vec::new();
    for message in candidates {
        let tokens = message_token_count(message);
        if !batch.is_empty() && used + tokens > budget {
            break;
        }
        used += tokens;
        batch.push(message.id);
    }

    if batch.is_empty() {
        batch.push(candidates[0].id);
    }
    batch
}

/// Newest messages first when trimming regenerate input — older context lives in the summary.
pub fn select_newest_batch_for_budget<'a>(
    candidates: &[&'a Message],
    previous_summary: &str,
    settings: &Settings,
) -> Vec<&'a Message> {
    if candidates.is_empty() {
        return Vec::new();
    }

    let budget = summarize_input_token_budget(settings, previous_summary);
    let mut used = 0i64;
    let mut batch = Vec::new();
    for message in candidates.iter().rev() {
        let tokens = message_token_count(message);
        if !batch.is_empty() && used + tokens > budget {
            break;
        }
        used += tokens;
        batch.push(*message);
    }

    if batch.is_empty() {
        batch.push(candidates[candidates.len() - 1]);
    }
    batch.reverse();
    batch
}

fn build_summarize_prompt(previous_summary: &str, transcript: &str) -> Vec<serde_json::Value> {
    vec![
        json!({
            "role": "system",
            "content": SUMMARIZE_SYSTEM_PROMPT,
        }),
        json!({
            "role": "user",
            "content": format!(
                "Previous summary:\n{}\n\nNew messages to incorporate:\n{transcript}",
                if previous_summary.is_empty() {
                    "(none)"
                } else {
                    previous_summary
                }
            ),
        }),
    ]
}

fn build_regenerate_prompt(previous_summary: &str, transcript: &str) -> Vec<serde_json::Value> {
    vec![
        json!({
            "role": "system",
            "content": REGENERATE_SYSTEM_PROMPT,
        }),
        json!({
            "role": "user",
            "content": format!(
                "Previous summary:\n{previous_summary}\n\nMessages currently in the chat:\n{}",
                if transcript.is_empty() {
                    "(none)".to_string()
                } else {
                    transcript.to_string()
                }
            ),
        }),
    ]
}

fn summarize_max_retries() -> u32 {
    config::GENERATION_MAX_RETRIES
        .load(std::sync::atomic::Ordering::SeqCst)
        .max(1)
}

/// Strips reasoning blocks from summarize output and validates a visible summary body.
pub(crate) fn process_summarize_output(raw: &str) -> Result<String, &'static str> {
    let parsed = parse_thought_blocks(raw);
    if !parsed.thought_complete {
        return Err("summarization ended inside a thought block");
    }
    let summary = parsed.reply.trim().to_string();
    if summary.is_empty() {
        return Err("summarization returned no visible text");
    }
    Ok(summary)
}

async fn call_summarize_model_once(
    pool: &SqlitePool,
    settings: &Settings,
    prompt: &[serde_json::Value],
    job_id: Option<i64>,
) -> AppResult<String> {
    let raw =
        chat_completion_with_connection_fallback(pool, settings, prompt, job_id, None, None, None)
            .await?;
    process_summarize_output(&raw).map_err(AppError::inference)
}

async fn call_summarize_model(
    pool: &SqlitePool,
    settings: &Settings,
    prompt: &[serde_json::Value],
    job_id: Option<i64>,
) -> AppResult<String> {
    let max_attempts = summarize_max_retries();
    let mut last_error = "summarization failed".to_string();

    for attempt in 1..=max_attempts {
        match call_summarize_model_once(pool, settings, prompt, job_id).await {
            Ok(summary) => return Ok(summary),
            Err(err) => {
                last_error = err.to_string();
                if attempt == max_attempts {
                    return Err(err);
                }
                tracing::warn!(
                    attempt,
                    max_attempts,
                    error = %last_error,
                    "retrying summarization"
                );
            }
        }
    }

    Err(AppError::inference(last_error))
}

pub async fn maybe_enqueue_summarize(
    pool: &SqlitePool,
    work_tx: &mpsc::UnboundedSender<()>,
    chat_id: i64,
    settings: &Settings,
) -> AppResult<()> {
    let _ = try_enqueue_summarize(pool, work_tx, chat_id, settings, false).await?;
    Ok(())
}

pub async fn delete_chat_summary(pool: &SqlitePool, chat_id: i64) -> AppResult<()> {
    let _ = db::get_chat(pool, chat_id).await?;
    db::update_chat_summary(pool, chat_id, "").await?;
    db::clear_messages_in_summary(pool, chat_id).await?;
    let messages = db::list_messages(pool, chat_id).await?;
    let marker_ids: Vec<i64> = messages
        .iter()
        .filter(|message| message.is_summary)
        .map(|message| message.id)
        .collect();
    if !marker_ids.is_empty() {
        db::delete_messages(pool, &marker_ids).await?;
    }
    db::touch_chat(pool, chat_id).await?;
    Ok(())
}

pub async fn enqueue_regenerate_summary_for_chat(
    pool: &SqlitePool,
    work_tx: &mpsc::UnboundedSender<()>,
    chat_id: i64,
    marker_id: i64,
    settings: &Settings,
) -> AppResult<Job> {
    if !has_inference_provider(settings) {
        return Err(crate::error::AppError::bad_request(
            "Configure an inference model in Settings before regenerating a summary",
        ));
    }
    if db::has_active_summarize_job(pool, chat_id).await? {
        return Err(crate::error::AppError::bad_request(
            "A summarization job is already in progress for this chat",
        ));
    }

    cleanup_stale_summary_markers(pool, chat_id).await?;

    let chat = db::get_chat(pool, chat_id).await?;
    if chat.summary.trim().is_empty() {
        return Err(crate::error::AppError::bad_request(
            "This chat has no summary to regenerate",
        ));
    }

    let message = db::get_message(pool, chat_id, marker_id).await?;
    if !message.is_summary {
        return Err(crate::error::AppError::bad_request(
            "Only summary markers can be regenerated",
        ));
    }

    db::update_message_content(pool, marker_id, SUMMARIZE_PLACEHOLDER).await?;
    let job =
        db::enqueue_summarize_job(pool, chat_id, marker_id, SUMMARIZE_REGENERATE_MARKER).await?;
    let _ = work_tx.send(());
    Ok(job)
}

pub async fn enqueue_summarize_for_chat(
    pool: &SqlitePool,
    work_tx: &mpsc::UnboundedSender<()>,
    chat_id: i64,
    settings: &Settings,
) -> AppResult<Job> {
    if !has_inference_provider(settings) {
        return Err(crate::error::AppError::bad_request(
            "Configure an inference model in Settings before summarizing",
        ));
    }
    if db::has_active_summarize_job(pool, chat_id).await? {
        return Err(crate::error::AppError::bad_request(
            "A summarization job is already in progress for this chat",
        ));
    }

    try_enqueue_summarize(pool, work_tx, chat_id, settings, true)
        .await?
        .ok_or_else(|| {
            crate::error::AppError::bad_request(
                "Not enough messages to summarize — keep at least a few recent messages",
            )
        })
}

async fn try_enqueue_summarize(
    pool: &SqlitePool,
    work_tx: &mpsc::UnboundedSender<()>,
    chat_id: i64,
    settings: &Settings,
    force: bool,
) -> AppResult<Option<Job>> {
    if !force && (!settings.summarize_enabled || !has_inference_provider(settings)) {
        return Ok(None);
    }
    if db::has_active_summarize_job(pool, chat_id).await? {
        return Ok(None);
    }

    cleanup_stale_summary_markers(pool, chat_id).await?;

    let chat = db::get_chat(pool, chat_id).await?;
    let character = db::get_character(pool, chat.character_id).await.ok();
    let messages = db::list_messages(pool, chat_id).await?;
    let Some(plan) = plan_summarization(
        settings,
        &chat.summary,
        &messages,
        character.as_ref(),
        force,
    ) else {
        return Ok(None);
    };

    let marker = db::insert_message(
        pool,
        chat_id,
        MessageRole::System,
        SUMMARIZE_PLACEHOLDER.to_string(),
        true,
    )
    .await?;

    let anchor = (plan.anchor_before - Duration::milliseconds(1)).to_rfc3339();
    db::set_message_created_at(pool, marker.id, &anchor).await?;
    let guidance_notes = if force { SUMMARIZE_FORCE_MARKER } else { "" };
    let job = db::enqueue_summarize_job(pool, chat_id, marker.id, guidance_notes).await?;
    let _ = work_tx.send(());
    Ok(Some(job))
}

/// Result of the internal multi-pass summarize loop (one queued job, zero or more inference passes).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SummarizeLoopProgress {
    pub passes_completed: usize,
    pub total_summarized: usize,
    pub current_summary: String,
    pub anchor_before: Option<DateTime<Utc>>,
}

/// Pure simulation of batch selection across every pass in a single summarize job.
#[cfg(test)]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChunkedSummarizeSimulation {
    pub passes: Vec<Vec<i64>>,
    pub total_messages: usize,
}

#[cfg(test)]
pub fn simulate_chunked_summarize_passes(
    settings: &Settings,
    chat_summary: &str,
    messages: &[Message],
    character: Option<&Character>,
    force: bool,
) -> ChunkedSummarizeSimulation {
    let mut current_summary = chat_summary.to_string();
    let mut remaining: Vec<Message> = messages.to_vec();
    let mut passes = Vec::new();
    let mut total_messages = 0usize;

    loop {
        let Some(plan) =
            plan_summarization(settings, &current_summary, &remaining, character, force)
        else {
            break;
        };

        let candidates: Vec<&Message> = remaining
            .iter()
            .filter(|message| plan.to_summarize_ids.contains(&message.id))
            .collect();
        let batch_ids = select_oldest_batch_for_budget(&candidates, &current_summary, settings);
        if batch_ids.is_empty() {
            break;
        }

        passes.push(batch_ids.clone());
        total_messages += batch_ids.len();
        current_summary.push_str("|pass");
        for message in remaining.iter_mut() {
            if batch_ids.contains(&message.id) {
                message.in_summary = true;
            }
        }
    }

    ChunkedSummarizeSimulation {
        passes,
        total_messages,
    }
}

pub(crate) async fn execute_summarize_loop<S, Fut>(
    pool: &SqlitePool,
    chat_id: i64,
    settings: &Settings,
    force: bool,
    mut summarize: S,
) -> AppResult<SummarizeLoopProgress>
where
    S: FnMut(Vec<serde_json::Value>) -> Fut,
    Fut: std::future::Future<Output = AppResult<String>>,
{
    let chat = db::get_chat(pool, chat_id).await?;
    let character = db::get_character(pool, chat.character_id).await.ok();

    let mut current_summary = chat.summary.clone();
    let mut total_summarized = 0usize;
    let mut passes_completed = 0usize;
    let mut anchor_before = None::<DateTime<Utc>>;

    loop {
        let messages = db::list_messages(pool, chat_id).await?;
        let Some(plan) = plan_summarization(
            settings,
            &current_summary,
            &messages,
            character.as_ref(),
            force,
        ) else {
            break;
        };

        if anchor_before.is_none() {
            anchor_before = Some(plan.anchor_before);
        }

        let candidates: Vec<&Message> = messages
            .iter()
            .filter(|message| plan.to_summarize_ids.contains(&message.id))
            .collect();
        let batch_ids = select_oldest_batch_for_budget(&candidates, &current_summary, settings);
        let batch: Vec<&Message> = candidates
            .iter()
            .filter(|message| batch_ids.contains(&message.id))
            .copied()
            .collect();

        let transcript = build_transcript(&batch);
        let prompt = build_summarize_prompt(&current_summary, &transcript);
        current_summary = summarize(prompt).await?;

        db::update_chat_summary(pool, chat_id, &current_summary).await?;
        db::mark_messages_in_summary(pool, &batch_ids).await?;
        total_summarized += batch_ids.len();
        passes_completed += 1;
    }

    if passes_completed > 0 {
        db::touch_chat(pool, chat_id).await?;
    }

    Ok(SummarizeLoopProgress {
        passes_completed,
        total_summarized,
        current_summary,
        anchor_before,
    })
}

pub async fn run_summarize_job(
    pool: &SqlitePool,
    job_id: i64,
    chat_id: i64,
    marker_id: i64,
    settings: &Settings,
    kind: SummarizeJobKind,
) -> AppResult<()> {
    if kind == SummarizeJobKind::Regenerate {
        return run_regenerate_summary_job(pool, job_id, chat_id, marker_id, settings).await;
    }

    let force = kind == SummarizeJobKind::Force;
    let settings_for_loop = settings.clone();
    let pool_for_loop = pool.clone();
    let progress = execute_summarize_loop(pool, chat_id, settings, force, move |prompt| {
        let settings = settings_for_loop.clone();
        let pool = pool_for_loop.clone();
        let job_id = job_id;
        async move { call_summarize_model(&pool, &settings, &prompt, Some(job_id)).await }
    })
    .await?;

    if progress.total_summarized == 0 {
        db::delete_messages(pool, &[marker_id]).await?;
        db::complete_job(pool, job_id, dreamwell_types::JobStatus::Completed, None).await?;
        return Ok(());
    }

    let marker_body = format_summary_marker(progress.total_summarized, &progress.current_summary);
    db::update_message_content(pool, marker_id, &marker_body).await?;
    if let Some(anchor_before) = progress.anchor_before {
        let anchor = (anchor_before - Duration::milliseconds(1)).to_rfc3339();
        db::set_message_created_at(pool, marker_id, &anchor).await?;
    }
    db::complete_job(pool, job_id, dreamwell_types::JobStatus::Completed, None).await?;
    Ok(())
}

async fn run_regenerate_summary_job(
    pool: &SqlitePool,
    job_id: i64,
    chat_id: i64,
    marker_id: i64,
    settings: &Settings,
) -> AppResult<()> {
    let chat = db::get_chat(pool, chat_id).await?;
    if chat.summary.trim().is_empty() {
        db::delete_messages(pool, &[marker_id]).await?;
        db::complete_job(pool, job_id, dreamwell_types::JobStatus::Completed, None).await?;
        return Ok(());
    }

    let messages = db::list_messages(pool, chat_id).await?;
    let marker_count = messages
        .iter()
        .find(|message| message.id == marker_id)
        .and_then(|message| parse_summary_marker_count(&message.content))
        .unwrap_or(0);

    let candidates: Vec<&Message> = messages
        .iter()
        .filter(|message| is_active_for_context(message))
        .collect();
    let batch = select_newest_batch_for_budget(&candidates, &chat.summary, settings);
    let transcript = build_transcript(&batch);
    let prompt = build_regenerate_prompt(&chat.summary, &transcript);
    let summary = call_summarize_model(pool, settings, &prompt, Some(job_id)).await?;

    db::update_chat_summary(pool, chat_id, &summary).await?;
    let count = marker_count.max(1);
    let marker_body = format_summary_marker(count, &summary);
    db::update_message_content(pool, marker_id, &marker_body).await?;
    refresh_chat_summary_markers(pool, chat_id, &summary).await?;
    db::touch_chat(pool, chat_id).await?;
    db::complete_job(pool, job_id, dreamwell_types::JobStatus::Completed, None).await?;
    Ok(())
}

pub fn format_summary_marker(summarized_count: usize, summary: &str) -> String {
    let preview = if summary.len() > 600 {
        format!("{}…", &summary[..600])
    } else {
        summary.to_string()
    };
    format!(
        "**Earlier conversation summarized** ({summarized_count} messages)\n\n{preview}\n\n_The full summary is included in the model's context for future replies. Earlier messages remain visible in the chat._"
    )
}

pub fn parse_summary_marker_count(body: &str) -> Option<usize> {
    let open = body.find('(')?;
    let close = body[open..].find(" messages)")?;
    body[open + 1..open + close].trim().parse().ok()
}

pub async fn refresh_chat_summary_markers(
    pool: &SqlitePool,
    chat_id: i64,
    summary: &str,
) -> AppResult<()> {
    let messages = db::list_messages(pool, chat_id).await?;
    for message in messages {
        if !message.is_summary
            || message.content == SUMMARIZE_PLACEHOLDER
            || !message
                .content
                .starts_with("**Earlier conversation summarized**")
        {
            continue;
        }
        let Some(count) = parse_summary_marker_count(&message.content) else {
            continue;
        };
        let body = format_summary_marker(count, summary);
        db::update_message_content(pool, message.id, &body).await?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;
    use dreamwell_types::{InferenceConnection, JsonFormatStrategy, Settings};

    fn msg(id: i64, content: &str) -> Message {
        Message {
            id,
            chat_id: 1,
            role: MessageRole::User,
            content: content.to_string(),
            thought_content: String::new(),
            thought_duration_ms: None,
            thought_in_progress: false,
            variable_updates: Vec::new(),
            reply_beats: Vec::new(),
            state_changes: Vec::new(),
            generation_phase: String::new(),
            is_summary: false,
            in_summary: false,
            created_at: Utc::now(),
            job_status: None,
            generation_error: None,
            generation_notice: String::new(),
        }
    }

    fn test_settings() -> Settings {
        Settings {
            inference_url: "http://localhost:8080/v1".into(),
            active_connection_id: Some(1),
            connections: vec![InferenceConnection {
                id: 1,
                name: "Test".into(),
                inference_url: "http://localhost:8080/v1".into(),
                api_key_set: false,
                model: "m".into(),
                enabled: true,
                sort_order: 0,
                json_format_strategy: JsonFormatStrategy::Auto,
                tool_call_parser: "auto".into(),
                temperature: 0.8,
                top_p: 0.9,
                max_tokens: 512,
                context_tokens: 4096,
                max_context_messages: 40,
                auto_context_on_model_change: true,
            }],
            model: "m".into(),
            temperature: 0.8,
            top_p: 0.9,
            max_tokens: 512,
            system_prompt_prefix: String::new(),
            system_prompt_suffix: String::new(),
            user_name: "User".into(),
            persona_description: String::new(),
            summarize_enabled: true,
            summarize_adaptive: true,
            summarize_after_messages: 4,
            summarize_keep_recent: 2,
            variables_enabled: true,
            thought_blocks_enabled: true,
            max_context_messages: 40,
            context_tokens: 4096,
            auto_context_on_model_change: true,
            max_concurrent_jobs: 1,
            model_profiles: Vec::new(),
            chat_model_plan: String::new(),
            chat_model_prose: String::new(),
            chat_temperature_plan: None,
            chat_top_p_plan: None,
            chat_temperature_prose: None,
            chat_top_p_prose: None,
        }
    }

    #[test]
    fn adaptive_plan_trims_when_history_exceeds_budget() {
        let settings = test_settings();
        let messages: Vec<Message> = (0..12).map(|i| msg(i, &"word ".repeat(200))).collect();
        let plan = plan_summarization(&settings, "", &messages, None, false);
        assert!(plan.is_some());
        let plan = plan.unwrap();
        assert!(plan.summarized_count > 0);
        assert!(plan.summarized_count < messages.len());
    }

    #[test]
    fn legacy_plan_respects_message_threshold() {
        let mut settings = test_settings();
        settings.summarize_adaptive = false;
        settings.summarize_after_messages = 20;
        settings.summarize_keep_recent = 8;
        let messages: Vec<Message> = (0..10).map(|i| msg(i, "hi")).collect();
        assert!(plan_summarization(&settings, "", &messages, None, false).is_none());
        let messages: Vec<Message> = (0..25).map(|i| msg(i, "hi")).collect();
        let plan = plan_summarization(&settings, "", &messages, None, false).unwrap();
        assert_eq!(plan.summarized_count, 17);
    }

    #[test]
    fn forced_plan_bypasses_auto_thresholds() {
        let mut settings = test_settings();
        settings.summarize_enabled = false;
        settings.summarize_adaptive = false;
        settings.summarize_after_messages = 20;
        settings.summarize_keep_recent = 2;
        let messages: Vec<Message> = (0..10).map(|i| msg(i, "hi")).collect();
        assert!(plan_summarization(&settings, "", &messages, None, false).is_none());
        let plan = plan_summarization(&settings, "", &messages, None, true).unwrap();
        assert_eq!(plan.summarized_count, 8);
    }

    #[test]
    fn format_summary_marker_includes_count() {
        let body = format_summary_marker(5, "They met at the tavern.");
        assert!(body.contains("5 messages"));
        assert!(body.contains("tavern"));
    }

    #[test]
    fn select_oldest_batch_respects_input_budget() {
        let settings = test_settings();
        let messages: Vec<Message> = (0..10).map(|i| msg(i, &"word ".repeat(500))).collect();
        let refs: Vec<&Message> = messages.iter().collect();
        let batch = select_oldest_batch_for_budget(&refs, "", &settings);
        assert!(!batch.is_empty());
        assert!(batch.len() < messages.len());
        assert_eq!(batch[0], 0);
    }

    #[test]
    fn select_oldest_batch_includes_at_least_one_oversized_message() {
        let settings = test_settings();
        let messages = [msg(1, &"word ".repeat(20_000))];
        let refs: Vec<&Message> = messages.iter().collect();
        let batch = select_oldest_batch_for_budget(&refs, "", &settings);
        assert_eq!(batch, [1]);
    }

    #[test]
    fn select_newest_batch_prefers_recent_messages() {
        let settings = test_settings();
        let messages: Vec<Message> = (0..10).map(|i| msg(i, &"word ".repeat(500))).collect();
        let refs: Vec<&Message> = messages.iter().collect();
        let batch = select_newest_batch_for_budget(&refs, "", &settings);
        assert!(!batch.is_empty());
        assert!(batch.len() < messages.len());
        assert_eq!(batch.last().map(|m| m.id), Some(9));
    }

    #[test]
    fn summarize_input_budget_reserves_summary_text() {
        let settings = test_settings();
        let empty = summarize_input_token_budget(&settings, "");
        let with_summary = summarize_input_token_budget(&settings, &"word ".repeat(4000));
        assert!(with_summary < empty);
        assert!(with_summary >= SUMMARIZE_INPUT_BUDGET_FLOOR);
    }

    #[test]
    fn summarize_job_kind_reads_guidance_notes() {
        assert_eq!(
            summarize_job_kind(SUMMARIZE_FORCE_MARKER),
            SummarizeJobKind::Force
        );
        assert_eq!(
            summarize_job_kind(SUMMARIZE_REGENERATE_MARKER),
            SummarizeJobKind::Regenerate
        );
        assert_eq!(summarize_job_kind(""), SummarizeJobKind::Auto);
    }

    #[test]
    fn process_summarize_output_strips_thought_and_keeps_body() {
        let raw = "<thinking>planning</thinking>\nThey met at the tavern.";
        assert_eq!(
            process_summarize_output(raw).expect("summary"),
            "They met at the tavern."
        );
    }

    #[test]
    fn process_summarize_output_strips_gemma_thought_channel() {
        let raw = "<|channel>thought\nnotes<channel|>Plot twist at the inn.";
        assert_eq!(
            process_summarize_output(raw).expect("summary"),
            "Plot twist at the inn."
        );
    }

    #[test]
    fn process_summarize_output_rejects_unclosed_thought() {
        assert_eq!(
            process_summarize_output("Reply <thinking>still planning").unwrap_err(),
            "summarization ended inside a thought block"
        );
    }

    #[test]
    fn process_summarize_output_rejects_thought_only() {
        assert_eq!(
            process_summarize_output("<thinking>notes only</thinking>").unwrap_err(),
            "summarization returned no visible text"
        );
    }

    #[test]
    fn process_summarize_output_accepts_plain_text() {
        let raw = "They traveled north and found shelter.";
        assert_eq!(
            process_summarize_output(raw).expect("summary"),
            "They traveled north and found shelter."
        );
    }

    #[test]
    fn stale_summary_marker_detects_error_and_cancelled_content() {
        assert!(is_stale_summary_marker(
            "[Summarization failed: connection refused]"
        ));
        assert!(is_stale_summary_marker("[Summarization cancelled]"));
        assert!(!is_stale_summary_marker(SUMMARIZE_PLACEHOLDER));
        assert!(!is_stale_summary_marker(&format_summary_marker(
            3,
            "They met at the tavern."
        )));
    }

    #[test]
    fn parse_summary_marker_count_reads_message_count() {
        let body = format_summary_marker(12, "Plot twist.");
        assert_eq!(parse_summary_marker_count(&body), Some(12));
        assert_eq!(parse_summary_marker_count("not a marker"), None);
    }

    fn chunked_test_settings() -> Settings {
        let mut settings = test_settings();
        settings.summarize_adaptive = false;
        settings.summarize_keep_recent = 2;
        settings
    }

    #[test]
    fn is_active_for_context_excludes_folded_messages() {
        let mut message = msg(1, "hello");
        assert!(is_active_for_context(&message));
        message.in_summary = true;
        assert!(!is_active_for_context(&message));
        message.in_summary = false;
        message.is_summary = true;
        assert!(!is_active_for_context(&message));
    }

    #[test]
    fn chunked_simulation_runs_multiple_passes_within_one_job() {
        let settings = chunked_test_settings();
        let messages: Vec<Message> = (0..12).map(|i| msg(i, &"word ".repeat(500))).collect();
        let simulation = simulate_chunked_summarize_passes(&settings, "", &messages, None, true);

        assert!(
            simulation.passes.len() > 1,
            "expected multiple internal passes for one summarize job, got {}",
            simulation.passes.len()
        );
        assert_eq!(simulation.total_messages, 10);

        let mut flattened = Vec::new();
        for pass in &simulation.passes {
            assert!(!pass.is_empty());
            for id in pass {
                assert!(!flattened.contains(id));
                flattened.push(*id);
            }
        }
        assert_eq!(flattened.len(), simulation.total_messages);
        assert_eq!(flattened.first().copied(), Some(0));
    }

    #[test]
    fn chunked_simulation_pass_batches_are_oldest_first() {
        let settings = chunked_test_settings();
        let messages: Vec<Message> = (0..12).map(|i| msg(i, &"word ".repeat(500))).collect();
        let simulation = simulate_chunked_summarize_passes(&settings, "", &messages, None, true);

        let first_pass = simulation.passes.first().expect("at least one pass");
        assert_eq!(first_pass.first().copied(), Some(0));
        if simulation.passes.len() > 1 {
            let second_pass = &simulation.passes[1];
            assert!(second_pass.first().copied().unwrap() > first_pass.last().copied().unwrap());
        }
    }

    mod integration {
        use super::*;
        use dreamwell_types::{CharacterCreate, SettingsUpdate};
        use std::sync::atomic::{AtomicUsize, Ordering};
        use std::sync::Arc;
        use tokio::sync::mpsc;

        async fn test_pool() -> (tempfile::TempDir, SqlitePool) {
            let dir = tempfile::tempdir().expect("tempdir");
            let path = dir.path().join("test.db");
            let pool = db::connect(&format!("sqlite:{}", path.display()))
                .await
                .expect("connect");
            (dir, pool)
        }

        async fn test_settings_with_model(pool: &SqlitePool) -> Settings {
            let conn = db::create_inference_connection(
                pool,
                dreamwell_types::InferenceConnectionCreate {
                    name: "Test".into(),
                    inference_url: "http://localhost:11434/v1".into(),
                    api_key: None,
                },
            )
            .await
            .expect("connection");
            db::update_inference_connection(
                pool,
                conn.id,
                dreamwell_types::InferenceConnectionUpdate {
                    model: Some("test-model".into()),
                    ..Default::default()
                },
            )
            .await
            .expect("model");
            db::update_settings(
                pool,
                SettingsUpdate {
                    active_connection_id: Some(conn.id),
                    model: Some("test-model".into()),
                    context_tokens: Some(4096),
                    summarize_enabled: Some(true),
                    summarize_adaptive: Some(false),
                    summarize_keep_recent: Some(2),
                    ..Default::default()
                },
            )
            .await
            .expect("settings")
        }

        async fn seed_chat_with_messages(
            pool: &SqlitePool,
            count: usize,
            word_repeat: usize,
        ) -> i64 {
            let character = db::create_character(
                pool,
                CharacterCreate {
                    name: "Tester".into(),
                    description: String::new(),
                    personality: String::new(),
                    scenario: String::new(),
                    first_message: String::new(),
                    example_dialogue: String::new(),
                    system_prompt: String::new(),
                    avatar_url: None,
                },
            )
            .await
            .expect("character");
            let chat = db::create_chat(pool, "Chunked summarize".into(), character.id)
                .await
                .expect("chat");
            for idx in 0..count {
                let role = if idx % 2 == 0 {
                    MessageRole::User
                } else {
                    MessageRole::Assistant
                };
                db::insert_message(pool, chat.id, role, "word ".repeat(word_repeat), false)
                    .await
                    .expect("message");
            }
            chat.id
        }

        async fn summarize_job_count(pool: &SqlitePool, chat_id: i64) -> i64 {
            sqlx::query_scalar(
                "SELECT COUNT(*) FROM generation_jobs WHERE chat_id = ?1 AND job_type = 'chat_summarize'",
            )
            .bind(chat_id)
            .fetch_one(pool)
            .await
            .expect("count")
        }

        #[tokio::test]
        async fn manual_summarize_enqueues_one_job_not_many() {
            let (_dir, pool) = test_pool().await;
            let settings = test_settings_with_model(&pool).await;
            let chat_id = seed_chat_with_messages(&pool, 12, 500).await;
            let (work_tx, _work_rx) = mpsc::unbounded_channel();

            let job = enqueue_summarize_for_chat(&pool, &work_tx, chat_id, &settings)
                .await
                .expect("enqueue");
            assert_eq!(summarize_job_count(&pool, chat_id).await, 1);

            let second = enqueue_summarize_for_chat(&pool, &work_tx, chat_id, &settings).await;
            assert!(
                second.is_err(),
                "active summarize job should block another enqueue"
            );
            assert_eq!(summarize_job_count(&pool, chat_id).await, 1);
            assert_eq!(job.job_type, dreamwell_types::JobType::ChatSummarize);
        }

        #[tokio::test]
        async fn execute_summarize_loop_runs_multiple_passes_before_completion() {
            let (_dir, pool) = test_pool().await;
            let settings = test_settings_with_model(&pool).await;
            let chat_id = seed_chat_with_messages(&pool, 12, 500).await;
            let initial_count = db::list_messages(&pool, chat_id)
                .await
                .expect("messages")
                .len();

            let pass_counter = Arc::new(AtomicUsize::new(0));
            let progress = execute_summarize_loop(&pool, chat_id, &settings, true, {
                let pass_counter = pass_counter.clone();
                move |_prompt| {
                    let pass = pass_counter.fetch_add(1, Ordering::SeqCst) + 1;
                    async move { Ok(format!("summary-after-pass-{pass}")) }
                }
            })
            .await
            .expect("loop");

            assert!(
                progress.passes_completed > 1,
                "expected multiple inference passes in one job, got {}",
                progress.passes_completed
            );
            assert_eq!(progress.total_summarized, 10);
            assert_eq!(
                progress.current_summary,
                format!("summary-after-pass-{}", progress.passes_completed)
            );

            let remaining = db::list_messages(&pool, chat_id).await.expect("messages");
            assert_eq!(
                remaining.len(),
                initial_count,
                "summarized messages stay in chat history"
            );
            assert_eq!(
                remaining
                    .iter()
                    .filter(|message| message.in_summary)
                    .count(),
                10
            );
            let chat = db::get_chat(&pool, chat_id).await.expect("chat");
            assert_eq!(chat.summary, "summary-after-pass-2");
        }

        #[tokio::test]
        async fn failed_pass_keeps_prior_passes_and_removing_marker_matches_failure_cleanup() {
            let (_dir, pool) = test_pool().await;
            let settings = test_settings_with_model(&pool).await;
            let chat_id = seed_chat_with_messages(&pool, 12, 500).await;
            let initial_history = db::list_messages(&pool, chat_id)
                .await
                .expect("messages")
                .iter()
                .filter(|message| !message.is_summary)
                .count();

            let (work_tx, _work_rx) = mpsc::unbounded_channel();
            let job = enqueue_summarize_for_chat(&pool, &work_tx, chat_id, &settings)
                .await
                .expect("enqueue");
            let marker_id = job.message_id.expect("marker");

            let pass_counter = Arc::new(AtomicUsize::new(0));
            let result = execute_summarize_loop(&pool, chat_id, &settings, true, {
                let pass_counter = pass_counter.clone();
                move |_prompt| {
                    let pass = pass_counter.fetch_add(1, Ordering::SeqCst) + 1;
                    async move {
                        if pass >= 2 {
                            Err(crate::error::AppError::inference("inference unavailable"))
                        } else {
                            Ok("partial-summary".into())
                        }
                    }
                }
            })
            .await;
            assert!(result.is_err(), "second pass should fail");

            let chat = db::get_chat(&pool, chat_id).await.expect("chat");
            assert_eq!(
                chat.summary, "partial-summary",
                "completed passes stay committed when a later pass fails"
            );

            let remaining = db::list_messages(&pool, chat_id).await.expect("messages");
            assert_eq!(
                remaining
                    .iter()
                    .filter(|message| !message.is_summary)
                    .count(),
                initial_history,
                "conversation messages remain in history after a failed pass"
            );
            assert!(
                remaining.iter().any(|message| message.in_summary),
                "completed passes should mark messages as folded into the summary"
            );
            assert!(
                remaining
                    .iter()
                    .any(|message| !message.is_summary && !message.in_summary),
                "failed pass should not fold the remaining planned messages"
            );

            db::delete_messages(&pool, &[marker_id])
                .await
                .expect("marker cleanup");
            db::complete_job(
                &pool,
                job.id,
                dreamwell_types::JobStatus::Failed,
                Some("inference unavailable".into()),
            )
            .await
            .expect("fail job");

            let messages = db::list_messages(&pool, chat_id).await.expect("messages");
            assert!(
                !messages.iter().any(|message| message.id == marker_id),
                "failed summarize jobs remove the placeholder marker"
            );
            assert_eq!(chat.summary, "partial-summary");
        }
    }
}
