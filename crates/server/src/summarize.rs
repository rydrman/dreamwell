use dreamwell_types::{MessageRole, Settings};
use serde_json::json;
use sqlx::SqlitePool;

use crate::db;
use crate::error::AppResult;
use crate::inference::chat_completion;

pub async fn maybe_summarize_chat(
    pool: &SqlitePool,
    chat_id: i64,
    settings: &Settings,
) -> AppResult<()> {
    if !settings.summarize_enabled || settings.model.is_empty() {
        return Ok(());
    }

    let chat = db::get_chat(pool, chat_id).await?;
    let messages = db::list_messages(pool, chat_id).await?;
    let non_system: Vec<_> = messages
        .into_iter()
        .filter(|m| m.role != MessageRole::System && !m.is_summary)
        .collect();

    if non_system.len() as i64 <= settings.summarize_after_messages {
        return Ok(());
    }

    let keep = settings.summarize_keep_recent as usize;
    let split_at = non_system.len().saturating_sub(keep);
    if split_at == 0 {
        return Ok(());
    }
    let (to_summarize, _) = non_system.split_at(split_at);
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
            "content": "Summarize the following roleplay conversation. Preserve key plot points, relationships, and facts. Be concise.",
        }),
        json!({
            "role": "user",
            "content": format!(
                "Previous summary:\n{}\n\nNew messages to incorporate:\n{transcript}",
                if chat.summary.is_empty() { "(none)" } else { &chat.summary }
            ),
        }),
    ];

    let summary = chat_completion(
        &settings.inference_url,
        &settings.model,
        &prompt,
        0.3,
        settings.top_p,
        512,
    )
    .await;

    let summary = match summary {
        Ok(s) => s,
        Err(_) => return Ok(()),
    };

    db::update_chat_summary(pool, chat_id, summary.trim()).await?;
    let ids: Vec<i64> = to_summarize.iter().map(|m| m.id).collect();
    db::delete_messages(pool, &ids).await?;
    Ok(())
}
