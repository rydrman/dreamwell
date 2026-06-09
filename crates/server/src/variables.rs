use dreamwell_types::MessageVariableUpdate;
use regex::Regex;
use sqlx::SqlitePool;

use crate::db;
use crate::error::AppResult;

pub fn extract_variables_from_text(text: &str) -> (String, Vec<(String, String)>) {
    let re =
        Regex::new(r#"(?is)<(?:var|fact)\s+key=["']([^"']+)["']\s*>(.*?)</(?:var|fact)>"#).unwrap();
    let mut updates = Vec::new();
    let cleaned = re
        .replace_all(text, |caps: &regex::Captures| {
            let key = caps.get(1).map(|m| m.as_str().trim()).unwrap_or_default();
            let value = caps.get(2).map(|m| m.as_str().trim()).unwrap_or_default();
            if !key.is_empty() {
                updates.push((key.to_string(), value.to_string()));
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
    updates: &[(String, String)],
) -> AppResult<()> {
    for (key, value) in updates {
        db::upsert_variable(pool, chat_id, key.clone(), value.clone()).await?;
    }
    Ok(())
}

pub async fn build_message_variable_updates(
    pool: &SqlitePool,
    chat_id: i64,
    updates: &[(String, String)],
) -> AppResult<Vec<MessageVariableUpdate>> {
    let mut result = Vec::with_capacity(updates.len());
    for (key, value) in updates {
        let previous_value = db::get_variable_value(pool, chat_id, key).await?;
        result.push(MessageVariableUpdate {
            key: key.clone(),
            value: value.clone(),
            previous_value,
        });
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
            vec![("location".to_string(), "tavern".to_string())]
        );
    }

    #[test]
    fn extract_legacy_fact_tags() {
        let (cleaned, updates) = extract_variables_from_text(r#"<fact key="gold">12</fact>"#);
        assert_eq!(cleaned, "");
        assert_eq!(updates, vec![("gold".to_string(), "12".to_string())]);
    }
}
