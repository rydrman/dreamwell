use dreamwell_types::{Job, JobType, MessageRole, Settings};
use serde_json::json;
use sqlx::SqlitePool;
use tokio::sync::mpsc;

use crate::config;
use crate::db;
use crate::error::{AppError, AppResult};
use crate::inference::chat_completion;
use crate::variables::{
    apply_variable_updates, build_message_variable_updates, merge_variable_tags_into_message,
    parse_variable_updates,
};

const RECHECK_SYSTEM_PROMPT: &str = r#"You review assistant chat replies for chat variable state.

Given the reply text and current variables, output ONLY <var> XML tags to correct, add, or remove variables that should persist across the session (location, inventory, HP, quest stage, mood, etc.).

Rules:
- Use <var key="name">value</var> to set or replace a value
- Use <var key="name" delete/> to remove a variable
- Fix values that contradict the narrative
- Add variables for state changes described in prose but missing tags
- Do not repeat tags for values that are already correct
- Output nothing (empty response) if no corrections are needed
- Do not include prose, markdown, or explanations — only var tags"#;

fn recheck_output_tokens(settings: &Settings) -> i64 {
    if settings.context_tokens > 0 {
        (settings.context_tokens / 16).clamp(128, 512)
    } else {
        256
    }
}

fn recheck_max_retries() -> u32 {
    config::GENERATION_MAX_RETRIES
        .load(std::sync::atomic::Ordering::SeqCst)
        .max(1)
}

fn format_current_variables(variables: &[(String, String)]) -> String {
    if variables.is_empty() {
        "(none)".to_string()
    } else {
        variables
            .iter()
            .map(|(key, value)| format!("- {key}: {value}"))
            .collect::<Vec<_>>()
            .join("\n")
    }
}

fn build_recheck_prompt(reply: &str, variables: &[(String, String)]) -> Vec<serde_json::Value> {
    vec![
        json!({
            "role": "system",
            "content": RECHECK_SYSTEM_PROMPT,
        }),
        json!({
            "role": "user",
            "content": format!(
                "Current chat variables:\n{}\n\nAssistant reply to review:\n{reply}",
                format_current_variables(variables),
            ),
        }),
    ]
}

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

pub async fn run_variable_recheck_job(
    pool: &SqlitePool,
    job_id: i64,
    chat_id: i64,
    message_id: i64,
    settings: &Settings,
) -> AppResult<()> {
    if !settings.variables_enabled {
        db::complete_job(pool, job_id, dreamwell_types::JobStatus::Completed, None).await?;
        return Ok(());
    }

    let message = db::get_message(pool, chat_id, message_id).await?;
    if message.content.trim().is_empty() {
        db::complete_job(pool, job_id, dreamwell_types::JobStatus::Completed, None).await?;
        return Ok(());
    }

    let messages = db::list_messages(pool, chat_id).await?;
    let panel = db::list_variables(pool, chat_id).await?;
    let current_state = crate::variable_state::pairs_sorted(crate::variable_state::chat_state_at(
        &messages, &panel, message_id,
    ));
    let prompt = build_recheck_prompt(&message.content, &current_state);

    let max_attempts = recheck_max_retries();
    let mut raw = None;

    for attempt in 1..=max_attempts {
        match chat_completion(
            &settings.inference_url,
            &settings.model,
            &prompt,
            0.2,
            settings.top_p,
            recheck_output_tokens(settings),
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
                    "retrying variable recheck"
                );
            }
        }
    }

    let raw = raw.unwrap_or_default();

    let parsed = parse_variable_updates(&raw);
    if parsed.is_empty() {
        db::complete_job(pool, job_id, dreamwell_types::JobStatus::Completed, None).await?;
        return Ok(());
    }

    let meaningful =
        crate::story_variables::filter_meaningful_story_updates(&parsed, &current_state);
    if meaningful.is_empty() {
        db::complete_job(pool, job_id, dreamwell_types::JobStatus::Completed, None).await?;
        return Ok(());
    }

    let updates_for_message =
        build_message_variable_updates(pool, chat_id, message_id, &meaningful).await?;
    apply_variable_updates(pool, chat_id, message_id, &meaningful).await?;

    let updated_content = merge_variable_tags_into_message(&message.content, &meaningful);
    db::append_message_variable_recheck(pool, message_id, &updated_content, &updates_for_message)
        .await?;
    db::touch_chat(pool, chat_id).await?;
    db::complete_job(pool, job_id, dreamwell_types::JobStatus::Completed, None).await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recheck_prompt_includes_reply_and_variables() {
        let prompt = build_recheck_prompt(
            "You enter the tavern.",
            &[("location".to_string(), "forest".to_string())],
        );
        let user = prompt[1]["content"].as_str().unwrap();
        assert!(user.contains("location: forest"));
        assert!(user.contains("You enter the tavern."));
    }
}
