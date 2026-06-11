use dreamwell_types::{Message, MessageVariableUpdate};
use regex::Regex;
use sqlx::SqlitePool;

use crate::db;
use crate::error::AppResult;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum VariableUpdate {
    Set { key: String, value: String },
    Delete { key: String },
}

pub fn extract_variables_from_text(text: &str) -> (String, Vec<VariableUpdate>) {
    let delete_re = Regex::new(
        r#"(?is)<(?:var|fact)\s+key=["']([^"']+)["']\s+delete(?:\s*=\s*["']?(?:true|1)["']?)?\s*/>"#,
    )
    .unwrap();
    let set_re =
        Regex::new(r#"(?is)<(?:var|fact)\s+key=["']([^"']+)["']\s*>(.*?)</(?:var|fact)>"#).unwrap();

    let mut updates = Vec::new();
    let without_deletes = delete_re
        .replace_all(text, |caps: &regex::Captures| {
            let key = caps.get(1).map(|m| m.as_str().trim()).unwrap_or_default();
            if !key.is_empty() {
                updates.push(VariableUpdate::Delete {
                    key: key.to_string(),
                });
            }
            ""
        })
        .into_owned();
    let cleaned = set_re
        .replace_all(&without_deletes, |caps: &regex::Captures| {
            let key = caps.get(1).map(|m| m.as_str().trim()).unwrap_or_default();
            let value = caps.get(2).map(|m| m.as_str().trim()).unwrap_or_default();
            if !key.is_empty() {
                updates.push(VariableUpdate::Set {
                    key: key.to_string(),
                    value: value.to_string(),
                });
            }
            ""
        })
        .trim()
        .to_string();
    (cleaned, updates)
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
    fn extract_var_tags() {
        let (cleaned, updates) =
            extract_variables_from_text(r#"Hello <var key="location">tavern</var> world"#);
        assert_eq!(cleaned, "Hello  world");
        assert_eq!(
            updates,
            vec![VariableUpdate::Set {
                key: "location".to_string(),
                value: "tavern".to_string(),
            }]
        );
    }

    #[test]
    fn extract_legacy_fact_tags() {
        let (cleaned, updates) = extract_variables_from_text(r#"<fact key="gold">12</fact>"#);
        assert_eq!(cleaned, "");
        assert_eq!(
            updates,
            vec![VariableUpdate::Set {
                key: "gold".to_string(),
                value: "12".to_string(),
            }]
        );
    }

    #[test]
    fn extract_delete_var_tags() {
        let (cleaned, updates) =
            extract_variables_from_text(r#"Done <var key="quest_stage" delete/> here"#);
        assert_eq!(cleaned, "Done  here");
        assert_eq!(
            updates,
            vec![VariableUpdate::Delete {
                key: "quest_stage".to_string(),
            }]
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

    #[test]
    fn extract_delete_var_tags_with_value() {
        let (cleaned, updates) = extract_variables_from_text(
            r#"Reset <var key="temp_buff" delete="true"/> and <var key="hp">50</var>"#,
        );
        assert_eq!(cleaned, "Reset  and");
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
}
