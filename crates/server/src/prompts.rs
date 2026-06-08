use dreamwell_types::{Character, MessageRole, Settings};
use serde_json::json;
use sqlx::SqlitePool;

use crate::db;
use crate::error::AppResult;

fn build_character_system_prompt(character: Option<&Character>) -> String {
    let Some(character) = character else {
        return String::new();
    };
    if !character.system_prompt.trim().is_empty() {
        return character.system_prompt.trim().to_string();
    }
    let mut parts = Vec::new();
    if !character.name.is_empty() {
        parts.push(format!("You are {}.", character.name));
    }
    if !character.description.is_empty() {
        parts.push(format!("Description: {}", character.description));
    }
    if !character.personality.is_empty() {
        parts.push(format!("Personality: {}", character.personality));
    }
    if !character.scenario.is_empty() {
        parts.push(format!("Scenario: {}", character.scenario));
    }
    if !character.example_dialogue.is_empty() {
        parts.push(format!("Example dialogue:\n{}", character.example_dialogue));
    }
    parts.join("\n\n")
}

fn format_facts(facts: &[dreamwell_types::Fact]) -> String {
    if facts.is_empty() {
        return String::new();
    }
    let lines: Vec<String> = facts
        .iter()
        .map(|f| format!("- {}: {}", f.key, f.value))
        .collect();
    format!("Known facts about this conversation:\n{}", lines.join("\n"))
}

fn facts_instruction() -> &'static str {
    "You may update conversation facts using XML tags like <fact key=\"location\">tavern</fact>. Only emit fact tags when information should be remembered."
}

pub async fn build_messages_for_inference(
    pool: &SqlitePool,
    chat_id: i64,
    summary: &str,
    character_id: Option<i64>,
    settings: &Settings,
) -> AppResult<Vec<serde_json::Value>> {
    let character = if let Some(cid) = character_id {
        db::get_character(pool, cid).await.ok()
    } else {
        None
    };
    let facts = if settings.facts_enabled {
        db::list_facts(pool, chat_id).await?
    } else {
        vec![]
    };

    let mut system_parts = Vec::new();
    if !settings.system_prompt_prefix.trim().is_empty() {
        system_parts.push(settings.system_prompt_prefix.trim().to_string());
    }
    let char_prompt = build_character_system_prompt(character.as_ref());
    if !char_prompt.is_empty() {
        system_parts.push(char_prompt);
    }
    if !summary.trim().is_empty() {
        system_parts.push(format!("Conversation summary so far:\n{summary}"));
    }
    let facts_text = format_facts(&facts);
    if !facts_text.is_empty() {
        system_parts.push(facts_text);
    }
    if settings.facts_enabled {
        system_parts.push(facts_instruction().to_string());
    }
    if !settings.system_prompt_suffix.trim().is_empty() {
        system_parts.push(settings.system_prompt_suffix.trim().to_string());
    }

    let mut messages = Vec::new();
    if !system_parts.is_empty() {
        messages.push(json!({
            "role": "system",
            "content": system_parts.join("\n\n"),
        }));
    }

    let mut db_messages = db::list_messages(pool, chat_id).await?;
    db_messages.retain(|m| !m.is_summary && m.role != MessageRole::System);
    if settings.max_context_messages > 0 {
        let keep = settings.max_context_messages as usize;
        if db_messages.len() > keep {
            db_messages = db_messages.split_off(db_messages.len() - keep);
        }
    }
    for msg in db_messages {
        messages.push(json!({
            "role": match msg.role {
                MessageRole::User => "user",
                MessageRole::Assistant => "assistant",
                MessageRole::System => "system",
            },
            "content": msg.content,
        }));
    }
    Ok(messages)
}
