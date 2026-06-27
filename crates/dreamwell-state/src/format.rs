use std::collections::HashMap;

use dreamwell_types::{SequencePayload, SessionActor, StateEntry, StateKind};

fn actor_label(actor_id: Option<i64>, actors: &[SessionActor]) -> Option<String> {
    actor_id.and_then(|id| actors.iter().find(|a| a.id == id).map(|a| a.name.clone()))
}

pub fn build_state_block(state: &[StateEntry], actors: &[SessionActor]) -> String {
    build_state_block_annotated(state, actors, &HashMap::new())
}

/// Like [`build_state_block`], but appends an authoring note (e.g. a scenario var's
/// description) after each matching entry's value line.
pub fn build_state_block_annotated(
    state: &[StateEntry],
    actors: &[SessionActor],
    annotations: &HashMap<(Option<i64>, String), String>,
) -> String {
    let mut lines = Vec::new();
    let mut by_actor: HashMap<Option<i64>, Vec<&StateEntry>> = HashMap::new();
    for entry in state {
        by_actor.entry(entry.actor_id).or_default().push(entry);
    }

    if let Some(world) = by_actor.remove(&None) {
        lines.push("World:".to_string());
        append_state_entries(&mut lines, &world, annotations);
    }

    for actor in actors {
        if let Some(entries) = by_actor.remove(&Some(actor.id)) {
            lines.push(format!("{}:", actor.name));
            if !actor.skills.is_empty() {
                let skills: Vec<String> = actor
                    .skills
                    .iter()
                    .map(|(name, value)| format!("{name}: {value:+}"))
                    .collect();
                lines.push(format!("  Skills: {}", skills.join(", ")));
            }
            append_state_entries(&mut lines, &entries, annotations);
        }
    }

    for (actor_id, entries) in by_actor {
        let label = actor_label(actor_id, actors).unwrap_or_else(|| "Unknown".to_string());
        lines.push(format!("{label}:"));
        append_state_entries(&mut lines, &entries, annotations);
    }
    lines.join("\n")
}

fn append_state_entries(
    lines: &mut Vec<String>,
    entries: &[&StateEntry],
    annotations: &HashMap<(Option<i64>, String), String>,
) {
    for entry in entries {
        let mut line = match entry.kind {
            StateKind::Measurement => {
                let value = entry.float_value.unwrap_or(0.0);
                let bounds = match (entry.float_min, entry.float_max) {
                    (Some(min), Some(max)) => format!(" ({}–{})", min, max),
                    (None, Some(max)) => format!(" (≤{})", max),
                    (Some(min), None) => format!(" (≥{})", min),
                    (None, None) => String::new(),
                };
                let unit = entry
                    .unit
                    .as_deref()
                    .filter(|u| !u.is_empty())
                    .map(|u| format!(" {}", u))
                    .unwrap_or_default();
                format!("- {} (measurement): {}{}{}", entry.key, value, unit, bounds)
            }
            StateKind::Sequence => {
                if let Some(seq) = SequencePayload::decode(&entry.value) {
                    let active = seq.active_item().unwrap_or("?");
                    let items = seq.items.join(", ");
                    let loop_tag = if seq.r#loop { " loop" } else { "" };
                    format!(
                        "- {} (sequence): {} [{}]{}",
                        entry.key, active, items, loop_tag
                    )
                } else {
                    format!("- {} (sequence): (invalid)", entry.key)
                }
            }
            StateKind::Condition => {
                format!("- {} (condition): {}", entry.key, entry.value)
            }
            StateKind::Variable => {
                format!("- {} (variable): {}", entry.key, entry.value)
            }
        };
        if let Some(note) = annotations
            .get(&(entry.actor_id, entry.key.clone()))
            .map(|s| s.trim())
            .filter(|s| !s.is_empty())
        {
            line.push_str(&format!(" [{note}]"));
        }
        lines.push(line);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;

    #[test]
    fn build_state_block_includes_measurement_and_sequence() {
        let actor = SessionActor {
            id: 1,
            game_id: 1,
            role: "pc".to_string(),
            name: "Alex".to_string(),
            description: "A thief".to_string(),
            skills: [("Finesse".to_string(), 1)].into_iter().collect(),
            sort_order: 0,
            created_at: Utc::now(),
            updated_at: Utc::now(),
        };
        let state = vec![
            StateEntry {
                id: 1,
                game_id: 1,
                actor_id: Some(1),
                kind: StateKind::Measurement,
                key: "stress".to_string(),
                value: String::new(),
                num_value: None,
                max_value: None,
                float_value: Some(2.5),
                float_min: None,
                float_max: Some(5.0),
                unit: Some("stress".to_string()),
                source_turn: 1,
                updated_at: Utc::now(),
            },
            StateEntry {
                id: 2,
                game_id: 1,
                actor_id: None,
                kind: StateKind::Sequence,
                key: "turn".to_string(),
                value: r#"{"items":["pc","maya"],"position":0,"loop":true}"#.to_string(),
                num_value: None,
                max_value: None,
                float_value: None,
                float_min: None,
                float_max: None,
                unit: None,
                source_turn: 1,
                updated_at: Utc::now(),
            },
        ];
        let block = build_state_block(&state, &[actor]);
        assert!(block.contains("stress (measurement): 2.5"));
        assert!(block.contains("turn (sequence): pc"));
    }
}
