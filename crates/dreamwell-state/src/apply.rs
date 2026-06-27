use std::collections::HashMap;

use dreamwell_types::{
    clamp_measurement, AppliedStateChange, SequencePayload, SessionActor, StateChangeRequest,
    StateEntry, StateKind, StateOp,
};

use crate::resolve::{normalize_target, resolve_actor_id};

#[derive(Debug, Clone, PartialEq)]
pub enum EntryMutation {
    Insert {
        actor_id: Option<i64>,
        kind: StateKind,
        key: String,
        value: String,
        float_value: Option<f64>,
        float_min: Option<f64>,
        float_max: Option<f64>,
        unit: Option<String>,
    },
    UpdateText {
        entry_id: i64,
        value: String,
    },
    UpdateMeasurement {
        entry_id: i64,
        float_value: Option<f64>,
        float_min: Option<f64>,
        float_max: Option<f64>,
        unit: Option<String>,
        clear: bool,
    },
    UpdateSequence {
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum KindFamily {
    Text,
    Measurement,
    Sequence,
}

fn kind_family(kind: StateKind) -> KindFamily {
    match kind {
        StateKind::Variable | StateKind::Condition => KindFamily::Text,
        StateKind::Measurement => KindFamily::Measurement,
        StateKind::Sequence => KindFamily::Sequence,
    }
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

        let (applied, change_mutations) = match kind_family(change.kind) {
            KindFamily::Text => plan_text_with_collision(change, actor_id, existing),
            KindFamily::Measurement => plan_measurement_with_collision(change, actor_id, existing),
            KindFamily::Sequence => plan_sequence_with_collision(change, actor_id, existing),
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

fn plan_with_collision<F>(
    change: &StateChangeRequest,
    actor_id: Option<i64>,
    existing: Option<&StateEntry>,
    same_family: impl Fn(&StateKind) -> bool,
    plan_same: F,
    plan_new: F,
) -> (AppliedStateChange, Vec<EntryMutation>)
where
    F: Fn(
        &StateChangeRequest,
        Option<i64>,
        Option<&StateEntry>,
    ) -> (AppliedStateChange, Vec<EntryMutation>),
{
    let Some(existing) = existing else {
        let (applied, mutations) = plan_new(change, actor_id, None);
        return (applied, mutations);
    };

    if same_family(&existing.kind) {
        let (applied, mutations) = plan_same(change, actor_id, Some(existing));
        let mut out = mutations;
        if existing.kind != change.kind {
            out.push(EntryMutation::UpdateKind {
                entry_id: existing.id,
                kind: change.kind,
            });
        }
        return (applied, out);
    }

    let (mut applied, new_mutations) = plan_new(change, actor_id, None);
    applied.prev_kind = Some(existing.kind);
    applied.prev_value = Some(existing.value.clone());
    applied.prev_num = existing.float_value.map(|v| v.round() as i64);
    let mut mutations = vec![EntryMutation::Delete {
        entry_id: existing.id,
    }];
    mutations.extend(new_mutations);
    (applied, mutations)
}

fn plan_text_with_collision(
    change: &StateChangeRequest,
    actor_id: Option<i64>,
    existing: Option<&StateEntry>,
) -> (AppliedStateChange, Vec<EntryMutation>) {
    plan_with_collision(
        change,
        actor_id,
        existing,
        |kind| matches!(kind, StateKind::Variable | StateKind::Condition),
        plan_text_change,
        plan_text_change,
    )
}

fn plan_measurement_with_collision(
    change: &StateChangeRequest,
    actor_id: Option<i64>,
    existing: Option<&StateEntry>,
) -> (AppliedStateChange, Vec<EntryMutation>) {
    plan_with_collision(
        change,
        actor_id,
        existing,
        |kind| *kind == StateKind::Measurement,
        plan_measurement_change,
        plan_measurement_change,
    )
}

fn plan_sequence_with_collision(
    change: &StateChangeRequest,
    actor_id: Option<i64>,
    existing: Option<&StateEntry>,
) -> (AppliedStateChange, Vec<EntryMutation>) {
    plan_with_collision(
        change,
        actor_id,
        existing,
        |kind| *kind == StateKind::Sequence,
        plan_sequence_change,
        plan_sequence_change,
    )
}

fn plan_text_change(
    change: &StateChangeRequest,
    actor_id: Option<i64>,
    existing: Option<&StateEntry>,
) -> (AppliedStateChange, Vec<EntryMutation>) {
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
        _ => change.value.clone().unwrap_or_default(),
    };

    let mutation = if change.op == StateOp::Remove && new_value.is_empty() {
        existing
            .map(|entry| EntryMutation::Delete { entry_id: entry.id })
            .into_iter()
            .collect()
    } else if let Some(entry) = existing {
        vec![EntryMutation::UpdateText {
            entry_id: entry.id,
            value: new_value.clone(),
        }]
    } else {
        vec![EntryMutation::Insert {
            actor_id,
            kind: change.kind,
            key: change.key.clone(),
            value: new_value.clone(),
            float_value: None,
            float_min: None,
            float_max: None,
            unit: None,
        }]
    };

    let applied = AppliedStateChange {
        target: change.target.clone(),
        kind: change.kind,
        key: change.key.clone(),
        op: change.op,
        value: Some(new_value),
        delta: None,
        prev_value,
        prev_num: existing.and_then(|e| e.float_value.map(|v| v.round() as i64)),
        prev_kind,
    };
    (applied, mutation)
}

fn float_from_change(change: &StateChangeRequest) -> Option<f64> {
    change.float_value.or_else(|| {
        change
            .value
            .as_deref()
            .and_then(|v| v.parse::<f64>().ok())
            .or_else(|| change.delta.map(|d| d as f64))
    })
}

fn plan_measurement_change(
    change: &StateChangeRequest,
    actor_id: Option<i64>,
    existing: Option<&StateEntry>,
) -> (AppliedStateChange, Vec<EntryMutation>) {
    let prev_kind = existing.map(|e| e.kind);
    let prev_value = existing.map(|e| e.value.clone());
    let prev_num = existing.and_then(|e| e.float_value.map(|v| v.round() as i64));

    let (float_value, float_min, float_max, unit, clear) = match change.op {
        StateOp::Remove => (None, None, None, None, true),
        StateOp::SetMin => (
            existing.and_then(|e| e.float_value),
            float_from_change(change),
            existing.and_then(|e| e.float_max),
            existing.and_then(|e| e.unit.clone()),
            false,
        ),
        StateOp::SetMax => (
            existing.and_then(|e| e.float_value),
            existing.and_then(|e| e.float_min),
            float_from_change(change),
            existing.and_then(|e| e.unit.clone()),
            false,
        ),
        StateOp::Set => {
            let raw = float_from_change(change).unwrap_or(0.0);
            let min = existing.and_then(|e| e.float_min);
            let max = existing.and_then(|e| e.float_max);
            let value = clamp_measurement(raw, min, max);
            (
                Some(value),
                min,
                max,
                change
                    .unit
                    .clone()
                    .or_else(|| existing.and_then(|e| e.unit.clone())),
                false,
            )
        }
        StateOp::Add => {
            let current = existing.and_then(|e| e.float_value).unwrap_or(0.0);
            let raw = current + change.delta.unwrap_or(0) as f64;
            let min = existing.and_then(|e| e.float_min);
            let max = existing.and_then(|e| e.float_max);
            let value = clamp_measurement(raw, min, max);
            (
                Some(value),
                min,
                max,
                existing.and_then(|e| e.unit.clone()),
                false,
            )
        }
        _ => (
            existing.and_then(|e| e.float_value),
            existing.and_then(|e| e.float_min),
            existing.and_then(|e| e.float_max),
            existing.and_then(|e| e.unit.clone()),
            false,
        ),
    };

    let display_value = float_value
        .map(|v| v.to_string())
        .or_else(|| prev_num.map(|n| n.to_string()));

    let mutation = if clear {
        existing
            .map(|entry| EntryMutation::Delete { entry_id: entry.id })
            .into_iter()
            .collect()
    } else if let Some(entry) = existing {
        vec![EntryMutation::UpdateMeasurement {
            entry_id: entry.id,
            float_value,
            float_min,
            float_max,
            unit: unit.clone(),
            clear: false,
        }]
    } else {
        vec![EntryMutation::Insert {
            actor_id,
            kind: StateKind::Measurement,
            key: change.key.clone(),
            value: String::new(),
            float_value,
            float_min,
            float_max,
            unit: unit.clone(),
        }]
    };

    let applied = AppliedStateChange {
        target: change.target.clone(),
        kind: StateKind::Measurement,
        key: change.key.clone(),
        op: change.op,
        value: display_value,
        delta: change.delta,
        prev_value,
        prev_num,
        prev_kind,
    };
    (applied, mutation)
}

fn plan_sequence_change(
    change: &StateChangeRequest,
    actor_id: Option<i64>,
    existing: Option<&StateEntry>,
) -> (AppliedStateChange, Vec<EntryMutation>) {
    let prev_kind = existing.map(|e| e.kind);
    let prev_value = existing.map(|e| e.value.clone());
    let prev_num = existing
        .and_then(|e| SequencePayload::decode(&e.value))
        .map(|s| s.position);

    let encoded = if change.op == StateOp::Remove {
        None
    } else if change.op == StateOp::Step {
        let Some(entry) = existing else {
            return (
                AppliedStateChange {
                    target: change.target.clone(),
                    kind: StateKind::Sequence,
                    key: change.key.clone(),
                    op: change.op,
                    value: None,
                    delta: change.delta,
                    prev_value,
                    prev_num,
                    prev_kind,
                },
                vec![],
            );
        };
        let mut seq = SequencePayload::decode(&entry.value).unwrap_or_else(|| {
            SequencePayload::new(vec!["step".into()], Some(0), false).expect("fallback")
        });
        let _ = seq.step(change.delta.unwrap_or(1));
        Some(seq.encode())
    } else {
        let items = change.sequence_items.clone().unwrap_or_default();
        let payload = SequencePayload::new(
            items,
            change.sequence_position,
            change.sequence_loop.unwrap_or(false),
        );
        payload.map(|p| p.encode())
    };

    let mutation = if change.op == StateOp::Remove {
        existing
            .map(|entry| EntryMutation::Delete { entry_id: entry.id })
            .into_iter()
            .collect()
    } else if let Some(value) = encoded.clone() {
        if let Some(entry) = existing {
            vec![EntryMutation::UpdateSequence {
                entry_id: entry.id,
                value,
            }]
        } else {
            vec![EntryMutation::Insert {
                actor_id,
                kind: StateKind::Sequence,
                key: change.key.clone(),
                value,
                float_value: None,
                float_min: None,
                float_max: None,
                unit: None,
            }]
        }
    } else {
        vec![]
    };

    let applied = AppliedStateChange {
        target: change.target.clone(),
        kind: StateKind::Sequence,
        key: change.key.clone(),
        op: change.op,
        value: encoded,
        delta: change.delta,
        prev_value,
        prev_num,
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
            if matches!(prev_kind, StateKind::Measurement | StateKind::Sequence) {
                if let Some(value) = &change.prev_value {
                    mutations.push(RevertMutation::RestoreStructured {
                        actor_id,
                        kind: prev_kind,
                        key: change.key.clone(),
                        value: value.clone(),
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
            StateKind::Measurement | StateKind::Sequence => {
                if let Some(prev) = &change.prev_value {
                    if prev.is_empty() {
                        mutations.push(RevertMutation::DeleteByKey {
                            actor_id,
                            kind: change.kind,
                            key: change.key.clone(),
                        });
                    } else {
                        mutations.push(RevertMutation::RestoreStructured {
                            actor_id,
                            kind: change.kind,
                            key: change.key.clone(),
                            value: prev.clone(),
                        });
                    }
                } else {
                    mutations.push(RevertMutation::DeleteByKey {
                        actor_id,
                        kind: change.kind,
                        key: change.key.clone(),
                    });
                }
            }
            StateKind::Condition | StateKind::Variable => {
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
    RestoreStructured {
        actor_id: Option<i64>,
        kind: StateKind,
        key: String,
        value: String,
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

pub fn state_kind_str(kind: StateKind) -> &'static str {
    match kind {
        StateKind::Measurement => "measurement",
        StateKind::Condition => "condition",
        StateKind::Variable => "variable",
        StateKind::Sequence => "sequence",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;

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

    fn measurement_entry(key: &str, value: f64, max: Option<f64>) -> StateEntry {
        StateEntry {
            id: 1,
            game_id: 1,
            actor_id: Some(1),
            kind: StateKind::Measurement,
            key: key.to_string(),
            value: String::new(),
            num_value: None,
            max_value: None,
            float_value: Some(value),
            float_min: None,
            float_max: max,
            unit: None,
            source_turn: 1,
            updated_at: Utc::now(),
        }
    }

    #[test]
    fn measurement_clamps_to_max() {
        let plan = plan_state_changes(
            &[StateChangeRequest {
                target: "pc".into(),
                kind: StateKind::Measurement,
                key: "stress".into(),
                op: StateOp::Set,
                value: None,
                delta: None,
                float_value: Some(9.0),
                float_min: None,
                float_max: None,
                unit: None,
                sequence_items: None,
                sequence_position: None,
                sequence_loop: None,
            }],
            &[actor()],
            &[measurement_entry("stress", 2.0, Some(5.0))],
        );
        assert!(matches!(
            plan.mutations[0],
            EntryMutation::UpdateMeasurement {
                float_value: Some(5.0),
                ..
            }
        ));
    }

    #[test]
    fn sequence_rejects_empty_set() {
        let plan = plan_state_changes(
            &[StateChangeRequest {
                target: "world".into(),
                kind: StateKind::Sequence,
                key: "initiative".into(),
                op: StateOp::Set,
                value: None,
                delta: None,
                float_value: None,
                float_min: None,
                float_max: None,
                unit: None,
                sequence_items: Some(vec![]),
                sequence_position: None,
                sequence_loop: None,
            }],
            &[],
            &[],
        );
        assert!(plan.mutations.is_empty());
    }

    #[test]
    fn variable_set_still_works() {
        let plan = plan_state_changes(
            &[StateChangeRequest {
                target: "pc".into(),
                kind: StateKind::Variable,
                key: "mood".into(),
                op: StateOp::Set,
                value: Some("calm".into()),
                delta: None,
                float_value: None,
                float_min: None,
                float_max: None,
                unit: None,
                sequence_items: None,
                sequence_position: None,
                sequence_loop: None,
            }],
            &[actor()],
            &[],
        );
        assert_eq!(plan.mutations.len(), 1);
    }
}
