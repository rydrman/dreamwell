use chrono::{DateTime, Duration, Utc};
use dreamwell_types::{
    estimate_token_count, prompt_token_budget, Character, Message, MessageRole, Settings,
};
use serde_json::json;
use sqlx::SqlitePool;
use tokio::sync::mpsc;

use crate::db;
use crate::error::AppResult;
use crate::inference::chat_completion;
use crate::prompts::estimate_static_prompt_tokens;

const SUMMARIZE_PLACEHOLDER: &str = "Summarizing earlier messages…";

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
) -> Option<SummarizePlan> {
    if !settings.summarize_enabled {
        return None;
    }

    let non_system: Vec<&Message> = messages
        .iter()
        .filter(|m| m.role != MessageRole::System && !m.is_summary)
        .collect();

    let min_total = settings.summarize_after_messages.max(4) as usize;
    if non_system.len() <= min_total {
        return None;
    }

    let min_keep = settings.summarize_keep_recent.max(2) as usize;
    let keep = if settings.summarize_adaptive && settings.context_tokens > 0 {
        adaptive_keep_count(settings, chat_summary, &non_system, character, min_keep)
    } else {
        settings.summarize_keep_recent.max(2) as usize
    };

    let split_at = non_system.len().saturating_sub(keep);
    if split_at == 0 {
        return None;
    }

    if (!settings.summarize_adaptive || settings.context_tokens <= 0)
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

pub async fn maybe_enqueue_summarize(
    pool: &SqlitePool,
    work_tx: &mpsc::UnboundedSender<()>,
    chat_id: i64,
    settings: &Settings,
) -> AppResult<()> {
    if !settings.summarize_enabled || settings.model.is_empty() {
        return Ok(());
    }
    if db::has_active_summarize_job(pool, chat_id).await? {
        return Ok(());
    }

    let chat = db::get_chat(pool, chat_id).await?;
    let character = db::get_character(pool, chat.character_id).await.ok();
    let messages = db::list_messages(pool, chat_id).await?;
    let Some(plan) = plan_summarization(settings, &chat.summary, &messages, character.as_ref())
    else {
        return Ok(());
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
    db::enqueue_summarize_job(pool, chat_id, marker.id).await?;
    let _ = work_tx.send(());
    Ok(())
}

pub async fn run_summarize_job(
    pool: &SqlitePool,
    job_id: i64,
    chat_id: i64,
    marker_id: i64,
    settings: &Settings,
) -> AppResult<()> {
    let chat = db::get_chat(pool, chat_id).await?;
    let character = db::get_character(pool, chat.character_id).await.ok();
    let messages = db::list_messages(pool, chat_id).await?;

    let Some(plan) = plan_summarization(settings, &chat.summary, &messages, character.as_ref())
    else {
        db::delete_messages(pool, &[marker_id]).await?;
        db::complete_job(pool, job_id, dreamwell_types::JobStatus::Completed, None).await?;
        return Ok(());
    };

    let to_summarize: Vec<&Message> = messages
        .iter()
        .filter(|m| plan.to_summarize_ids.contains(&m.id))
        .collect();

    let transcript = to_summarize
        .iter()
        .map(|m| {
            let role = match m.role {
                MessageRole::User => "user",
                MessageRole::Assistant => "assistant",
                MessageRole::System => "system",
            };
            format!("{role}: {}", m.content)
        })
        .collect::<Vec<_>>()
        .join("\n");

    let prompt = vec![
        json!({
            "role": "system",
            "content": "Summarize the following roleplay conversation. Preserve key plot points, relationships, character voice, and established facts. Be concise but complete.",
        }),
        json!({
            "role": "user",
            "content": format!(
                "Previous summary:\n{}\n\nNew messages to incorporate:\n{transcript}",
                if chat.summary.is_empty() {
                    "(none)"
                } else {
                    &chat.summary
                }
            ),
        }),
    ];

    let max_summary_tokens = (settings.context_tokens / 8).clamp(256, 1024).max(256);
    let summary = chat_completion(
        &settings.inference_url,
        &settings.model,
        &prompt,
        0.3,
        settings.top_p,
        max_summary_tokens,
    )
    .await?;

    let summary = summary.trim().to_string();
    if summary.is_empty() {
        return Err(crate::error::AppError::inference(
            "summarization returned empty text",
        ));
    }

    db::update_chat_summary(pool, chat_id, &summary).await?;
    db::delete_messages(pool, &plan.to_summarize_ids).await?;

    let marker_body = format_summary_marker(plan.summarized_count, &summary);
    db::update_message_content(pool, marker_id, &marker_body).await?;
    let anchor = (plan.anchor_before - Duration::milliseconds(1)).to_rfc3339();
    db::set_message_created_at(pool, marker_id, &anchor).await?;
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
        "**Earlier conversation summarized** ({summarized_count} messages)\n\n{preview}\n\n_The full summary is included in the model's context for future replies._"
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;
    use dreamwell_types::Settings;

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
            is_summary: false,
            created_at: Utc::now(),
            job_status: None,
        }
    }

    fn test_settings() -> Settings {
        Settings {
            inference_url: String::new(),
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
        }
    }

    #[test]
    fn adaptive_plan_trims_when_history_exceeds_budget() {
        let settings = test_settings();
        let messages: Vec<Message> = (0..12).map(|i| msg(i, &"word ".repeat(200))).collect();
        let plan = plan_summarization(&settings, "", &messages, None);
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
        assert!(plan_summarization(&settings, "", &messages, None).is_none());
        let messages: Vec<Message> = (0..25).map(|i| msg(i, "hi")).collect();
        let plan = plan_summarization(&settings, "", &messages, None).unwrap();
        assert_eq!(plan.summarized_count, 17);
    }

    #[test]
    fn format_summary_marker_includes_count() {
        let body = format_summary_marker(5, "They met at the tavern.");
        assert!(body.contains("5 messages"));
        assert!(body.contains("tavern"));
    }
}
