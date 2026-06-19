use std::collections::HashMap;

use dreamwell_types::{
    AppliedStateChange, GameActor, GameStateEntry, StateChangeRequest, StateKind, StateOp,
};
use sqlx::SqlitePool;

use crate::error::AppResult;

pub fn build_state_block(state: &[GameStateEntry], actors: &[GameActor]) -> String {
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
        append_state_entries(&mut lines, &actor_state);
    }
    let world_state: Vec<_> = state.iter().filter(|e| e.actor_id.is_none()).collect();
    if !world_state.is_empty() {
        lines.push("## World".to_string());
        append_state_entries(&mut lines, &world_state);
    }
    lines.join("\n")
}

fn append_state_entries(lines: &mut Vec<String>, entries: &[&GameStateEntry]) {
    for entry in entries {
        match entry.kind {
            StateKind::Resource => {
                let current = entry.num_value.unwrap_or(0);
                let max = entry.max_value.unwrap_or(current);
                lines.push(format!("- {} (resource): {}/{}", entry.key, current, max));
            }
            StateKind::Clock => {
                let current = entry.num_value.unwrap_or(0);
                let segments = entry.max_value.unwrap_or(4);
                lines.push(format!("- {} (clock): {}/{}", entry.key, current, segments));
            }
            StateKind::Condition => {
                lines.push(format!("- {} (condition): {}", entry.key, entry.value));
            }
            StateKind::Fact => {
                lines.push(format!("- {} (fact): {}", entry.key, entry.value));
            }
        }
    }
}

pub fn resolve_actor_id(target: &str, actors: &[GameActor]) -> Option<i64> {
    match target {
        "world" => None,
        "pc" => actors.iter().find(|a| a.role == "pc").map(|a| a.id),
        other => actors
            .iter()
            .find(|a| a.role == other || a.name.eq_ignore_ascii_case(other))
            .map(|a| a.id),
    }
}

pub fn validate_skill(skill: &str, actor: &GameActor) -> String {
    if actor.skills.contains_key(skill) {
        skill.to_string()
    } else {
        // Allow freeform skills; default modifier from sheet is 0
        skill.to_string()
    }
}

pub fn skill_modifier(skill: &str, actor: &GameActor) -> i64 {
    actor.skills.get(skill).copied().unwrap_or(0)
}

pub async fn apply_state_changes(
    pool: &SqlitePool,
    game_id: i64,
    turn_id: i64,
    changes: &[StateChangeRequest],
    actors: &[GameActor],
    current: &[GameStateEntry],
) -> AppResult<Vec<AppliedStateChange>> {
    let mut index: HashMap<(Option<i64>, String, StateKind), &GameStateEntry> = HashMap::new();
    for entry in current {
        index.insert((entry.actor_id, entry.key.clone(), entry.kind), entry);
    }

    let mut applied = Vec::new();
    for change in changes {
        let actor_id = if change.target == "world" {
            None
        } else {
            resolve_actor_id(&change.target, actors)
        };
        let key = (actor_id, change.key.clone(), change.kind);
        let existing = index.get(&key).copied();

        let applied_change = match change.kind {
            StateKind::Resource | StateKind::Clock => {
                apply_numeric_change(pool, game_id, turn_id, change, actor_id, existing).await?
            }
            StateKind::Condition | StateKind::Fact => {
                apply_text_change(pool, game_id, turn_id, change, actor_id, existing).await?
            }
        };
        applied.push(applied_change);
    }
    Ok(applied)
}

async fn apply_numeric_change(
    pool: &SqlitePool,
    game_id: i64,
    turn_id: i64,
    change: &StateChangeRequest,
    actor_id: Option<i64>,
    existing: Option<&GameStateEntry>,
) -> AppResult<AppliedStateChange> {
    let prev_num = existing.and_then(|e| e.num_value);
    let prev_value = existing.map(|e| e.value.clone());
    let max = existing
        .and_then(|e| e.max_value)
        .or_else(|| change.delta.map(|_| 5))
        .unwrap_or(5);

    let current = prev_num.unwrap_or(0);
    let new_num = match change.op {
        StateOp::Set => change.delta.unwrap_or(change.num_value_from_value()),
        StateOp::Add => current + change.delta.unwrap_or(0),
        StateOp::Remove => 0,
    };
    let clamped = new_num.clamp(0, max);
    let now = chrono::Utc::now().to_rfc3339();
    let kind_str = state_kind_str(change.kind);

    if let Some(entry) = existing {
        sqlx::query(
            "UPDATE game_state_entries SET num_value=?1, max_value=?2, source_turn=?3, updated_at=?4 WHERE id=?5",
        )
        .bind(clamped)
        .bind(max)
        .bind(turn_id)
        .bind(&now)
        .bind(entry.id)
        .execute(pool)
        .await?;
    } else {
        sqlx::query(
            "INSERT INTO game_state_entries (game_id, actor_id, kind, key, value, num_value, max_value, source_turn, updated_at) VALUES (?1,?2,?3,?4,'',?5,?6,?7,?8)",
        )
        .bind(game_id)
        .bind(actor_id)
        .bind(kind_str)
        .bind(&change.key)
        .bind(clamped)
        .bind(max)
        .bind(turn_id)
        .bind(&now)
        .execute(pool)
        .await?;
    }

    Ok(AppliedStateChange {
        target: change.target.clone(),
        kind: change.kind,
        key: change.key.clone(),
        op: change.op,
        value: None,
        delta: change.delta,
        prev_value,
        prev_num,
    })
}

async fn apply_text_change(
    pool: &SqlitePool,
    game_id: i64,
    turn_id: i64,
    change: &StateChangeRequest,
    actor_id: Option<i64>,
    existing: Option<&GameStateEntry>,
) -> AppResult<AppliedStateChange> {
    let prev_value = existing.map(|e| e.value.clone());
    let new_value = match change.op {
        StateOp::Set => change.value.clone().unwrap_or_default(),
        StateOp::Add => format!(
            "{}{}",
            prev_value.as_deref().unwrap_or(""),
            change.value.as_deref().unwrap_or("")
        ),
        StateOp::Remove => String::new(),
    };
    let now = chrono::Utc::now().to_rfc3339();
    let kind_str = state_kind_str(change.kind);

    if change.op == StateOp::Remove && new_value.is_empty() {
        if let Some(entry) = existing {
            sqlx::query("DELETE FROM game_state_entries WHERE id = ?1")
                .bind(entry.id)
                .execute(pool)
                .await?;
        }
    } else if let Some(entry) = existing {
        sqlx::query(
            "UPDATE game_state_entries SET value=?1, source_turn=?2, updated_at=?3 WHERE id=?4",
        )
        .bind(&new_value)
        .bind(turn_id)
        .bind(&now)
        .bind(entry.id)
        .execute(pool)
        .await?;
    } else {
        sqlx::query(
            "INSERT INTO game_state_entries (game_id, actor_id, kind, key, value, source_turn, updated_at) VALUES (?1,?2,?3,?4,?5,?6,?7)",
        )
        .bind(game_id)
        .bind(actor_id)
        .bind(kind_str)
        .bind(&change.key)
        .bind(&new_value)
        .bind(turn_id)
        .bind(&now)
        .execute(pool)
        .await?;
    }

    Ok(AppliedStateChange {
        target: change.target.clone(),
        kind: change.kind,
        key: change.key.clone(),
        op: change.op,
        value: Some(new_value),
        delta: None,
        prev_value,
        prev_num: None,
    })
}

pub async fn revert_turn_state_changes(
    pool: &SqlitePool,
    game_id: i64,
    turn_id: i64,
    changes: &[AppliedStateChange],
    actors: &[GameActor],
) -> AppResult<()> {
    for change in changes.iter().rev() {
        let actor_id = if change.target == "world" {
            None
        } else {
            resolve_actor_id(&change.target, actors)
        };
        let kind_str = state_kind_str(change.kind);
        let now = chrono::Utc::now().to_rfc3339();

        match change.kind {
            StateKind::Resource | StateKind::Clock => {
                if let Some(prev) = change.prev_num {
                    sqlx::query(
                        "UPDATE game_state_entries SET num_value=?1, source_turn=-1, updated_at=?2 WHERE game_id=?3 AND actor_id IS ?4 AND kind=?5 AND key=?6",
                    )
                    .bind(prev)
                    .bind(&now)
                    .bind(game_id)
                    .bind(actor_id)
                    .bind(kind_str)
                    .bind(&change.key)
                    .execute(pool)
                    .await?;
                } else {
                    sqlx::query(
                        "DELETE FROM game_state_entries WHERE game_id=?1 AND actor_id IS ?2 AND kind=?3 AND key=?4",
                    )
                    .bind(game_id)
                    .bind(actor_id)
                    .bind(kind_str)
                    .bind(&change.key)
                    .execute(pool)
                    .await?;
                }
            }
            StateKind::Condition | StateKind::Fact => {
                if let Some(prev) = &change.prev_value {
                    sqlx::query(
                        "UPDATE game_state_entries SET value=?1, source_turn=-1, updated_at=?2 WHERE game_id=?3 AND actor_id IS ?4 AND kind=?5 AND key=?6",
                    )
                    .bind(prev)
                    .bind(&now)
                    .bind(game_id)
                    .bind(actor_id)
                    .bind(kind_str)
                    .bind(&change.key)
                    .execute(pool)
                    .await?;
                } else {
                    sqlx::query(
                        "DELETE FROM game_state_entries WHERE game_id=?1 AND actor_id IS ?2 AND kind=?3 AND key=?4",
                    )
                    .bind(game_id)
                    .bind(actor_id)
                    .bind(kind_str)
                    .bind(&change.key)
                    .execute(pool)
                    .await?;
                }
            }
        }
    }
    let _ = turn_id;
    Ok(())
}

fn state_kind_str(kind: StateKind) -> &'static str {
    match kind {
        StateKind::Resource => "resource",
        StateKind::Condition => "condition",
        StateKind::Fact => "fact",
        StateKind::Clock => "clock",
    }
}

trait StateChangeValue {
    fn num_value_from_value(&self) -> i64;
}

impl StateChangeValue for StateChangeRequest {
    fn num_value_from_value(&self) -> i64 {
        self.value
            .as_deref()
            .and_then(|v| v.parse().ok())
            .unwrap_or(0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;

    fn sample_actor() -> GameActor {
        GameActor {
            id: 1,
            game_id: 1,
            role: "pc".to_string(),
            name: "Alex".to_string(),
            description: "A thief".to_string(),
            skills: [("Finesse".to_string(), 1)].into_iter().collect(),
            sort_order: 0,
            created_at: Utc::now(),
            updated_at: Utc::now(),
        }
    }

    #[test]
    fn build_state_block_includes_actor_and_skills() {
        let actor = sample_actor();
        let state = vec![GameStateEntry {
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
