use std::collections::HashMap;

use dreamwell_types::{
    AppliedStateChange, SessionActor, StateChangeRequest, StateEntry, StateKind, StateOp,
};

use crate::resolve::{normalize_target, resolve_actor_id};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EntryMutation {
    Insert {
        actor_id: Option<i64>,
        kind: StateKind,
        key: String,
        value: String,
        num_value: Option<i64>,
        max_value: Option<i64>,
    },
    UpdateNumeric {
        entry_id: i64,
        num_value: i64,
        max_value: i64,
    },
    UpdateText {
        entry_id: i64,
        value: String,
    },
    UpdateKind {
        entry_id: i64,
        kind: StateKind,
    },
    Delete {
        entry_id: i64,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VivifyActor {
    pub name: String,
    pub role: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ApplyPlan {
    pub vivify: Vec<VivifyActor>,
    pub mutations: Vec<EntryMutation>,
    pub audit: Vec<AppliedStateChange>,
}

fn is_numeric_kind(kind: StateKind) -> bool {
    matches!(kind, StateKind::Resource | StateKind::Clock)
}

pub fn plan_state_changes(
    changes: &[StateChangeRequest],
    actors: &[SessionActor],
    current: &[StateEntry],
) -> ApplyPlan {
    let mut index: HashMap<(Option<i64>, String), &StateEntry> = HashMap::new();
    for entry in current {
        index.insert((entry.actor_id, entry.key.clone()), entry);
    }

    let mut vivify = Vec::new();
    let mut vivify_ids: HashMap<String, i64> = HashMap::new();
    let mut next_temp_id = -1i64;

    let mut resolve_target = |target: &str| -> Option<i64> {
        if normalize_target(target) == "world" {
            return None;
        }
        if let Some(id) = resolve_actor_id(target, actors) {
            return Some(id);
        }
        let normalized = normalize_target(target);
        if normalized == "pc" {
            return None;
        }
        let name = target.to_string();
        if let Some(&id) = vivify_ids.get(&name) {
            return Some(id);
        }
        let temp_id = next_temp_id;
        next_temp_id -= 1;
        vivify_ids.insert(name.clone(), temp_id);
        vivify.push(VivifyActor {
            name: name.clone(),
            role: "npc".to_string(),
        });
        Some(temp_id)
    };

    let mut mutations = Vec::new();
    let mut audit = Vec::new();

    for change in changes {
        let actor_id = resolve_target(&change.target);
        if normalize_target(&change.target) != "world" && actor_id.is_none() {
            continue;
        }
        let slot = (actor_id, change.key.clone());
        let existing = index.get(&slot).copied();

        let (applied, change_mutations) = if is_numeric_kind(change.kind) {
            plan_numeric_with_collision(change, actor_id, existing)
        } else {
            plan_text_with_collision(change, actor_id, existing)
        };

        for mutation in &change_mutations {
            if matches!(mutation, EntryMutation::Delete { .. }) {
                index.remove(&slot);
            }
        }
        mutations.extend(change_mutations);
        audit.push(applied);
    }

    ApplyPlan {
        vivify,
        mutations,
        audit,
    }
}

fn plan_numeric_with_collision(
    change: &StateChangeRequest,
    actor_id: Option<i64>,
    existing: Option<&StateEntry>,
) -> (AppliedStateChange, Vec<EntryMutation>) {
    let Some(existing) = existing else {
        let (applied, mutation) = plan_numeric_change(change, actor_id, None);
        return (applied, mutation.into_iter().collect());
    };

    if is_numeric_kind(existing.kind) {
        let (applied, mutation) = plan_numeric_change(change, actor_id, Some(existing));
        let mut mutations: Vec<EntryMutation> = mutation.into_iter().collect();
        if existing.kind != change.kind {
            mutations.push(EntryMutation::UpdateKind {
                entry_id: existing.id,
                kind: change.kind,
            });
        }
        return (applied, mutations);
    }

    let (mut applied, insert) = plan_numeric_change(change, actor_id, None);
    applied.prev_kind = Some(existing.kind);
    applied.prev_value = Some(existing.value.clone());
    applied.prev_num = existing.num_value;
    let mut mutations = vec![EntryMutation::Delete {
        entry_id: existing.id,
    }];
    if let Some(m) = insert {
        mutations.push(m);
    }
    (applied, mutations)
}

fn plan_text_with_collision(
    change: &StateChangeRequest,
    actor_id: Option<i64>,
    existing: Option<&StateEntry>,
) -> (AppliedStateChange, Vec<EntryMutation>) {
    let Some(existing) = existing else {
        let (applied, mutation) = plan_text_change(change, actor_id, None);
        return (applied, mutation.into_iter().collect());
    };

    if !is_numeric_kind(existing.kind) {
        let (applied, mutation) = plan_text_change(change, actor_id, Some(existing));
        let mut mutations: Vec<EntryMutation> = mutation.into_iter().collect();
        if existing.kind != change.kind {
            mutations.push(EntryMutation::UpdateKind {
                entry_id: existing.id,
                kind: change.kind,
            });
        }
        return (applied, mutations);
    }

    let (mut applied, insert) = plan_text_change(change, actor_id, None);
    applied.prev_kind = Some(existing.kind);
    applied.prev_value = Some(existing.value.clone());
    applied.prev_num = existing.num_value;
    let mut mutations = vec![EntryMutation::Delete {
        entry_id: existing.id,
    }];
    if let Some(m) = insert {
        mutations.push(m);
    }
    (applied, mutations)
}

fn plan_numeric_change(
    change: &StateChangeRequest,
    actor_id: Option<i64>,
    existing: Option<&StateEntry>,
) -> (AppliedStateChange, Option<EntryMutation>) {
    let prev_kind = existing.map(|e| e.kind);
    let prev_num = existing.and_then(|e| e.num_value);
    let prev_value = existing.map(|e| e.value.clone());
    let max = existing
        .and_then(|e| e.max_value)
        .or_else(|| {
            if change.kind == StateKind::Clock {
                Some(4)
            } else {
                change.delta.map(|_| 5).or(Some(5))
            }
        })
        .unwrap_or(5);

    let current = prev_num.unwrap_or(0);
    let new_num = match change.op {
        StateOp::Set => change.delta.unwrap_or(num_value_from_value(change)),
        StateOp::Add => current + change.delta.unwrap_or(0),
        StateOp::Remove => 0,
    };
    let clamped = new_num.clamp(0, max);

    let mutation = if let Some(entry) = existing {
        Some(EntryMutation::UpdateNumeric {
            entry_id: entry.id,
            num_value: clamped,
            max_value: max,
        })
    } else {
        Some(EntryMutation::Insert {
            actor_id,
            kind: change.kind,
            key: change.key.clone(),
            value: String::new(),
            num_value: Some(clamped),
            max_value: Some(max),
        })
    };

    let applied = AppliedStateChange {
        target: change.target.clone(),
        kind: change.kind,
        key: change.key.clone(),
        op: change.op,
        value: Some(clamped.to_string()),
        delta: change.delta,
        prev_value,
        prev_num,
        prev_kind,
    };
    (applied, mutation)
}

fn plan_text_change(
    change: &StateChangeRequest,
    actor_id: Option<i64>,
    existing: Option<&StateEntry>,
) -> (AppliedStateChange, Option<EntryMutation>) {
    let prev_kind = existing.map(|e| e.kind);
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

    let mutation = if change.op == StateOp::Remove && new_value.is_empty() {
        existing.map(|entry| EntryMutation::Delete { entry_id: entry.id })
    } else if let Some(entry) = existing {
        Some(EntryMutation::UpdateText {
            entry_id: entry.id,
            value: new_value.clone(),
        })
    } else {
        Some(EntryMutation::Insert {
            actor_id,
            kind: change.kind,
            key: change.key.clone(),
            value: new_value.clone(),
            num_value: None,
            max_value: None,
        })
    };

    let applied = AppliedStateChange {
        target: change.target.clone(),
        kind: change.kind,
        key: change.key.clone(),
        op: change.op,
        value: Some(new_value),
        delta: None,
        prev_value,
        prev_num: existing.and_then(|e| e.num_value),
        prev_kind,
    };
    (applied, mutation)
}

pub fn plan_revert_changes(
    changes: &[AppliedStateChange],
    actors: &[SessionActor],
) -> Vec<RevertMutation> {
    let mut mutations = Vec::new();
    for change in changes.iter().rev() {
        let actor_id = if normalize_target(&change.target) == "world" {
            None
        } else {
            resolve_actor_id(&change.target, actors)
        };

        let replaced_kind = change.prev_kind.filter(|prev| *prev != change.kind);
        if let Some(prev_kind) = replaced_kind {
            mutations.push(RevertMutation::DeleteByKey {
                actor_id,
                kind: change.kind,
                key: change.key.clone(),
            });
            if is_numeric_kind(prev_kind) {
                if let Some(num_value) = change.prev_num {
                    mutations.push(RevertMutation::RestoreNumeric {
                        actor_id,
                        kind: prev_kind,
                        key: change.key.clone(),
                        num_value,
                    });
                }
            } else if let Some(value) = &change.prev_value {
                mutations.push(RevertMutation::RestoreText {
                    actor_id,
                    kind: prev_kind,
                    key: change.key.clone(),
                    value: value.clone(),
                });
            }
            continue;
        }

        match change.kind {
            StateKind::Resource | StateKind::Clock => {
                if let Some(prev) = change.prev_num {
                    mutations.push(RevertMutation::RestoreNumeric {
                        actor_id,
                        kind: change.kind,
                        key: change.key.clone(),
                        num_value: prev,
                    });
                } else {
                    mutations.push(RevertMutation::DeleteByKey {
                        actor_id,
                        kind: change.kind,
                        key: change.key.clone(),
                    });
                }
            }
            StateKind::Condition | StateKind::Fact => {
                if let Some(prev) = &change.prev_value {
                    mutations.push(RevertMutation::RestoreText {
                        actor_id,
                        kind: change.kind,
                        key: change.key.clone(),
                        value: prev.clone(),
                    });
                } else {
                    mutations.push(RevertMutation::DeleteByKey {
                        actor_id,
                        kind: change.kind,
                        key: change.key.clone(),
                    });
                }
            }
        }
    }
    mutations
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RevertMutation {
    RestoreNumeric {
        actor_id: Option<i64>,
        kind: StateKind,
        key: String,
        num_value: i64,
    },
    RestoreText {
        actor_id: Option<i64>,
        kind: StateKind,
        key: String,
        value: String,
    },
    DeleteByKey {
        actor_id: Option<i64>,
        kind: StateKind,
        key: String,
    },
}

fn num_value_from_value(change: &StateChangeRequest) -> i64 {
    change
        .value
        .as_deref()
        .and_then(|v| v.parse().ok())
        .unwrap_or(0)
}

pub fn state_kind_str(kind: StateKind) -> &'static str {
    match kind {
        StateKind::Resource => "resource",
        StateKind::Condition => "condition",
        StateKind::Fact => "fact",
        StateKind::Clock => "clock",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;

    fn entry(id: i64, key: &str, num: i64, max: i64) -> StateEntry {
        StateEntry {
            id,
            game_id: 1,
            actor_id: None,
            kind: StateKind::Resource,
            key: key.to_string(),
            value: String::new(),
            num_value: Some(num),
            max_value: Some(max),
            source_turn: 1,
            updated_at: Utc::now(),
        }
    }

    fn pc_entry(key: &str, num: i64, max: i64) -> StateEntry {
        StateEntry {
            id: 1,
            game_id: 1,
            actor_id: Some(1),
            kind: StateKind::Resource,
            key: key.to_string(),
            value: String::new(),
            num_value: Some(num),
            max_value: Some(max),
            source_turn: 1,
            updated_at: Utc::now(),
        }
    }

    fn text_entry(id: i64, key: &str, kind: StateKind, value: &str) -> StateEntry {
        StateEntry {
            id,
            game_id: 1,
            actor_id: None,
            kind,
            key: key.to_string(),
            value: value.to_string(),
            num_value: None,
            max_value: None,
            source_turn: 1,
            updated_at: Utc::now(),
        }
    }

    fn actor() -> SessionActor {
        SessionActor {
            id: 1,
            game_id: 1,
            role: "pc".into(),
            name: "Alex".into(),
            description: String::new(),
            skills: Default::default(),
            sort_order: 0,
            created_at: Utc::now(),
            updated_at: Utc::now(),
        }
    }

    #[test]
    fn add_clamps_to_max() {
        let plan = plan_state_changes(
            &[StateChangeRequest {
                target: "pc".into(),
                kind: StateKind::Resource,
                key: "stress".into(),
                op: StateOp::Add,
                value: None,
                delta: Some(10),
            }],
            &[actor()],
            &[pc_entry("stress", 2, 5)],
        );
        assert_eq!(plan.mutations.len(), 1);
        assert!(matches!(
            plan.mutations[0],
            EntryMutation::UpdateNumeric {
                num_value: 5,
                max_value: 5,
                ..
            }
        ));
        assert_eq!(plan.audit[0].value.as_deref(), Some("5"));
        assert_eq!(plan.audit[0].prev_num, Some(2));
    }

    #[test]
    fn unknown_target_schedules_vivify() {
        let plan = plan_state_changes(
            &[StateChangeRequest {
                target: "Alice".into(),
                kind: StateKind::Fact,
                key: "mood".into(),
                op: StateOp::Set,
                value: Some("happy".into()),
                delta: None,
            }],
            &[actor()],
            &[],
        );
        assert_eq!(plan.vivify.len(), 1);
        assert_eq!(plan.vivify[0].name, "Alice");
    }

    #[test]
    fn fact_set_applies_literal_number_value() {
        let change: StateChangeRequest = serde_json::from_value(serde_json::json!({
            "target": "world",
            "kind": "fact",
            "key": "door_count",
            "op": "set",
            "value": 3
        }))
        .unwrap();
        assert_eq!(change.value.as_deref(), Some("3"));

        let plan = plan_state_changes(&[change], &[], &[]);
        assert_eq!(plan.mutations.len(), 1);
        assert!(matches!(
            plan.mutations[0],
            EntryMutation::Insert {
                value: ref v,
                ..
            } if v == "3"
        ));
    }

    #[test]
    fn same_key_different_kind_updates_existing_text_slot() {
        let plan = plan_state_changes(
            &[StateChangeRequest {
                target: "world".into(),
                kind: StateKind::Condition,
                key: "location".into(),
                op: StateOp::Set,
                value: Some("harbor".into()),
                delta: None,
            }],
            &[],
            &[text_entry(7, "location", StateKind::Fact, "docks")],
        );
        assert_eq!(plan.mutations.len(), 2);
        assert!(matches!(
            plan.mutations[0],
            EntryMutation::UpdateText { .. }
        ));
        assert!(matches!(
            plan.mutations[1],
            EntryMutation::UpdateKind {
                kind: StateKind::Condition,
                ..
            }
        ));
        assert_eq!(plan.audit[0].prev_kind, Some(StateKind::Fact));
    }

    #[test]
    fn same_key_cross_family_replaces_numeric_with_text() {
        let plan = plan_state_changes(
            &[StateChangeRequest {
                target: "world".into(),
                kind: StateKind::Fact,
                key: "alert".into(),
                op: StateOp::Set,
                value: Some("high".into()),
                delta: None,
            }],
            &[],
            &[entry(9, "alert", 3, 5)],
        );
        assert_eq!(plan.mutations.len(), 2);
        assert!(matches!(plan.mutations[0], EntryMutation::Delete { .. }));
        assert!(matches!(plan.mutations[1], EntryMutation::Insert { .. }));
        assert_eq!(plan.audit[0].prev_kind, Some(StateKind::Resource));
        assert_eq!(plan.audit[0].prev_num, Some(3));
    }
}
