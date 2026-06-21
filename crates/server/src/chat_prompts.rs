use dreamwell_state::{plan_schema, PLAN_BEAT_RULES, STATE_CHANGE_PROMPT};
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
- state_changes: typed durable state updates that should persist after this reply

Plan ONLY the single assistant message that directly follows the latest user turn. Beats must be concrete to this turn, not generic chat patterns.

Do not write the final reply prose in this step — beats and state only."#;

const PROSE_SYSTEM: &str = r#"You write the assistant's reply as natural prose.

Rules:
- Cover every reply beat in order — each beat should be clearly reflected in the prose
- Beats are mandatory staging notes for THIS reply; do not substitute generic filler
- Do not contradict established typed state
- No JSON, no XML tags, no meta commentary — prose only"#;

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
            "content": PROSE_SYSTEM,
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
        assert!(PLAN_BEAT_RULES.contains("too generic"));
    }
}
