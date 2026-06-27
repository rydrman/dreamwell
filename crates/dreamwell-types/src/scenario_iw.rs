use std::collections::HashMap;

use serde::{Deserialize, Serialize};

use crate::StateKind;

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct RulesBlock {
    pub name: String,
    pub content: String,
}

/// Initial typed state for a scenario character (PC or NPC).
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct CharacterStateDef {
    pub key: String,
    pub kind: StateKind,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub initial_value: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub initial_num: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub initial_max: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub initial_float: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub unit: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sequence_items: Option<Vec<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sequence_loop: Option<bool>,
    #[serde(default)]
    pub visibility: String,
    #[serde(default)]
    pub update_hints: String,
}

impl CharacterStateDef {
    /// Promote per-character initial state into a tracked var bound to a runtime
    /// `target` ("pc", "world", or an NPC name).
    pub fn to_tracked_var(&self, target: &str) -> TrackedVarDef {
        TrackedVarDef {
            key: self.key.clone(),
            kind: self.kind,
            target: target.to_string(),
            description: self.description.clone(),
            initial_value: self.initial_value.clone(),
            initial_num: self.initial_num,
            initial_max: self.initial_max,
            initial_float: self.initial_float,
            unit: self.unit.clone(),
            sequence_items: self.sequence_items.clone(),
            sequence_loop: self.sequence_loop,
            visibility: self.visibility.clone(),
            update_hints: self.update_hints.clone(),
        }
    }
}

/// Merge generated character state into existing entries by key (add or update).
pub fn merge_character_state(
    existing: &[CharacterStateDef],
    generated: &[CharacterStateDef],
) -> Vec<CharacterStateDef> {
    let mut out = existing.to_vec();
    for def in generated {
        let key = def.key.trim();
        if key.is_empty() {
            continue;
        }
        if let Some(row) = out.iter_mut().find(|d| d.key == def.key) {
            *row = def.clone();
        } else {
            out.push(def.clone());
        }
    }
    out
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct GenerateCharacterStateTarget {
    pub role: String,
    pub name: String,
    pub description: String,
    #[serde(default)]
    pub traits: HashMap<String, i64>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct GenerateCharacterStateRequest {
    pub title: String,
    pub premise: String,
    pub setting: String,
    pub gm_style: String,
    #[serde(default)]
    pub objective: String,
    #[serde(default)]
    pub state_schema: Vec<TrackedVarDef>,
    #[serde(default)]
    pub cast: Vec<ScenarioNpc>,
    pub character: GenerateCharacterStateTarget,
    #[serde(default)]
    pub existing_state: Vec<CharacterStateDef>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct GenerateCharacterStateResponse {
    pub initial_state: Vec<CharacterStateDef>,
}

/// Merge world-level schema with per-character initial state for game creation.
/// Every entry becomes a tracked var bound to a runtime target ("world", "pc", or
/// an NPC name) — there is no separate scope axis.
pub fn merge_game_state_schema(
    base: &[TrackedVarDef],
    pc_state: &[CharacterStateDef],
    invited_cast: &[ScenarioNpc],
) -> Vec<TrackedVarDef> {
    let mut out: Vec<TrackedVarDef> = base
        .iter()
        .cloned()
        .map(|mut def| {
            def.target = normalize_target(&def.target);
            def
        })
        .collect();
    for def in pc_state {
        out.push(def.to_tracked_var("pc"));
    }
    for npc in invited_cast {
        for def in &npc.initial_state {
            out.push(def.to_tracked_var(npc.name.trim()));
        }
    }
    out
}

/// Normalize a tracked-var target: empty/blank becomes "world".
pub fn normalize_target(target: &str) -> String {
    let trimmed = target.trim();
    if trimmed.is_empty() {
        "world".to_string()
    } else {
        trimmed.to_string()
    }
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct ScenarioNpc {
    pub name: String,
    pub content: String,
    #[serde(default)]
    pub keywords: Vec<String>,
    #[serde(default)]
    pub traits: HashMap<String, i64>,
    #[serde(default)]
    pub initial_state: Vec<CharacterStateDef>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct SetupVarChoice {
    pub key: String,
    #[serde(default)]
    pub options: Vec<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct PcOption {
    pub name: String,
    pub description: String,
    #[serde(default)]
    pub traits: HashMap<String, i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub portrait_url: Option<String>,
    #[serde(default)]
    pub setup_vars: Vec<SetupVarChoice>,
    #[serde(default)]
    pub initial_state: Vec<CharacterStateDef>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct TraitDef {
    pub name: String,
    #[serde(default)]
    pub description: String,
}

/// A scenario-authored tracked value. Conceptually this is just initial state plus
/// authoring metadata: at game creation each def is seeded as a live state entry on
/// its `target` ("world", "pc", or an NPC name). There is no separate schema/scope
/// concept at runtime — `target` matches the runtime entry and the state tools.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(from = "TrackedVarDefWire")]
pub struct TrackedVarDef {
    pub key: String,
    pub kind: StateKind,
    /// Runtime target: "world", "pc", or an NPC name.
    #[serde(default = "default_target")]
    pub target: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub initial_value: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub initial_num: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub initial_max: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub initial_float: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub unit: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sequence_items: Option<Vec<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sequence_loop: Option<bool>,
    #[serde(default)]
    pub visibility: String,
    #[serde(default)]
    pub update_hints: String,
}

fn default_target() -> String {
    "world".to_string()
}

impl Default for TrackedVarDef {
    fn default() -> Self {
        Self {
            key: String::new(),
            kind: StateKind::default(),
            target: default_target(),
            description: String::new(),
            initial_value: String::new(),
            initial_num: None,
            initial_max: None,
            initial_float: None,
            unit: None,
            sequence_items: None,
            sequence_loop: None,
            visibility: String::new(),
            update_hints: String::new(),
        }
    }
}

/// Wire form used only for deserialization so we can read both the new `target`
/// field and the legacy `scope`/`actor_name` pair from older saved scenarios/games.
#[derive(Deserialize)]
struct TrackedVarDefWire {
    key: String,
    kind: StateKind,
    #[serde(default)]
    target: Option<String>,
    #[serde(default)]
    scope: Option<String>,
    #[serde(default)]
    actor_name: Option<String>,
    #[serde(default)]
    description: String,
    #[serde(default)]
    initial_value: String,
    #[serde(default)]
    initial_num: Option<i64>,
    #[serde(default)]
    initial_max: Option<i64>,
    #[serde(default)]
    initial_float: Option<f64>,
    #[serde(default)]
    unit: Option<String>,
    #[serde(default)]
    sequence_items: Option<Vec<String>>,
    #[serde(default)]
    sequence_loop: Option<bool>,
    #[serde(default)]
    visibility: String,
    #[serde(default)]
    update_hints: String,
}

impl From<TrackedVarDefWire> for TrackedVarDef {
    fn from(wire: TrackedVarDefWire) -> Self {
        let target = wire
            .target
            .filter(|t| !t.trim().is_empty())
            .map(|t| t.trim().to_string())
            .unwrap_or_else(|| match wire.scope.as_deref() {
                Some("pc") => "pc".to_string(),
                Some("npc") => wire
                    .actor_name
                    .as_deref()
                    .map(str::trim)
                    .filter(|n| !n.is_empty())
                    .map(str::to_string)
                    .unwrap_or_else(|| "world".to_string()),
                _ => "world".to_string(),
            });
        Self {
            key: wire.key,
            kind: wire.kind,
            target,
            description: wire.description,
            initial_value: wire.initial_value,
            initial_num: wire.initial_num,
            initial_max: wire.initial_max,
            initial_float: wire.initial_float,
            unit: wire.unit,
            sequence_items: wire.sequence_items,
            sequence_loop: wire.sequence_loop,
            visibility: wire.visibility,
            update_hints: wire.update_hints,
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct WinCondition {
    pub condition: String,
    pub epilogue_text: String,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct ContentFlags {
    #[serde(default)]
    pub mature: bool,
    #[serde(default)]
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct SourceMeta {
    pub platform: String,
    pub schema_version: f64,
    pub original_version: String,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct ScenarioTrigger {
    pub name: String,
    #[serde(default)]
    pub conditions: Vec<TriggerCondition>,
    #[serde(default)]
    pub effects: Vec<TriggerEffect>,
    #[serde(default)]
    pub can_repeat: bool,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct TriggerCondition {
    pub key: String,
    pub inequality: String,
    pub required_value: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum TriggerEffect {
    SetState { key: String, value: String },
    AppendGmInstruction { text: String },
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct SystemRollRequest {
    pub label: String,
    pub dice_expr: String,
    pub purpose: String,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct TurnPlan {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub round: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub active_player: Option<String>,
    #[serde(default)]
    pub board_positions: HashMap<String, i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub card_drawn: Option<String>,
    #[serde(default)]
    pub system_rolls_needed: Vec<SystemRollRequest>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub npc_decision_summary: Option<String>,
    #[serde(default)]
    pub summary_beats: Vec<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct GameTurnSystemRoll {
    pub id: i64,
    pub turn_id: i64,
    pub label: String,
    pub dice_expr: String,
    #[serde(default)]
    pub rolls: Vec<i64>,
    #[serde(default)]
    pub outcome_key: String,
    #[serde(default)]
    pub outcome_summary: String,
    pub sort_order: i64,
    pub created_at: chrono::DateTime<chrono::Utc>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::StateKind;

    #[test]
    fn merge_character_state_updates_existing_keys() {
        let existing = vec![CharacterStateDef {
            key: "health".into(),
            kind: StateKind::Measurement,
            initial_num: Some(3),
            ..Default::default()
        }];
        let generated = vec![
            CharacterStateDef {
                key: "health".into(),
                kind: StateKind::Measurement,
                initial_num: Some(5),
                initial_max: Some(5),
                ..Default::default()
            },
            CharacterStateDef {
                key: "mood".into(),
                kind: StateKind::Variable,
                initial_value: "wary".into(),
                ..Default::default()
            },
        ];
        let merged = merge_character_state(&existing, &generated);
        assert_eq!(merged.len(), 2);
        assert_eq!(merged[0].initial_num, Some(5));
        assert_eq!(merged[1].key, "mood");
    }

    #[test]
    fn merge_game_state_schema_adds_pc_and_npc_entries() {
        let base = vec![TrackedVarDef {
            key: "weather".into(),
            kind: StateKind::Variable,
            target: "world".into(),
            initial_value: "clear".into(),
            ..Default::default()
        }];
        let pc_state = vec![CharacterStateDef {
            key: "resolve".into(),
            kind: StateKind::Measurement,
            initial_num: Some(3),
            initial_max: Some(5),
            ..Default::default()
        }];
        let invited_cast = vec![ScenarioNpc {
            name: "Guard".into(),
            initial_state: vec![CharacterStateDef {
                key: "alertness".into(),
                kind: StateKind::Sequence,
                initial_num: Some(0),
                initial_max: Some(4),
                ..Default::default()
            }],
            ..ScenarioNpc::default()
        }];
        let merged = merge_game_state_schema(&base, &pc_state, &invited_cast);
        assert_eq!(merged.len(), 3);
        assert_eq!(merged[0].target, "world");
        assert_eq!(merged[1].target, "pc");
        assert_eq!(merged[2].target, "Guard");
    }

    #[test]
    fn tracked_var_def_reads_legacy_scope_and_actor_name() {
        let pc: TrackedVarDef = serde_json::from_value(serde_json::json!({
            "key": "resolve", "kind": "measurement", "scope": "pc"
        }))
        .unwrap();
        assert_eq!(pc.target, "pc");

        let npc: TrackedVarDef = serde_json::from_value(serde_json::json!({
            "key": "alertness", "kind": "sequence", "scope": "npc", "actor_name": "Guard"
        }))
        .unwrap();
        assert_eq!(npc.target, "Guard");

        let world: TrackedVarDef = serde_json::from_value(serde_json::json!({
            "key": "weather", "kind": "variable"
        }))
        .unwrap();
        assert_eq!(world.target, "world");

        let explicit: TrackedVarDef = serde_json::from_value(serde_json::json!({
            "key": "mood", "kind": "variable", "target": "Maya"
        }))
        .unwrap();
        assert_eq!(explicit.target, "Maya");
    }
}
