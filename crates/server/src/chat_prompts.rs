use dreamwell_state::{plan_schema, CHARACTER_ACTION_RULES, PLAN_BEAT_RULES, STATE_CHANGE_PROMPT};
use dreamwell_types::{Character, MacroContext, Settings};
use serde_json::json;

use crate::chat_state::build_state_block;
use crate::db;
use crate::error::AppResult;
use crate::prompts::build_character_system_prompt;
use sqlx::SqlitePool;

const PLAN_SYSTEM: &str = r#"You plan the assistant's next reply before prose is written.

Given the conversation — especially the latest user message — output JSON with:
- reply_beats: short, specific bullet points this reply must cover (in order)
- state_changes: leave as an empty array — durable state is updated via tools during the prose pass

Plan ONLY the single assistant message that directly follows the latest user turn. Beats must be concrete to this turn, not generic chat patterns.

Do not write the final reply prose in this step — beats only."#;

const PROSE_SYSTEM: &str = r#"You write the assistant's reply as natural prose.

Rules:
- Cover every reply beat in order — each beat should be clearly reflected in the prose
- Beats are mandatory staging notes for THIS reply; do not substitute generic filler
- Do not contradict established typed state
- When the narration establishes or changes durable state, call the matching state tool — the tool is the source of truth
- DEFAULT to set_variable for durable text attributes. Use set_condition for ephemeral statuses; set_measurement for floats; set_sequence/step_sequence for ordered lists.
- target is "pc", "world", or a named character; key is a short snake_case attribute; value is just the value, not a sentence.
- When the user must make a choice they have not specified, call present_fork with the situation and options, then stop.
- Plain prose and state tool calls only — no JSON, no XML tags, no meta commentary

Tool call syntax (use this exact format): call:tool_name{key:value,key2:value2}
- Do NOT use parentheses like set_variable(key="mood") — those are not parsed.
- Quote string values that contain spaces or commas: call:set_variable{target:world,key:location,value:"tavern common room"}"#;

pub fn chat_plan_schema() -> serde_json::Value {
    plan_schema("reply_beats")
}

pub async fn build_plan_messages(
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
    let actors = db::list_chat_actors(pool, chat_id).await?;
    let state = db::list_chat_state_entries(pool, chat_id).await?;
    let state_block = build_state_block(&state, &actors);
    let messages = db::list_messages(pool, chat_id).await?;
    let active_messages: Vec<_> = messages
        .iter()
        .filter(|msg| crate::summarize::is_active_for_context(msg))
        .collect();
    let latest_user = active_messages
        .iter()
        .rev()
        .find(|msg| msg.role == dreamwell_types::MessageRole::User);

    let mut context = String::new();
    if !summary.trim().is_empty() {
        context.push_str(&format!("Conversation summary:\n{summary}\n\n"));
    }
    if !state_block.is_empty() {
        context.push_str(&format!("Current typed state:\n{state_block}\n\n"));
    }
    if !actors.is_empty() {
        context.push_str("Session actors (use pc or a name as state target):\n");
        for actor in &actors {
            context.push_str(&format!(
                "- {} ({}){}\n",
                actor.name,
                actor.role,
                if actor.description.is_empty() {
                    String::new()
                } else {
                    format!(" — {}", actor.description)
                }
            ));
        }
        context.push('\n');
    }
    if let Some(user_msg) = latest_user {
        context.push_str("Latest user message (plan the reply to THIS turn):\n");
        context.push_str(&user_msg.content);
        context.push_str("\n\n");
    }
    context.push_str("Recent messages (oldest first):\n");
    for msg in active_messages.iter().rev().take(8).rev() {
        let role = match msg.role {
            dreamwell_types::MessageRole::User => "User",
            dreamwell_types::MessageRole::Assistant => "Assistant",
            dreamwell_types::MessageRole::System => "System",
        };
        context.push_str(&format!("{role}: {}\n", msg.content));
    }
    context.push_str(
        "\nOutput reply_beats that are specific to the latest user message — avoid generic beats that could apply to any turn.",
    );

    if let Some(ref character) = character {
        let char_prompt = build_character_system_prompt(character, &ctx);
        if !char_prompt.is_empty() {
            context.push_str(&format!("\nCharacter:\n{char_prompt}\n"));
        }
    }

    Ok(vec![
        json!({
            "role": "system",
            "content": format!("{PLAN_SYSTEM}\n\n{PLAN_BEAT_RULES}\n\n{STATE_CHANGE_PROMPT}"),
        }),
        json!({
            "role": "user",
            "content": context,
        }),
    ])
}

pub async fn build_prose_messages(
    pool: &SqlitePool,
    chat_id: i64,
    summary: &str,
    beats: &[String],
    state_block: &str,
    settings: &Settings,
    character: Option<&Character>,
) -> AppResult<Vec<serde_json::Value>> {
    let ctx = MacroContext::from_character_and_settings(
        character,
        &settings.user_name,
        &settings.persona_description,
    );
    let beats_text = if beats.is_empty() {
        "(none)".to_string()
    } else {
        beats
            .iter()
            .map(|b| format!("- {b}"))
            .collect::<Vec<_>>()
            .join("\n")
    };
    let mut user = format!("Reply beats to cover:\n{beats_text}");
    if !summary.trim().is_empty() {
        user.push_str(&format!("\n\nConversation summary:\n{summary}"));
    }
    let messages = db::list_messages(pool, chat_id).await?;
    let active_messages: Vec<_> = messages
        .iter()
        .filter(|msg| crate::summarize::is_active_for_context(msg))
        .collect();
    if !active_messages.is_empty() {
        user.push_str("\n\nRecent conversation (oldest first):\n");
        let keep = if settings.max_context_messages > 0 {
            settings.max_context_messages as usize
        } else {
            12
        };
        for msg in active_messages.iter().rev().take(keep).rev() {
            let role = match msg.role {
                dreamwell_types::MessageRole::User => "User",
                dreamwell_types::MessageRole::Assistant => "Assistant",
                dreamwell_types::MessageRole::System => "System",
            };
            user.push_str(&format!("{role}: {}\n", msg.content));
        }
    }
    if !state_block.is_empty() {
        user.push_str(&format!("\n\nCurrent typed state:\n{state_block}"));
    }
    if let Some(character) = character {
        let char_prompt = build_character_system_prompt(character, &ctx);
        if !char_prompt.is_empty() {
            user.push_str(&format!("\n\nCharacter:\n{char_prompt}"));
        }
    }

    Ok(vec![
        json!({
            "role": "system",
            "content": format!("{PROSE_SYSTEM}\n\n{CHARACTER_ACTION_RULES}"),
        }),
        json!({
            "role": "user",
            "content": user,
        }),
    ])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plan_system_includes_specific_beat_rules() {
        assert!(PLAN_SYSTEM.contains("specific"));
        assert!(PLAN_SYSTEM.contains("empty array"));
        assert!(PLAN_BEAT_RULES.contains("too generic"));
        assert!(PLAN_BEAT_RULES.contains("Describe how nervous Maya looks"));
    }

    #[test]
    fn prose_system_includes_state_tools() {
        assert!(PROSE_SYSTEM.contains("set_variable"));
        assert!(PROSE_SYSTEM.contains("present_fork"));
    }

    #[test]
    fn prose_system_includes_character_action_rules() {
        let prose_system = format!("{PROSE_SYSTEM}\n\n{CHARACTER_ACTION_RULES}");
        assert!(prose_system.contains("action and spoken lines"));
        assert!(prose_system.contains("Maya picks at the napkin"));
    }
}
