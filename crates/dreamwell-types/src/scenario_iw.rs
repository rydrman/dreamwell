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
    #[serde(default)]
    pub visibility: String,
    #[serde(default)]
    pub update_hints: String,
}

impl CharacterStateDef {
    pub fn to_tracked_var(&self, scope: StateScope, actor_name: Option<&str>) -> TrackedVarDef {
        TrackedVarDef {
            key: self.key.clone(),
            kind: self.kind,
            scope,
            actor_name: actor_name.map(str::to_string),
            description: self.description.clone(),
            initial_value: self.initial_value.clone(),
            initial_num: self.initial_num,
            initial_max: self.initial_max,
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
pub fn merge_game_state_schema(
    base: &[TrackedVarDef],
    pc_state: &[CharacterStateDef],
    invited_cast: &[ScenarioNpc],
) -> Vec<TrackedVarDef> {
    let mut out = base.to_vec();
    for def in pc_state {
        out.push(def.to_tracked_var(StateScope::Pc, None));
    }
    for npc in invited_cast {
        for def in &npc.initial_state {
            out.push(def.to_tracked_var(StateScope::Npc, Some(npc.name.as_str())));
        }
    }
    out
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

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StateScope {
    #[default]
    World,
    Pc,
    Npc,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct TrackedVarDef {
    pub key: String,
    pub kind: StateKind,
    #[serde(default)]
    pub scope: StateScope,
    /// NPC name when `scope` is `Npc`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub actor_name: Option<String>,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub initial_value: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub initial_num: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub initial_max: Option<i64>,
    #[serde(default)]
    pub visibility: String,
    #[serde(default)]
    pub update_hints: String,
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
            kind: StateKind::Resource,
            initial_num: Some(3),
            ..Default::default()
        }];
        let generated = vec![
            CharacterStateDef {
                key: "health".into(),
                kind: StateKind::Resource,
                initial_num: Some(5),
                initial_max: Some(5),
                ..Default::default()
            },
            CharacterStateDef {
                key: "mood".into(),
                kind: StateKind::Fact,
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
            kind: StateKind::Fact,
            scope: StateScope::World,
            initial_value: "clear".into(),
            ..Default::default()
        }];
        let pc_state = vec![CharacterStateDef {
            key: "resolve".into(),
            kind: StateKind::Resource,
            initial_num: Some(3),
            initial_max: Some(5),
            ..Default::default()
        }];
        let invited_cast = vec![ScenarioNpc {
            name: "Guard".into(),
            initial_state: vec![CharacterStateDef {
                key: "alertness".into(),
                kind: StateKind::Clock,
                initial_num: Some(0),
                initial_max: Some(4),
                ..Default::default()
            }],
            ..ScenarioNpc::default()
        }];
        let merged = merge_game_state_schema(&base, &pc_state, &invited_cast);
        assert_eq!(merged.len(), 3);
        assert_eq!(merged[1].scope, StateScope::Pc);
        assert_eq!(merged[2].scope, StateScope::Npc);
        assert_eq!(merged[2].actor_name.as_deref(), Some("Guard"));
    }
}
