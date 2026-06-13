use std::collections::HashMap;

use dreamwell_types::{BeatVariableUpdate, ChatVariable, Message, StoryChapter, StoryVariable};

/// Manual panel entries use negative anchors so they are distinct in the unique index.
pub const MANUAL_MESSAGE_SOURCE: i64 = -1;
pub const MANUAL_STORY_SOURCE: i64 = -1;

pub fn apply_beat_updates(state: &mut HashMap<String, String>, updates: &[BeatVariableUpdate]) {
    for update in updates {
        if update.deleted {
            state.remove(&update.key);
        } else {
            state.insert(update.key.clone(), update.value.clone());
        }
    }
}

pub fn replay_story_beat_updates(
    chapters: &[StoryChapter],
    chapter_order: i64,
    beat_order: i64,
) -> HashMap<String, String> {
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
    state
}

pub fn story_panel_applies_before(
    variable: &StoryVariable,
    chapter_order: i64,
    beat_order: i64,
) -> bool {
    if variable.source_chapter_order == MANUAL_STORY_SOURCE {
        return true;
    }
    variable.source_chapter_order < chapter_order
        || (variable.source_chapter_order == chapter_order
            && variable.source_beat_order < beat_order)
}

pub fn overlay_story_panel(
    state: &mut HashMap<String, String>,
    panel: &[StoryVariable],
    chapter_order: i64,
    beat_order: i64,
) {
    let mut entries: Vec<&StoryVariable> = panel
        .iter()
        .filter(|variable| story_panel_applies_before(variable, chapter_order, beat_order))
        .collect();
    entries.sort_by_key(|variable| {
        if variable.source_chapter_order == MANUAL_STORY_SOURCE {
            (i64::MIN, i64::MIN)
        } else {
            (variable.source_chapter_order, variable.source_beat_order)
        }
    });
    for variable in entries {
        state.insert(variable.key.clone(), variable.value.clone());
    }
}

pub fn replay_chat_message_updates(
    messages: &[Message],
    before_message_id: i64,
) -> HashMap<String, String> {
    let mut state: HashMap<String, String> = HashMap::new();
    for message in messages
        .iter()
        .filter(|message| message.id < before_message_id)
    {
        for update in &message.variable_updates {
            if update.deleted {
                state.remove(&update.key);
            } else {
                state.insert(update.key.clone(), update.value.clone());
            }
        }
    }
    state
}

pub fn chat_panel_applies_before(variable: &ChatVariable, before_message_id: i64) -> bool {
    variable.source_message_id == MANUAL_MESSAGE_SOURCE
        || variable.source_message_id < before_message_id
}

pub fn overlay_chat_panel(
    state: &mut HashMap<String, String>,
    panel: &[ChatVariable],
    before_message_id: i64,
) {
    let mut entries: Vec<&ChatVariable> = panel
        .iter()
        .filter(|variable| chat_panel_applies_before(variable, before_message_id))
        .collect();
    entries.sort_by_key(|variable| variable.source_message_id);
    for variable in entries {
        state.insert(variable.key.clone(), variable.value.clone());
    }
}

pub fn chat_state_at(
    messages: &[Message],
    panel: &[ChatVariable],
    before_message_id: i64,
) -> HashMap<String, String> {
    let mut state = replay_chat_message_updates(messages, before_message_id);
    overlay_chat_panel(&mut state, panel, before_message_id);
    state
}

pub fn story_state_at(
    chapters: &[StoryChapter],
    panel: &[StoryVariable],
    chapter_order: i64,
    beat_order: i64,
) -> HashMap<String, String> {
    let mut state = replay_story_beat_updates(chapters, chapter_order, beat_order);
    overlay_story_panel(&mut state, panel, chapter_order, beat_order);
    state
}

pub fn pairs_sorted(state: HashMap<String, String>) -> Vec<(String, String)> {
    let mut pairs: Vec<(String, String)> = state.into_iter().collect();
    pairs.sort_by(|a, b| a.0.cmp(&b.0));
    pairs
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;
    use dreamwell_types::{MessageRole, StoryBeat};

    fn chat_message(id: i64, updates: Vec<BeatVariableUpdate>) -> Message {
        Message {
            id,
            chat_id: 1,
            role: MessageRole::Assistant,
            content: String::new(),
            thought_content: String::new(),
            thought_duration_ms: None,
            thought_in_progress: false,
            variable_updates: updates,
            is_summary: false,
            in_summary: false,
            created_at: Utc::now(),
            job_status: None,
            generation_error: None,
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
    fn chat_replay_uses_message_order_and_panel_scopes() {
        let messages = vec![
            chat_message(1, vec![set("location", "tavern")]),
            chat_message(2, vec![set("location", "forest")]),
        ];
        let panel = vec![
            ChatVariable {
                id: 1,
                chat_id: 1,
                key: "gold".to_string(),
                value: "10".to_string(),
                source_message_id: MANUAL_MESSAGE_SOURCE,
                updated_at: Utc::now(),
            },
            ChatVariable {
                id: 2,
                chat_id: 1,
                key: "mood".to_string(),
                value: "tense".to_string(),
                source_message_id: 2,
                updated_at: Utc::now(),
            },
        ];
        let before_second = chat_state_at(&messages, &panel, 2);
        assert_eq!(before_second.get("location"), Some(&"tavern".to_string()));
        assert!(!before_second.contains_key("mood"));

        let end = chat_state_at(&messages, &panel, i64::MAX);
        assert_eq!(end.get("location"), Some(&"forest".to_string()));
        assert_eq!(end.get("mood"), Some(&"tense".to_string()));
        assert_eq!(end.get("gold"), Some(&"10".to_string()));
    }

    #[test]
    fn story_panel_allows_same_key_at_different_beats() {
        let panel = vec![
            StoryVariable {
                id: 1,
                story_id: 1,
                key: "location".to_string(),
                value: "tavern".to_string(),
                source_chapter_order: 0,
                source_beat_order: 0,
                updated_at: Utc::now(),
            },
            StoryVariable {
                id: 2,
                story_id: 1,
                key: "location".to_string(),
                value: "forest".to_string(),
                source_chapter_order: 0,
                source_beat_order: 2,
                updated_at: Utc::now(),
            },
        ];
        let early = story_state_at(&[], &panel, 0, 1);
        assert_eq!(early.get("location"), Some(&"tavern".to_string()));

        let later = story_state_at(&[], &panel, 0, 3);
        assert_eq!(later.get("location"), Some(&"forest".to_string()));
    }

    #[test]
    fn story_replay_excludes_current_beat_updates() {
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
            beats: vec![StoryBeat {
                id: 1,
                chapter_id: 1,
                title: "Beat 1".to_string(),
                synopsis: String::new(),
                content: String::new(),
                variable_updates: vec![set("coin", "gold")],
                sort_order: 0,
                created_at: Utc::now(),
                updated_at: Utc::now(),
                job_status: None,
            }],
        }];
        let state = story_state_at(&chapters, &[], 0, 1);
        assert_eq!(state.get("coin"), Some(&"gold".to_string()));
        let current = story_state_at(&chapters, &[], 0, 0);
        assert!(!current.contains_key("coin"));
    }
}
