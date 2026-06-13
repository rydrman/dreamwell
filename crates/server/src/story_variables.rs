use std::collections::HashMap;

use dreamwell_types::{BeatVariableUpdate, StoryChapter, StoryVariable};
use sqlx::SqlitePool;

use crate::error::AppResult;
use crate::variables::VariableUpdate;

/// Manual panel edits use negative source positions so they apply at every beat.
pub const MANUAL_VARIABLE_SOURCE: i64 = -1;

/// Variables available when generating prose for a beat (prior beats + manual overrides).
pub async fn variables_for_beat_generation(
    pool: &SqlitePool,
    chapters: &[StoryChapter],
    story_id: i64,
    chapter_order: i64,
    beat_order: i64,
) -> AppResult<Vec<(String, String)>> {
    let mut state: HashMap<String, String> =
        replay_variables_before(chapters, chapter_order, beat_order)
            .into_iter()
            .collect();
    for variable in crate::db::list_story_variables(pool, story_id).await? {
        if variable.source_chapter_order == MANUAL_VARIABLE_SOURCE {
            state.insert(variable.key, variable.value);
        }
    }
    let mut pairs: Vec<(String, String)> = state.into_iter().collect();
    pairs.sort_by(|a, b| a.0.cmp(&b.0));
    Ok(pairs)
}

/// Variables known strictly before the given beat position (prior beats only).
pub fn replay_variables_before(
    chapters: &[StoryChapter],
    chapter_order: i64,
    beat_order: i64,
) -> Vec<(String, String)> {
    let mut state: HashMap<String, String> = HashMap::new();
    for chapter in chapters
        .iter()
        .filter(|chapter| chapter.sort_order < chapter_order)
    {
        for beat in &chapter.beats {
            apply_beat_updates(&mut state, &beat.variable_updates);
        }
    }
    if let Some(chapter) = chapters
        .iter()
        .find(|chapter| chapter.sort_order == chapter_order)
    {
        for beat in chapter
            .beats
            .iter()
            .filter(|beat| beat.sort_order < beat_order)
        {
            apply_beat_updates(&mut state, &beat.variable_updates);
        }
    }
    let mut pairs: Vec<(String, String)> = state.into_iter().collect();
    pairs.sort_by(|a, b| a.0.cmp(&b.0));
    pairs
}

fn apply_beat_updates(state: &mut HashMap<String, String>, updates: &[BeatVariableUpdate]) {
    for update in updates {
        if update.deleted {
            state.remove(&update.key);
        } else {
            state.insert(update.key.clone(), update.value.clone());
        }
    }
}

pub fn filter_meaningful_story_updates(
    updates: &[VariableUpdate],
    current_variables: &[StoryVariable],
) -> Vec<VariableUpdate> {
    updates
        .iter()
        .filter(|update| match update {
            VariableUpdate::Set { key, value } => current_variables
                .iter()
                .find(|variable| variable.key == *key)
                .map(|variable| variable.value != *value)
                .unwrap_or(true),
            VariableUpdate::Delete { key } => current_variables
                .iter()
                .any(|variable| variable.key == *key),
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
    use dreamwell_types::StoryBeat;

    fn beat(order: i64, updates: Vec<BeatVariableUpdate>) -> StoryBeat {
        StoryBeat {
            id: order + 1,
            chapter_id: 1,
            title: format!("Beat {}", order + 1),
            synopsis: String::new(),
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
            deleted: false,
        }
    }

    #[test]
    fn replay_excludes_current_and_later_beats() {
        let chapters = vec![StoryChapter {
            id: 1,
            story_id: 1,
            title: "Chapter 1".to_string(),
            synopsis: String::new(),
            sort_order: 0,
            created_at: Utc::now(),
            updated_at: Utc::now(),
            beats: vec![
                beat(0, vec![set("baker", "Tomas")]),
                beat(1, vec![set("bread", "rye")]),
                beat(2, vec![set("coin", "gold")]),
            ],
        }];
        let vars = replay_variables_before(&chapters, 0, 1);
        assert_eq!(vars, vec![("baker".to_string(), "Tomas".to_string()),]);
    }
}
