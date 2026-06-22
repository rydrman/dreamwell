//! Serde types for Infinite Worlds JSON exports (subset used by import).

use serde::Deserialize;

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct IwWorld {
    pub title: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub background: String,
    #[serde(default)]
    pub instructions: String,
    #[serde(default)]
    pub author_style: String,
    #[serde(default)]
    pub first_input: String,
    #[serde(default)]
    pub objective: String,
    #[serde(default)]
    pub char_select_text: String,
    #[serde(default)]
    pub description_request: String,
    #[serde(default)]
    pub schema_version: f64,
    #[serde(default)]
    pub version: String,
    #[serde(default)]
    pub mature: bool,
    #[serde(default)]
 
    #[serde(
        default,
        deserialize_with = "crate::serde_helpers::deserialize_string_or_string_list"
    )]
    pub content_warnings: Vec<String>,
    #[serde(default)]
    pub skills: Vec<String>,
    #[serde(default)]
    pub possible_characters: Vec<IwPossibleCharacter>,
    #[serde(default)]
    pub lore_book_entries: Vec<IwLoreEntry>,
    #[serde(default)]
    pub tracked_items: Vec<IwTrackedItem>,
    #[serde(default)]
    pub instruction_blocks: Vec<IwInstructionBlock>,
    #[serde(default)]
    pub trigger_events: Vec<IwTriggerEvent>,
    pub victory_condition: Option<IwVictoryCondition>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct IwPossibleCharacter {
    pub name: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub portrait: String,
    #[serde(default)]
    pub skills: std::collections::HashMap<String, i64>,
    #[serde(default)]
    pub initial_tracked_item_values: Vec<IwInitialTrackedValue>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct IwInitialTrackedValue {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub visibility: String,
    #[serde(default, rename = "initialPCValue")]
    pub initial_pc_value: Vec<String>,
    #[serde(default, rename = "initialValueBasedOnPC")]
    pub initial_value_based_on_pc: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct IwLoreEntry {
    pub name: String,
    #[serde(default)]
    pub content: String,
    #[serde(default)]
    pub keywords: Vec<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct IwTrackedItem {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub data_type: String,
    #[serde(default)]
    pub visibility: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub update_instructions: String,
    #[serde(default, rename = "initialValue")]
    pub initial_value: String,
    #[serde(default, rename = "initialValueBasedOnPC")]
    pub initial_value_based_on_pc: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct IwInstructionBlock {
    pub name: String,
    #[serde(default)]
    pub content: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct IwTriggerEvent {
    pub name: String,
    #[serde(default)]
    pub trigger_conditions: Vec<serde_json::Value>,
    #[serde(default)]
    pub trigger_effects: Vec<IwTriggerEffect>,
    #[serde(default)]
    pub can_trigger_more_than_once: bool,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct IwTriggerEffect {
    #[serde(rename = "type")]
    pub effect_type: String,
    pub data: serde_json::Value,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct IwVictoryCondition {
    #[serde(default)]
    pub condition: String,
    #[serde(default)]
    pub text: String,
}
