use dreamwell_types::{Character, MessageRole, Settings};
use serde_json::json;
use sqlx::SqlitePool;

use crate::db;
use crate::error::AppResult;

pub fn substitute_macros(text: &str, char_name: &str, user_name: &str) -> String {
    text.replace("{{char}}", char_name)
        .replace("{{user}}", user_name)
        .replace("{{Char}}", char_name)
        .replace("{{User}}", user_name)
}

fn build_character_system_prompt(character: &Character, user_name: &str) -> String {
    if !character.system_prompt.trim().is_empty() {
        return substitute_macros(character.system_prompt.trim(), &character.name, user_name);
    }
    let mut parts = Vec::new();
    if !character.description.trim().is_empty() {
        parts.push(substitute_macros(
            character.description.trim(),
            &character.name,
            user_name,
        ));
    }
    if !character.personality.trim().is_empty() {
        parts.push(format!(
            "{}'s personality: {}",
            character.name,
            substitute_macros(character.personality.trim(), &character.name, user_name)
        ));
    }
    if !character.scenario.trim().is_empty() {
        parts.push(format!(
            "Scenario: {}",
            substitute_macros(character.scenario.trim(), &character.name, user_name)
        ));
    }
    parts.join("\n\n")
}

pub fn parse_example_dialogue(
    text: &str,
    char_name: &str,
    user_name: &str,
) -> Vec<(MessageRole, String)> {
    if !text.contains("<START>") && !text.contains("<start>") {
        return Vec::new();
    }
    let mut messages = Vec::new();
    for block in text.split("<START>").flat_map(|b| b.split("<start>")) {
        let block = block.trim();
        if block.is_empty() {
            continue;
        }
        for line in block.lines() {
            let line = line.trim();
            if line.is_empty() {
                continue;
            }
            let (role, content) = if let Some(rest) = line
                .strip_prefix("{{user}}:")
                .or_else(|| line.strip_prefix("{{User}}:"))
            {
                (MessageRole::User, rest.trim())
            } else if let Some(rest) = line
                .strip_prefix("{{char}}:")
                .or_else(|| line.strip_prefix("{{Char}}:"))
            {
                (MessageRole::Assistant, rest.trim())
            } else if let Some(rest) = line.strip_prefix(&format!("{user_name}:")) {
                (MessageRole::User, rest.trim())
            } else if let Some(rest) = line.strip_prefix(&format!("{char_name}:")) {
                (MessageRole::Assistant, rest.trim())
            } else if let Some((speaker, rest)) = line.split_once(':') {
                let speaker = speaker.trim().to_lowercase();
                if speaker == "user" || speaker == user_name.to_lowercase() {
                    (MessageRole::User, rest.trim())
                } else if speaker == "char" || speaker == char_name.to_lowercase() {
                    (MessageRole::Assistant, rest.trim())
                } else {
                    continue;
                }
            } else {
                continue;
            };
            if !content.is_empty() {
                messages.push((role, substitute_macros(content, char_name, user_name)));
            }
        }
    }
    messages
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
    character_id: i64,
    settings: &Settings,
) -> AppResult<Vec<serde_json::Value>> {
    let character = db::get_character(pool, character_id).await.ok();
    let char_name = character
        .as_ref()
        .map(|c| c.name.as_str())
        .unwrap_or("Character");
    let user_name = settings.user_name.trim();
    let user_name = if user_name.is_empty() {
        "User"
    } else {
        user_name
    };

    let facts = if settings.facts_enabled {
        db::list_facts(pool, chat_id).await?
    } else {
        vec![]
    };

    let mut system_parts = Vec::new();
    if !settings.system_prompt_prefix.trim().is_empty() {
        system_parts.push(substitute_macros(
            settings.system_prompt_prefix.trim(),
            char_name,
            user_name,
        ));
    }
    if let Some(ref character) = character {
        let char_prompt = build_character_system_prompt(character, user_name);
        if !char_prompt.is_empty() {
            system_parts.push(char_prompt);
        }
        let examples = parse_example_dialogue(&character.example_dialogue, char_name, user_name);
        if examples.is_empty() && !character.example_dialogue.trim().is_empty() {
            system_parts.push(format!(
                "Example dialogue:\n{}",
                substitute_macros(character.example_dialogue.trim(), char_name, user_name)
            ));
        }
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

    let mut messages = Vec::new();
    if !system_parts.is_empty() {
        messages.push(json!({
            "role": "system",
            "content": system_parts.join("\n\n"),
        }));
    }

    if let Some(ref character) = character {
        for (role, content) in
            parse_example_dialogue(&character.example_dialogue, char_name, user_name)
        {
            messages.push(json!({
                "role": match role {
                    MessageRole::User => "user",
                    MessageRole::Assistant => "assistant",
                    MessageRole::System => "system",
                },
                "content": content,
            }));
        }
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

    if !settings.system_prompt_suffix.trim().is_empty() {
        messages.push(json!({
            "role": "system",
            "content": substitute_macros(
                settings.system_prompt_suffix.trim(),
                char_name,
                user_name
            ),
        }));
    }

    Ok(messages)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn substitute_macros_replaces_char_and_user() {
        let out = substitute_macros("Hello {{char}}, this is {{user}}.", "Seraphina", "Alex");
        assert_eq!(out, "Hello Seraphina, this is Alex.");
    }

    #[test]
    fn build_character_prompt_uses_st_format() {
        let character = Character {
            id: 1,
            name: "Seraphina".to_string(),
            description: "A brave knight.".to_string(),
            personality: "Stoic and kind.".to_string(),
            scenario: "A rainy tavern.".to_string(),
            first_message: String::new(),
            example_dialogue: String::new(),
            system_prompt: String::new(),
            avatar_url: None,
            created_at: chrono::Utc::now(),
            updated_at: chrono::Utc::now(),
        };
        let prompt = build_character_system_prompt(&character, "User");
        assert!(prompt.contains("A brave knight."));
        assert!(prompt.contains("Seraphina's personality: Stoic and kind."));
        assert!(prompt.contains("Scenario: A rainy tavern."));
        assert!(!prompt.contains("You are Seraphina"));
    }

    #[test]
    fn parse_example_dialogue_splits_start_blocks() {
        let text = "<START>\n{{user}}: Hello\n{{char}}: Hi there\n<START>\n{{user}}: Bye\n{{char}}: Goodbye";
        let parsed = parse_example_dialogue(text, "Seraphina", "User");
        assert_eq!(parsed.len(), 4);
        assert_eq!(parsed[0].0, MessageRole::User);
        assert_eq!(parsed[0].1, "Hello");
        assert_eq!(parsed[1].0, MessageRole::Assistant);
        assert_eq!(parsed[1].1, "Hi there");
    }
}
