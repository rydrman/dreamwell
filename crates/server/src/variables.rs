use std::collections::HashSet;

use dreamwell_types::{Message, MessageVariableUpdate};
use regex::Regex;
use sqlx::SqlitePool;
use std::sync::OnceLock;

use crate::db;
use crate::error::AppResult;

const TAG: &str = r"(?:var|fact|variable)";
const IDENT: &str = r#"(?:key|name)\s*=\s*["']?([^"'>\s]+)["']?"#;

/// Strips variable tags for display or storage (e.g. story beat prose).
pub fn strip_variables_for_display(text: &str, hold_incomplete: bool) -> String {
    strip_variable_markup(text, hold_incomplete)
}

fn strip_variable_markup(text: &str, hold_incomplete: bool) -> String {
    let mut working = text.to_string();
    working = strip_delete_tags(&working);
    working = strip_set_value_tags(&working);
    working = strip_set_tags(&working);
    working = strip_orphan_closing_tags(&working);
    working = strip_incomplete_variable_tags(&working, hold_incomplete);
    collapse_spaces(working.trim())
}

fn delete_patterns() -> &'static [Regex] {
    static PATTERNS: OnceLock<Vec<Regex>> = OnceLock::new();
    PATTERNS.get_or_init(|| {
        vec![
            Regex::new(&format!(
                r#"(?is)<{TAG}\b[^>]*?{IDENT}[^>]*\bdelete\b(?:\s*=\s*["']?(?:true|1)["']?)?[^>]*/>"#
            ))
            .expect("delete self-closing regex"),
            Regex::new(&format!(
                r#"(?is)<{TAG}\b[^>]*?{IDENT}[^>]*\bdelete\b[^>]*>\s*</{TAG}\s*>"#
            ))
            .expect("delete empty element regex"),
        ]
    })
}

fn set_value_pattern() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(&format!(
            r#"(?is)<{TAG}\b[^>]*?{IDENT}[^>]*\bvalue\s*=\s*["']?([^"'>\s]*)["']?[^>]*/>"#
        ))
        .expect("set value self-closing regex")
    })
}

fn set_pattern() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(&format!(
            r#"(?is)<{TAG}\b[^>]*?{IDENT}[^>]*>(.*?)</{TAG}\s*>"#
        ))
        .expect("set regex")
    })
}

fn strip_delete_tags(text: &str) -> String {
    let mut working = text.to_string();
    for re in delete_patterns() {
        working = re.replace_all(&working, "").into_owned();
    }
    working
}

fn strip_set_value_tags(text: &str) -> String {
    set_value_pattern().replace_all(text, "").into_owned()
}

fn strip_set_tags(text: &str) -> String {
    set_pattern().replace_all(text, "").into_owned()
}

fn strip_orphan_closing_tags(text: &str) -> String {
    static RE: OnceLock<Regex> = OnceLock::new();
    let re =
        RE.get_or_init(|| Regex::new(&format!(r"(?is)</{TAG}\s*>")).expect("orphan close regex"));
    re.replace_all(text, "").into_owned()
}

fn strip_incomplete_variable_tags(text: &str, hold_incomplete: bool) -> String {
    let (mut visible, has_unclosed) = split_unclosed_variable_tag(text);
    if has_unclosed && !hold_incomplete {
        return visible;
    }

    if hold_incomplete {
        let holdback = trailing_partial_var_prefix(&visible);
        let visible_len = visible.len().saturating_sub(holdback);
        visible = visible[..visible_len].trim_end().to_string();
    }
    visible
}

fn split_unclosed_variable_tag(text: &str) -> (String, bool) {
    let lower = text.to_lowercase();
    let mut last_unclosed: Option<usize> = None;

    for open in ["<variable", "<fact", "<var"] {
        if let Some(pos) = lower.rfind(open) {
            if !variable_tag_is_complete(&lower[pos..])
                && last_unclosed.is_none_or(|existing| pos > existing)
            {
                last_unclosed = Some(pos);
            }
        }
    }

    match last_unclosed {
        Some(pos) => (text[..pos].trim_end().to_string(), true),
        None => (text.to_string(), false),
    }
}

fn variable_tag_is_complete(lower: &str) -> bool {
    if let Some(slash_end) = lower.find("/>") {
        if !lower[..slash_end].contains('>') {
            return true;
        }
    }

    lower.contains("</var>") || lower.contains("</fact>") || lower.contains("</variable>")
}

fn trailing_partial_var_prefix(text: &str) -> usize {
    const PREFIXES: &[&str] = &[
        "</variable>",
        "</fact>",
        "</var>",
        "<variable",
        "<fact",
        "<var",
        "</",
        "<",
    ];
    let mut max_len = 0;
    for prefix in PREFIXES {
        for i in 1..prefix.len() {
            if text.ends_with(&prefix[..i]) {
                max_len = max_len.max(i);
            }
        }
    }
    max_len
}

fn collapse_spaces(text: &str) -> String {
    static SPACES: OnceLock<Regex> = OnceLock::new();
    let spaces = SPACES.get_or_init(|| Regex::new(r" {2,}").expect("space collapse regex"));
    spaces.replace_all(text, " ").to_string()
}

pub async fn revert_message_variable_updates(
    pool: &SqlitePool,
    chat_id: i64,
    message_id: i64,
    updates: &[MessageVariableUpdate],
) -> AppResult<()> {
    for update in updates.iter().rev() {
        if update.clears() {
            if let Some(previous) = &update.previous_value {
                db::upsert_variable(
                    pool,
                    chat_id,
                    update.key.clone(),
                    previous.clone(),
                    message_id,
                )
                .await?;
            }
        } else {
            let _ = db::delete_variable_scoped(pool, chat_id, &update.key, message_id).await;
        }
    }
    Ok(())
}

pub fn remove_variable_tags_for_keys(text: &str, keys: &HashSet<&str>) -> String {
    if keys.is_empty() {
        return text.to_string();
    }
    let mut working = text.to_string();
    for re in delete_patterns() {
        working = re
            .replace_all(&working, |caps: &regex::Captures| {
                let key = caps.get(1).map(|m| m.as_str().trim()).unwrap_or_default();
                if keys.contains(key) {
                    String::new()
                } else {
                    caps.get(0)
                        .map(|m| m.as_str().to_string())
                        .unwrap_or_default()
                }
            })
            .into_owned();
    }
    working = set_value_pattern()
        .replace_all(&working, |caps: &regex::Captures| {
            let key = caps.get(1).map(|m| m.as_str().trim()).unwrap_or_default();
            if keys.contains(key) {
                String::new()
            } else {
                caps.get(0)
                    .map(|m| m.as_str().to_string())
                    .unwrap_or_default()
            }
        })
        .into_owned();
    working = set_pattern()
        .replace_all(&working, |caps: &regex::Captures| {
            let key = caps.get(1).map(|m| m.as_str().trim()).unwrap_or_default();
            if keys.contains(key) {
                String::new()
            } else {
                caps.get(0)
                    .map(|m| m.as_str().to_string())
                    .unwrap_or_default()
            }
        })
        .into_owned();
    collapse_spaces(working.trim()).to_string()
}

/// Keeps only updates that would change current chat variable state.
pub async fn revert_variable_updates_from_messages(
    pool: &SqlitePool,
    chat_id: i64,
    messages: &[Message],
) -> AppResult<()> {
    for message in messages.iter().rev() {
        revert_message_variable_updates(pool, chat_id, message.id, &message.variable_updates)
            .await?;
    }
    Ok(())
}

/// Removes a deleted variable's tags from all chat messages and their update history.
pub async fn strip_variable_key_from_chat_messages(
    pool: &SqlitePool,
    chat_id: i64,
    key: &str,
) -> AppResult<()> {
    let keys: HashSet<&str> = std::iter::once(key).collect();
    let messages = db::list_messages(pool, chat_id).await?;
    for message in messages {
        let stripped_content = remove_variable_tags_for_keys(&message.content, &keys);
        let original_update_count = message.variable_updates.len();
        let filtered_updates: Vec<MessageVariableUpdate> = message
            .variable_updates
            .into_iter()
            .filter(|update| update.key != key)
            .collect();
        if stripped_content != message.content || filtered_updates.len() != original_update_count {
            db::update_message_content_and_variable_updates(
                pool,
                message.id,
                &stripped_content,
                &filtered_updates,
            )
            .await?;
        }
    }
    Ok(())
}

/// Removes a deleted variable's tags and update history from all story beats.
pub async fn strip_variable_key_from_story_beats(
    pool: &SqlitePool,
    story_id: i64,
    key: &str,
) -> AppResult<()> {
    let keys: HashSet<&str> = std::iter::once(key).collect();
    let detail = db::get_story_detail(pool, story_id).await?;
    for chapter in &detail.chapters {
        for beat in &chapter.beats {
            let stripped_content = remove_variable_tags_for_keys(&beat.content, &keys);
            let filtered_updates: Vec<MessageVariableUpdate> = beat
                .variable_updates
                .iter()
                .filter(|update| update.key != key)
                .cloned()
                .collect();
            if stripped_content != beat.content
                || filtered_updates.len() != beat.variable_updates.len()
            {
                db::finalize_beat_prose(
                    pool,
                    story_id,
                    chapter.sort_order,
                    beat.id,
                    &stripped_content,
                    &filtered_updates,
                )
                .await?;
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strip_variables_for_display_removes_tags() {
        assert_eq!(
            strip_variables_for_display(r#"Hello <var key="location">tavern</var> world"#, false),
            "Hello world"
        );
    }

    async fn test_chat(pool: &SqlitePool) -> dreamwell_types::Chat {
        let character = db::create_character(
            pool,
            dreamwell_types::CharacterCreate {
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
        db::create_chat(pool, "vars".into(), character.id)
            .await
            .expect("chat")
    }

    #[tokio::test]
    async fn revert_message_variable_updates_restores_previous_values() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("test.db");
        let pool = db::connect(&format!("sqlite:{}", path.display()))
            .await
            .expect("connect");
        let chat = test_chat(&pool).await;

        db::upsert_variable(
            &pool,
            chat.id,
            "location".into(),
            "forest".into(),
            crate::variable_state::MANUAL_MESSAGE_SOURCE,
        )
        .await
        .expect("seed variable");

        let message = db::insert_message(
            &pool,
            chat.id,
            dreamwell_types::MessageRole::Assistant,
            "reply".into(),
            false,
        )
        .await
        .expect("message");

        let updates = vec![
            MessageVariableUpdate {
                key: "location".into(),
                value: "tavern".into(),
                previous_value: Some("forest".into()),
            },
            MessageVariableUpdate {
                key: "quest".into(),
                value: String::new(),
                previous_value: None,
            },
        ];
        db::upsert_variable(
            &pool,
            chat.id,
            "location".into(),
            "tavern".into(),
            message.id,
        )
        .await
        .expect("apply location");
        let _ = db::delete_variable_scoped(&pool, chat.id, "quest", message.id).await;

        revert_message_variable_updates(&pool, chat.id, message.id, &updates)
            .await
            .expect("revert");

        let vars = db::list_variables(&pool, chat.id).await.expect("list");
        assert_eq!(vars.len(), 1);
        assert_eq!(vars[0].key, "location");
        assert_eq!(vars[0].value, "forest");
    }

    #[test]
    fn remove_variable_tags_for_keys_strips_matching_tags() {
        let keys: HashSet<&str> = ["location"].iter().copied().collect();
        let text = r#"Hello <var key="location">tavern</var> and <var key="gold">12</var>"#;
        let stripped = remove_variable_tags_for_keys(text, &keys);
        assert!(!stripped.contains("location"));
        assert!(stripped.contains(r#"<var key="gold">12</var>"#));
    }

    #[tokio::test]
    async fn strip_variable_key_from_chat_messages_removes_tags_and_updates() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("test.db");
        let pool = db::connect(&format!("sqlite:{}", path.display()))
            .await
            .expect("connect");
        let chat = test_chat(&pool).await;

        let message = db::insert_message(
            &pool,
            chat.id,
            dreamwell_types::MessageRole::Assistant,
            r#"You arrive. <var key="location">tavern</var>"#.into(),
            false,
        )
        .await
        .expect("message");
        let updates = vec![MessageVariableUpdate {
            key: "location".into(),
            value: "tavern".into(),
            previous_value: None,
        }];
        db::finalize_message_generation(
            &pool,
            message.id,
            r#"You arrive. <var key="location">tavern</var>"#,
            "",
            None,
            false,
            &updates,
        )
        .await
        .expect("finalize");

        let variable = db::upsert_variable(
            &pool,
            chat.id,
            "location".into(),
            "tavern".into(),
            crate::variable_state::MANUAL_MESSAGE_SOURCE,
        )
        .await
        .expect("seed variable");
        db::delete_variable(&pool, chat.id, variable.id)
            .await
            .expect("delete");
        strip_variable_key_from_chat_messages(&pool, chat.id, "location")
            .await
            .expect("strip");

        let updated = db::get_message(&pool, chat.id, message.id)
            .await
            .expect("message");
        assert!(!updated.content.contains("<var"));
        assert!(updated.variable_updates.is_empty());
    }

    #[tokio::test]
    async fn revert_variable_updates_from_messages_uses_message_order() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("test.db");
        let pool = db::connect(&format!("sqlite:{}", path.display()))
            .await
            .expect("connect");
        let chat = test_chat(&pool).await;

        let first = db::insert_message(
            &pool,
            chat.id,
            dreamwell_types::MessageRole::Assistant,
            "first".into(),
            false,
        )
        .await
        .expect("message");
        let second = db::insert_message(
            &pool,
            chat.id,
            dreamwell_types::MessageRole::Assistant,
            "second".into(),
            false,
        )
        .await
        .expect("message");

        let first_updates = vec![MessageVariableUpdate {
            key: "hp".into(),
            value: "80".into(),
            previous_value: None,
        }];
        let second_updates = vec![MessageVariableUpdate {
            key: "hp".into(),
            value: "50".into(),
            previous_value: Some("80".into()),
        }];
        db::finalize_message_generation(&pool, first.id, "first", "", None, false, &first_updates)
            .await
            .expect("finalize first");
        db::finalize_message_generation(
            &pool,
            second.id,
            "second",
            "",
            None,
            false,
            &second_updates,
        )
        .await
        .expect("finalize second");
        db::upsert_variable(&pool, chat.id, "hp".into(), "80".into(), first.id)
            .await
            .expect("apply first");
        db::upsert_variable(&pool, chat.id, "hp".into(), "50".into(), second.id)
            .await
            .expect("apply second");

        let messages = vec![
            db::get_message(&pool, chat.id, first.id)
                .await
                .expect("first"),
            db::get_message(&pool, chat.id, second.id)
                .await
                .expect("second"),
        ];
        revert_variable_updates_from_messages(&pool, chat.id, &messages)
            .await
            .expect("revert");

        assert!(db::list_variables(&pool, chat.id)
            .await
            .expect("list")
            .is_empty());
    }
}
