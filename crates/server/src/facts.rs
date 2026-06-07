use regex::Regex;
use sqlx::SqlitePool;

use crate::db;
use crate::error::AppResult;

pub fn extract_facts_from_text(text: &str) -> (String, Vec<(String, String)>) {
    let re = Regex::new(r#"(?is)<fact\s+key=["']([^"']+)["']\s*>(.*?)</fact>"#).unwrap();
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

pub async fn apply_fact_updates(
    pool: &SqlitePool,
    chat_id: i64,
    updates: &[(String, String)],
) -> AppResult<()> {
    for (key, value) in updates {
        db::upsert_fact(pool, chat_id, key.clone(), value.clone()).await?;
    }
    Ok(())
}
