use dreamwell_types::{Message, MessageVariableUpdate};
use regex::Regex;
use sqlx::SqlitePool;
use std::sync::OnceLock;

use crate::db;
use crate::error::AppResult;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum VariableUpdate {
    Set { key: String, value: String },
    Delete { key: String },
}

/// Parses variable updates from model output without mutating the stored text.
pub fn parse_variable_updates(text: &str) -> Vec<VariableUpdate> {
    let mut updates = Vec::new();
    let mut working = text.to_string();
    working = extract_delete_tags(&working, &mut updates);
    working = extract_set_value_tags(&working, &mut updates);
    extract_set_tags(&working, &mut updates);
    updates
}

/// Visible reply text after variable tags are removed. Used only for validation.
pub(crate) fn visible_text_without_variables(text: &str) -> String {
    strip_variable_markup(text, false)
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
            Regex::new(
                r#"(?is)<(?:var|fact)\s+key=["']([^"']+)["'][^>]*\bdelete\b(?:\s*=\s*["']?(?:true|1)["']?)?[^>]*/>"#,
            )
            .expect("delete self-closing regex"),
            Regex::new(
                r#"(?is)<(?:var|fact)\s+key=["']([^"']+)["'][^>]*\bdelete\b[^>]*>\s*</(?:var|fact)>"#,
            )
            .expect("delete empty element regex"),
        ]
    })
}

fn set_value_pattern() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(
            r#"(?is)<(?:var|fact)\s+key=["']([^"']+)["'][^>]*\bvalue=["']([^"']*)["'][^>]*/>"#,
        )
        .expect("set value self-closing regex")
    })
}

fn set_pattern() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(r#"(?is)<(?:var|fact)\s+key=["']([^"']+)["'][^>]*>(.*?)</(?:var|fact)>"#)
            .expect("set regex")
    })
}

fn extract_delete_tags(text: &str, updates: &mut Vec<VariableUpdate>) -> String {
    let mut working = text.to_string();
    for re in delete_patterns() {
        working = re
            .replace_all(&working, |caps: &regex::Captures| {
                push_delete_update(updates, caps.get(1).map(|m| m.as_str()).unwrap_or_default());
                ""
            })
            .into_owned();
    }
    working
}

fn extract_set_value_tags(text: &str, updates: &mut Vec<VariableUpdate>) -> String {
    set_value_pattern()
        .replace_all(text, |caps: &regex::Captures| {
            let key = caps.get(1).map(|m| m.as_str().trim()).unwrap_or_default();
            let value = caps.get(2).map(|m| m.as_str().trim()).unwrap_or_default();
            push_set_update(updates, key, value);
            ""
        })
        .into_owned()
}

fn extract_set_tags(text: &str, updates: &mut Vec<VariableUpdate>) -> String {
    set_pattern()
        .replace_all(text, |caps: &regex::Captures| {
            let key = caps.get(1).map(|m| m.as_str().trim()).unwrap_or_default();
            let value = caps.get(2).map(|m| m.as_str().trim()).unwrap_or_default();
            push_set_update(updates, key, value);
            ""
        })
        .into_owned()
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

fn push_delete_update(updates: &mut Vec<VariableUpdate>, key: &str) {
    let key = key.trim();
    if !key.is_empty() {
        updates.push(VariableUpdate::Delete {
            key: key.to_string(),
        });
    }
}

fn push_set_update(updates: &mut Vec<VariableUpdate>, key: &str, value: &str) {
    let key = key.trim();
    if !key.is_empty() {
        updates.push(VariableUpdate::Set {
            key: key.to_string(),
            value: value.to_string(),
        });
    }
}

fn strip_orphan_closing_tags(text: &str) -> String {
    static RE: OnceLock<Regex> = OnceLock::new();
    let re = RE.get_or_init(|| Regex::new(r"(?is)</(?:var|fact)\s*>").expect("orphan close regex"));
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

    for open in ["<var", "<fact"] {
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

    lower.contains("</var>") || lower.contains("</fact>")
}

fn trailing_partial_var_prefix(text: &str) -> usize {
    const PREFIXES: &[&str] = &["</fact>", "</var>", "<fact", "<var", "</", "<"];
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

pub async fn apply_variable_updates(
    pool: &SqlitePool,
    chat_id: i64,
    updates: &[VariableUpdate],
) -> AppResult<()> {
    for update in updates {
        match update {
            VariableUpdate::Set { key, value } => {
                db::upsert_variable(pool, chat_id, key.clone(), value.clone()).await?;
            }
            VariableUpdate::Delete { key } => {
                let _ = db::delete_variable(pool, chat_id, key).await;
            }
        }
    }
    Ok(())
}

pub async fn build_message_variable_updates(
    pool: &SqlitePool,
    chat_id: i64,
    updates: &[VariableUpdate],
) -> AppResult<Vec<MessageVariableUpdate>> {
    let mut result = Vec::with_capacity(updates.len());
    for update in updates {
        match update {
            VariableUpdate::Set { key, value } => {
                let previous_value = db::get_variable_value(pool, chat_id, key).await?;
                result.push(MessageVariableUpdate {
                    key: key.clone(),
                    value: value.clone(),
                    previous_value,
                    deleted: false,
                });
            }
            VariableUpdate::Delete { key } => {
                let previous_value = db::get_variable_value(pool, chat_id, key).await?;
                result.push(MessageVariableUpdate {
                    key: key.clone(),
                    value: String::new(),
                    previous_value,
                    deleted: true,
                });
            }
        }
    }
    Ok(result)
}

pub async fn revert_message_variable_updates(
    pool: &SqlitePool,
    chat_id: i64,
    updates: &[MessageVariableUpdate],
) -> AppResult<()> {
    for update in updates.iter().rev() {
        match &update.previous_value {
            Some(previous) => {
                db::upsert_variable(pool, chat_id, update.key.clone(), previous.clone()).await?;
            }
            None => {
                let _ = db::delete_variable(pool, chat_id, &update.key).await;
            }
        }
    }
    Ok(())
}

pub async fn revert_variable_updates_from_messages(
    pool: &SqlitePool,
    chat_id: i64,
    messages: &[Message],
) -> AppResult<()> {
    for message in messages.iter().rev() {
        revert_message_variable_updates(pool, chat_id, &message.variable_updates).await?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_var_tags() {
        let updates = parse_variable_updates(r#"Hello <var key="location">tavern</var> world"#);
        assert_eq!(
            updates,
            vec![VariableUpdate::Set {
                key: "location".to_string(),
                value: "tavern".to_string(),
            }]
        );
    }

    #[test]
    fn parse_legacy_fact_tags() {
        let updates = parse_variable_updates(r#"<fact key="gold">12</fact>"#);
        assert_eq!(
            updates,
            vec![VariableUpdate::Set {
                key: "gold".to_string(),
                value: "12".to_string(),
            }]
        );
    }

    #[test]
    fn parse_delete_var_tags() {
        let updates = parse_variable_updates(r#"Done <var key="quest_stage" delete/> here"#);
        assert_eq!(
            updates,
            vec![VariableUpdate::Delete {
                key: "quest_stage".to_string(),
            }]
        );
    }

    #[test]
    fn parse_delete_var_tags_with_value() {
        let updates = parse_variable_updates(
            r#"Reset <var key="temp_buff" delete="true"/> and <var key="hp">50</var>"#,
        );
        assert_eq!(
            updates,
            vec![
                VariableUpdate::Delete {
                    key: "temp_buff".to_string(),
                },
                VariableUpdate::Set {
                    key: "hp".to_string(),
                    value: "50".to_string(),
                },
            ]
        );
    }

    #[test]
    fn parses_self_closing_value_tags() {
        let updates = parse_variable_updates(r#"Hi <var key="hp" value="50"/> there"#);
        assert_eq!(
            updates,
            vec![VariableUpdate::Set {
                key: "hp".to_string(),
                value: "50".to_string(),
            }]
        );
    }

    #[test]
    fn parses_delete_empty_element_tags() {
        let updates = parse_variable_updates(r#"Done <var key="quest" delete></var> now"#);
        assert_eq!(
            updates,
            vec![VariableUpdate::Delete {
                key: "quest".to_string(),
            }]
        );
    }

    #[test]
    fn ignores_incomplete_tags() {
        let updates = parse_variable_updates(r#"Visible only <var key="hp">50</var"#);
        assert!(updates.is_empty());
    }

    #[test]
    fn is_case_insensitive_for_tags() {
        let updates = parse_variable_updates(r#"<VAR key="x">y</VAR>"#);
        assert_eq!(
            updates,
            vec![VariableUpdate::Set {
                key: "x".to_string(),
                value: "y".to_string(),
            }]
        );
    }

    #[test]
    fn visible_text_strips_tags_for_validation() {
        assert_eq!(
            visible_text_without_variables(
                r#"*narrates* <var key="hp">80</var>
</var>"#
            ),
            "*narrates*"
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

        db::upsert_variable(&pool, chat.id, "location".into(), "forest".into())
            .await
            .expect("seed variable");

        let updates = vec![
            MessageVariableUpdate {
                key: "location".into(),
                value: "tavern".into(),
                previous_value: Some("forest".into()),
                deleted: false,
            },
            MessageVariableUpdate {
                key: "quest".into(),
                value: String::new(),
                previous_value: None,
                deleted: true,
            },
        ];
        apply_variable_updates(
            &pool,
            chat.id,
            &[
                VariableUpdate::Set {
                    key: "location".into(),
                    value: "tavern".into(),
                },
                VariableUpdate::Delete {
                    key: "quest".into(),
                },
            ],
        )
        .await
        .expect("apply");

        revert_message_variable_updates(&pool, chat.id, &updates)
            .await
            .expect("revert");

        let vars = db::list_variables(&pool, chat.id).await.expect("list");
        assert_eq!(vars.len(), 1);
        assert_eq!(vars[0].key, "location");
        assert_eq!(vars[0].value, "forest");
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
            deleted: false,
        }];
        let second_updates = vec![MessageVariableUpdate {
            key: "hp".into(),
            value: "50".into(),
            previous_value: Some("80".into()),
            deleted: false,
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
        apply_variable_updates(
            &pool,
            chat.id,
            &[VariableUpdate::Set {
                key: "hp".into(),
                value: "80".into(),
            }],
        )
        .await
        .expect("apply first");
        apply_variable_updates(
            &pool,
            chat.id,
            &[VariableUpdate::Set {
                key: "hp".into(),
                value: "50".into(),
            }],
        )
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
