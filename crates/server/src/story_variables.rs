use dreamwell_types::StoryChapter;
use sqlx::SqlitePool;

use crate::error::AppResult;

pub use crate::variable_state::MANUAL_STORY_SOURCE;

/// Alias kept for callers that imported `MANUAL_VARIABLE_SOURCE`.
pub const MANUAL_VARIABLE_SOURCE: i64 = MANUAL_STORY_SOURCE;

/// Materialized story state entries (typed model; no beat replay).
pub async fn variables_for_beat_generation(
    pool: &SqlitePool,
    _chapters: &[StoryChapter],
    story_id: i64,
    _chapter_order: i64,
    _beat_order: i64,
) -> AppResult<Vec<(String, String)>> {
    let panel = crate::db::list_story_variables(pool, story_id).await?;
    let mut pairs: Vec<(String, String)> = panel
        .into_iter()
        .map(|variable| (variable.key, variable.value))
        .collect();
    pairs.sort_by(|left, right| left.0.cmp(&right.0));
    Ok(pairs)
}

pub fn format_story_variables(variables: &[(String, String)]) -> String {
    if variables.is_empty() {
        return String::new();
    }
    let tags: Vec<String> = variables
        .iter()
        .map(|(key, value)| format_variable_tag(key, value))
        .collect();
    format!(
        "Current story variables (use this tag format when updating):\n{}",
        tags.join("\n")
    )
}

pub fn format_tracked_details(tracked_details: &str) -> String {
    let trimmed = tracked_details.trim();
    if trimmed.is_empty() {
        return String::new();
    }
    format!("Important details to track and keep consistent:\n{trimmed}")
}

fn escape_xml_attr(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('"', "&quot;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

fn escape_xml_text(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

fn format_variable_tag(key: &str, value: &str) -> String {
    format!(
        r#"<var key="{}">{}</var>"#,
        escape_xml_attr(key),
        escape_xml_text(value)
    )
}

pub fn story_variables_instruction() -> &'static str {
    "You may update story variables using XML tags like <var key=\"baker_name\">Tomas</var>. \
     Use the key attribute (not name). Reusing a key replaces its value. Remove a variable with \
     <var key=\"quest_stage\" delete/>. Only emit var tags for concrete canon established in this \
     beat (character names, objects, locations, relationships) — not for events planned in later beats."
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn format_story_variables_uses_var_tags() {
        let formatted = format_story_variables(&[
            ("baker_name".to_string(), "Tomas".to_string()),
            ("note".to_string(), "a & b <c>".to_string()),
        ]);
        assert!(formatted.contains(r#"<var key="baker_name">Tomas</var>"#));
        assert!(formatted.contains(r#"<var key="note">a &amp; b &lt;c&gt;</var>"#));
    }

    #[test]
    fn format_tracked_details_empty_when_blank() {
        assert!(format_tracked_details("").is_empty());
        assert!(format_tracked_details("   ").is_empty());
    }

    #[test]
    fn format_tracked_details_includes_content() {
        let formatted = format_tracked_details("- protagonist name\n- the locket");
        assert!(formatted.contains("Important details to track"));
        assert!(formatted.contains("protagonist name"));
    }
}
