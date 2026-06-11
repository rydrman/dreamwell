use dreamwell_types::MessageVariableUpdate;
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
