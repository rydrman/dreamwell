use std::collections::HashMap;

use dreamwell_types::{SessionActor, StateEntry, StateKind};

pub fn build_state_block(state: &[StateEntry], actors: &[SessionActor]) -> String {
    build_state_block_annotated(state, actors, &HashMap::new())
}

/// Like [`build_state_block`], but appends an authoring note (e.g. a scenario var's
/// update hint) to each entry whose `(actor_id, key)` is present in `annotations`.
/// This lets scenario-authored metadata ride alongside the live value instead of
/// being presented as a separate "schema" block.
pub fn build_state_block_annotated(
    state: &[StateEntry],
    actors: &[SessionActor],
    annotations: &HashMap<(Option<i64>, String), String>,
) -> String {
    let mut lines = Vec::new();
    for actor in actors {
        let actor_state: Vec<_> = state
            .iter()
            .filter(|e| e.actor_id == Some(actor.id))
            .collect();
        if actor_state.is_empty() {
            continue;
        }
        lines.push(format!("## {} ({})", actor.name, actor.role));
        if !actor.description.is_empty() {
            lines.push(actor.description.clone());
        }
        if !actor.skills.is_empty() {
            let skills: Vec<_> = actor
                .skills
                .iter()
                .map(|(k, v)| format!("{k}: {v:+}"))
                .collect();
            lines.push(format!("Skills: {}", skills.join(", ")));
        }
        append_state_entries(&mut lines, &actor_state, annotations);
    }
    let world_state: Vec<_> = state.iter().filter(|e| e.actor_id.is_none()).collect();
    if !world_state.is_empty() {
        lines.push("## World".to_string());
        append_state_entries(&mut lines, &world_state, annotations);
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
            StateKind::Resource => {
                let current = entry.num_value.unwrap_or(0);
                let max = entry.max_value.unwrap_or(current);
                format!("- {} (resource): {}/{}", entry.key, current, max)
            }
            StateKind::Clock => {
                let current = entry.num_value.unwrap_or(0);
                let segments = entry.max_value.unwrap_or(4);
                format!("- {} (clock): {}/{}", entry.key, current, segments)
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
    fn build_state_block_includes_actor_and_skills() {
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
        let state = vec![StateEntry {
            id: 1,
            game_id: 1,
            actor_id: Some(1),
            kind: StateKind::Resource,
            key: "stress".to_string(),
            value: String::new(),
            num_value: Some(2),
            max_value: Some(5),
            source_turn: 1,
            updated_at: Utc::now(),
        }];
        let block = build_state_block(&state, &[actor]);
        assert!(block.contains("Alex"));
        assert!(block.contains("Finesse: +1"));
        assert!(block.contains("stress"));
    }
}
