use dreamwell_types::{substitute_macros, Character, MacroContext, MessageRole, Settings};
use serde_json::json;
use sqlx::SqlitePool;

use crate::db;
use crate::error::AppResult;

fn build_character_system_prompt(character: &Character, ctx: &MacroContext<'_>) -> String {
    if !character.system_prompt.trim().is_empty() {
        return substitute_macros(character.system_prompt.trim(), ctx);
    }
    let mut parts = Vec::new();
    if !character.description.trim().is_empty() {
        parts.push(substitute_macros(character.description.trim(), ctx));
    }
    if !character.personality.trim().is_empty() {
        parts.push(format!(
            "{}'s personality: {}",
            character.name,
            substitute_macros(character.personality.trim(), ctx)
        ));
    }
    if !character.scenario.trim().is_empty() {
        parts.push(format!(
            "Scenario: {}",
            substitute_macros(character.scenario.trim(), ctx)
        ));
    }
    parts.join("\n\n")
}

pub fn parse_example_dialogue(text: &str, ctx: &MacroContext<'_>) -> Vec<(MessageRole, String)> {
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
            } else if let Some(rest) = line.strip_prefix(&format!("{}:", ctx.effective_user_name()))
            {
                (MessageRole::User, rest.trim())
            } else if let Some(rest) = line.strip_prefix(&format!("{}:", ctx.char_name)) {
                (MessageRole::Assistant, rest.trim())
            } else if let Some((speaker, rest)) = line.split_once(':') {
                let speaker = speaker.trim().to_lowercase();
                if speaker == "user" || speaker == ctx.effective_user_name().to_lowercase() {
                    (MessageRole::User, rest.trim())
                } else if speaker == "char" || speaker == ctx.char_name.to_lowercase() {
                    (MessageRole::Assistant, rest.trim())
                } else {
                    continue;
                }
            } else {
                continue;
            };
            if !content.is_empty() {
                messages.push((role, substitute_macros(content, ctx)));
            }
        }
    }
    messages
}

fn format_variables(variables: &[(String, String)]) -> String {
    if variables.is_empty() {
        return String::new();
    }
    let lines: Vec<String> = variables
        .iter()
        .map(|(key, value)| format!("- {key}: {value}"))
        .collect();
    format!("Current chat variables:\n{}", lines.join("\n"))
}

fn variables_instruction() -> &'static str {
    "You may update chat variables using XML tags like <var key=\"location\">tavern</var>. Use the key attribute (not name). Reusing a key replaces its value. Remove a variable with <var key=\"location\" delete/>. Only emit var tags for mutable session state that should persist (location, inventory, quest stage, etc.), not for static lore."
}

/// Rough token budget for permanent prompt content (character card, system prompts).
pub fn estimate_static_prompt_tokens(settings: &Settings, character: Option<&Character>) -> i64 {
    use dreamwell_types::estimate_token_count;

    let mut chars = settings.system_prompt_prefix.len()
        + settings.system_prompt_suffix.len()
        + settings.persona_description.len()
        + settings.user_name.len();
    if let Some(c) = character {
        chars += c.name.len()
            + c.description.len()
            + c.personality.len()
            + c.scenario.len()
            + c.system_prompt.len()
            + c.example_dialogue.len()
            + c.first_message.len();
    }
    estimate_token_count(&" ".repeat(chars))
}

pub async fn build_messages_for_inference(
    pool: &SqlitePool,
    chat_id: i64,
    summary: &str,
    character_id: i64,
    settings: &Settings,
) -> AppResult<Vec<serde_json::Value>> {
    let character = db::get_character(pool, character_id).await.ok();
    let ctx = MacroContext::from_character_and_settings(
        character.as_ref(),
        &settings.user_name,
        &settings.persona_description,
    );

    let messages = db::list_messages(pool, chat_id).await?;
    let panel = if settings.variables_enabled {
        db::list_variables(pool, chat_id).await?
    } else {
        vec![]
    };
    let variables = crate::variable_state::pairs_sorted(crate::variable_state::chat_state_at(
        &messages,
        &panel,
        i64::MAX,
    ));

    let mut system_parts = Vec::new();
    if !settings.system_prompt_prefix.trim().is_empty() {
        system_parts.push(substitute_macros(
            settings.system_prompt_prefix.trim(),
            &ctx,
        ));
    }
    if !settings.persona_description.trim().is_empty() {
        system_parts.push(substitute_macros(settings.persona_description.trim(), &ctx));
    }
    if let Some(ref character) = character {
        let char_prompt = build_character_system_prompt(character, &ctx);
        if !char_prompt.is_empty() {
            system_parts.push(char_prompt);
        }
        let examples = parse_example_dialogue(&character.example_dialogue, &ctx);
        if examples.is_empty() && !character.example_dialogue.trim().is_empty() {
            system_parts.push(format!(
                "Example dialogue:\n{}",
                substitute_macros(character.example_dialogue.trim(), &ctx)
            ));
        }
    }
    if !summary.trim().is_empty() {
        system_parts.push(format!("Conversation summary so far:\n{summary}"));
    }
    let variables_text = format_variables(&variables);
    if !variables_text.is_empty() {
        system_parts.push(variables_text);
    }
    if settings.variables_enabled {
        system_parts.push(variables_instruction().to_string());
    }

    let mut messages = Vec::new();
    if !system_parts.is_empty() {
        messages.push(json!({
            "role": "system",
            "content": system_parts.join("\n\n"),
        }));
    }

    if let Some(ref character) = character {
        for (role, content) in parse_example_dialogue(&character.example_dialogue, &ctx) {
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
    db_messages.retain(crate::summarize::is_active_for_context);
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
            "content": substitute_macros(settings.system_prompt_suffix.trim(), &ctx),
        }));
    }

    Ok(messages)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_ctx() -> MacroContext<'static> {
        MacroContext {
            char_name: "Seraphina",
            user_name: "Alex",
            persona: "A curious traveler.",
            description: "A brave knight.",
            personality: "Stoic and kind.",
            scenario: "A rainy tavern.",
            first_message: "Hello {{user}}.",
        }
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
        let ctx = test_ctx();
        let prompt = build_character_system_prompt(&character, &ctx);
        assert!(prompt.contains("A brave knight."));
        assert!(prompt.contains("Seraphina's personality: Stoic and kind."));
        assert!(prompt.contains("Scenario: A rainy tavern."));
        assert!(!prompt.contains("You are Seraphina"));
    }

    #[test]
    fn parse_example_dialogue_splits_start_blocks() {
        let text = "<START>\n{{user}}: Hello\n{{char}}: Hi there\n<START>\n{{user}}: Bye\n{{char}}: Goodbye";
        let parsed = parse_example_dialogue(text, &test_ctx());
        assert_eq!(parsed.len(), 4);
        assert_eq!(parsed[0].0, MessageRole::User);
        assert_eq!(parsed[0].1, "Hello");
        assert_eq!(parsed[1].0, MessageRole::Assistant);
        assert_eq!(parsed[1].1, "Hi there");
    }
}
