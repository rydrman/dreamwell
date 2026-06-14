use dreamwell_types::StoryChapter;
use sqlx::SqlitePool;

use crate::error::AppResult;
use crate::variable_state::{pairs_sorted, story_state_at};
use crate::variables::VariableUpdate;

pub use crate::variable_state::MANUAL_STORY_SOURCE;

/// Alias kept for callers that imported `MANUAL_VARIABLE_SOURCE`.
pub const MANUAL_VARIABLE_SOURCE: i64 = MANUAL_STORY_SOURCE;

/// Variables available when generating prose for a beat (prior beats + scoped panel entries).
pub async fn variables_for_beat_generation(
    pool: &SqlitePool,
    chapters: &[StoryChapter],
    story_id: i64,
    chapter_order: i64,
    beat_order: i64,
) -> AppResult<Vec<(String, String)>> {
    let panel = crate::db::list_story_variables(pool, story_id).await?;
    Ok(pairs_sorted(story_state_at(
        chapters,
        &panel,
        chapter_order,
        beat_order,
    )))
}

pub fn filter_meaningful_story_updates(
    updates: &[VariableUpdate],
    current_variables: &[(String, String)],
) -> Vec<VariableUpdate> {
    let current: std::collections::HashMap<_, _> = current_variables
        .iter()
        .map(|(key, value)| (key.clone(), value.clone()))
        .collect();
    updates
        .iter()
        .filter(|update| match update {
            VariableUpdate::Set { key, value } if value.is_empty() => current.contains_key(key),
            VariableUpdate::Set { key, value } => current.get(key) != Some(value),
            VariableUpdate::Delete { key } => current.contains_key(key),
        })
        .cloned()
        .collect()
}

pub fn format_story_variables(variables: &[(String, String)]) -> String {
    if variables.is_empty() {
        return String::new();
    }
    let lines: Vec<String> = variables
        .iter()
        .map(|(key, value)| format!("- {key}: {value}"))
        .collect();
    format!("Current story variables:\n{}", lines.join("\n"))
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
    use chrono::Utc;
    use dreamwell_types::{BeatVariableUpdate, StoryBeat};

    fn beat(order: i64, updates: Vec<BeatVariableUpdate>) -> StoryBeat {
        StoryBeat {
            id: order + 1,
            chapter_id: 1,
            title: format!("Beat {}", order + 1),
            synopsis: String::new(),
            mechanical: String::new(),
            content: String::new(),
            variable_updates: updates,
            sort_order: order,
            created_at: Utc::now(),
            updated_at: Utc::now(),
            job_status: None,
        }
    }

    fn set(key: &str, value: &str) -> BeatVariableUpdate {
        BeatVariableUpdate {
            key: key.to_string(),
            value: value.to_string(),
            previous_value: None,
        }
    }

    #[test]
    fn replay_excludes_current_and_later_beats() {
        let chapters = vec![StoryChapter {
            id: 1,
            story_id: 1,
            title: "Chapter 1".to_string(),
            synopsis: String::new(),
            prose_summary: String::new(),
            prose_summary_valid: false,
            prose_summary_at: None,
            sort_order: 0,
            created_at: Utc::now(),
            updated_at: Utc::now(),
            beats: vec![
                beat(0, vec![set("baker", "Tomas")]),
                beat(1, vec![set("bread", "rye")]),
                beat(2, vec![set("coin", "gold")]),
            ],
        }];
        let vars = crate::variable_state::replay_story_beat_updates(&chapters, 0, 1);
        assert_eq!(vars.get("baker"), Some(&"Tomas".to_string()));
        assert!(!vars.contains_key("bread"));
    }
}
