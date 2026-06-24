use std::collections::HashMap;

use serde::{Deserialize, Serialize};

use crate::StateKind;

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct RulesBlock {
    pub name: String,
    pub content: String,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct ScenarioNpc {
    pub name: String,
    pub content: String,
    #[serde(default)]
    pub keywords: Vec<String>,
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
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct TrackedVarDef {
    pub key: String,
    pub kind: StateKind,
    #[serde(default)]
    pub scope: StateScope,
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
