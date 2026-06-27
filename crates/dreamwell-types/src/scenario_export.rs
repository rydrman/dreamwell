use serde::{Deserialize, Serialize};

use crate::Scenario;
use crate::ScenarioCreate;

pub const SCENARIO_EXPORT_FORMAT: &str = "dreamwell.scenario.v1";

/// Portable scenario document for JSON import/export.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ScenarioExport {
    pub format: String,
    #[serde(flatten)]
    pub scenario: ScenarioCreate,
}

impl ScenarioExport {
    pub fn from_scenario(scenario: &Scenario) -> Self {
        Self {
            format: SCENARIO_EXPORT_FORMAT.to_string(),
            scenario: scenario_create_from_scenario(scenario),
        }
    }
}

pub fn scenario_create_from_scenario(scenario: &Scenario) -> ScenarioCreate {
    ScenarioCreate {
        title: scenario.title.clone(),
        premise: scenario.premise.clone(),
        setting: scenario.setting.clone(),
        gm_style: scenario.gm_style.clone(),
        opening_message: scenario.opening_message.clone(),
        opening_guidance: scenario.opening_guidance.clone(),
        pc_name: scenario.pc_name.clone(),
        pc_description: scenario.pc_description.clone(),
        pc_initial_state: scenario.pc_initial_state.clone(),
        traits: scenario.traits.clone(),
        character_id: scenario.character_id,
        rules_blocks: scenario.rules_blocks.clone(),
        objective: scenario.objective.clone(),
        setup_text: scenario.setup_text.clone(),
        trait_defs: scenario.trait_defs.clone(),
        cast: scenario.cast.clone(),
        pc_options: scenario.pc_options.clone(),
        state_schema: scenario.state_schema.clone(),
        cast_uniform_state: scenario.cast_uniform_state.clone(),
        win_condition: scenario.win_condition.clone(),
        content_flags: scenario.content_flags.clone(),
        source_meta: scenario.source_meta.clone(),
        scenario_triggers: scenario.scenario_triggers.clone(),
        game_elements: scenario.game_elements.clone(),
    }
}

pub fn parse_scenario_export_json(json: &str) -> Result<ScenarioCreate, serde_json::Error> {
    let export: ScenarioExport = serde_json::from_str(json)?;
    if export.format != SCENARIO_EXPORT_FORMAT {
        return Err(serde_json::Error::io(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            format!("unsupported scenario export format: {}", export.format),
        )));
    }
    Ok(export.scenario)
}

pub fn is_scenario_export_value(value: &serde_json::Value) -> bool {
    value.get("format").and_then(|v| v.as_str()) == Some(SCENARIO_EXPORT_FORMAT)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{ContentFlags, RulesBlock};

    #[test]
    fn round_trips_export_document() {
        let create = ScenarioCreate {
            title: "Board Game Night".into(),
            premise: "Friends play a cooperative board game.".into(),
            rules_blocks: vec![RulesBlock {
                name: "Game Mechanics".into(),
                content: "Roll and move.".into(),
            }],
            content_flags: ContentFlags {
                mature: false,
                warnings: vec![],
            },
            ..Default::default()
        };
        let json = serde_json::to_string(&ScenarioExport {
            format: SCENARIO_EXPORT_FORMAT.to_string(),
            scenario: create.clone(),
        })
        .expect("serialize");
        let parsed = parse_scenario_export_json(&json).expect("parse");
        assert_eq!(parsed, create);
    }

    #[test]
    fn rejects_unknown_format() {
        let json = r#"{"format":"other.v1","title":"X"}"#;
        assert!(parse_scenario_export_json(json).is_err());
    }
}
