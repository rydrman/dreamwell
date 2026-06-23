use std::collections::HashMap;

use crate::iw_types::IwWorld;
use crate::{
    build_game_elements_from_iw, default_game_traits, ContentFlags, PcOption, RulesBlock,
    ScenarioCreate, ScenarioNpc, ScenarioTrigger, SetupVarChoice, SourceMeta, StateKind,
    TrackedVarDef, TraitDef, TriggerCondition, TriggerEffect, WinCondition,
};

fn join_nonempty_sections(sections: &[(&str, &str)]) -> String {
    sections
        .iter()
        .filter(|(_, body)| !body.is_empty())
        .map(|(label, body)| format!("{label}:\n{body}"))
        .collect::<Vec<_>>()
        .join("\n\n")
}

fn iw_data_type_to_kind(data_type: &str) -> StateKind {
    match data_type.to_lowercase().as_str() {
        "number" => StateKind::Resource,
        _ => StateKind::Fact,
    }
}

fn is_setup_var(item: &crate::iw_types::IwTrackedItem) -> bool {
    item.initial_value_based_on_pc == "character" && item.visibility == "player_only"
}

pub fn iw_world_to_scenario(world: IwWorld) -> ScenarioCreate {
    let tracked_id_to_name: HashMap<String, String> = world
        .tracked_items
        .iter()
        .map(|t| (t.id.clone(), t.name.clone()))
        .collect();

    let rules_blocks: Vec<RulesBlock> = world
        .instruction_blocks
        .into_iter()
        .filter(|b| !b.content.trim().is_empty() || !b.name.trim().is_empty())
        .map(|b| RulesBlock {
            name: b.name,
            content: b.content,
        })
        .collect();

    let cast: Vec<ScenarioNpc> = world
        .lore_book_entries
        .into_iter()
        .map(|e| ScenarioNpc {
            name: e.name,
            content: e.content,
            keywords: e.keywords,
        })
        .collect();

    let trait_defs: Vec<TraitDef> = if world.skills.is_empty() {
        Vec::new()
    } else {
        world
            .skills
            .into_iter()
            .map(|name| TraitDef {
                name,
                description: String::new(),
            })
            .collect()
    };

    let setup_var_ids: std::collections::HashSet<String> = world
        .tracked_items
        .iter()
        .filter(|t| is_setup_var(t))
        .map(|t| t.id.clone())
        .collect();

    let state_schema: Vec<TrackedVarDef> = world
        .tracked_items
        .iter()
        .filter(|t| !is_setup_var(t))
        .map(|t| {
            let initial_num = if t.data_type == "number" {
                t.initial_value.parse().ok()
            } else {
                None
            };
            TrackedVarDef {
                key: t.name.clone(),
                kind: iw_data_type_to_kind(&t.data_type),
                description: t.description.clone(),
                initial_value: t.initial_value.clone(),
                initial_num,
                visibility: t.visibility.clone(),
                update_hints: t.update_instructions.clone(),
            }
        })
        .collect();

    let pc_options: Vec<PcOption> = world
        .possible_characters
        .into_iter()
        .map(|pc| {
            let setup_vars: Vec<SetupVarChoice> = pc
                .initial_tracked_item_values
                .into_iter()
                .filter(|v| setup_var_ids.contains(&v.id))
                .map(|v| SetupVarChoice {
                    key: v.name,
                    options: v.initial_pc_value,
                })
                .collect();
            let traits = if pc.skills.is_empty() {
                default_game_traits()
            } else {
                pc.skills
            };
            PcOption {
                name: pc.name,
                description: pc.description,
                traits,
                portrait_url: if pc.portrait.is_empty() {
                    None
                } else {
                    Some(pc.portrait)
                },
                setup_vars,
            }
        })
        .collect();

    let win_condition = world.victory_condition.map(|v| WinCondition {
        condition: v.condition,
        epilogue_text: v.text,
    });

    let scenario_triggers: Vec<ScenarioTrigger> = world
        .trigger_events
        .into_iter()
        .map(|event| map_trigger(event, &tracked_id_to_name))
        .collect();

    let premise = join_nonempty_sections(&[
        ("Background", world.background.trim()),
        ("Objective", world.objective.trim()),
        ("Description", world.description.trim()),
    ]);

    let gm_style = join_nonempty_sections(&[
        ("Author style", world.author_style.trim()),
        ("Output constraints", world.description_request.trim()),
        (
            "Writing Style",
            rules_blocks
                .iter()
                .find(|b| b.name == "Writing Style")
                .map(|b| b.content.as_str())
                .unwrap_or(""),
        ),
    ]);

    let truth_spaces = world
        .tracked_items
        .iter()
        .find(|t| t.name == "Truth_Spaces")
        .map(|t| t.initial_value.as_str());

    let game_elements = build_game_elements_from_iw(&rules_blocks, truth_spaces);

    ScenarioCreate {
        title: world.title,
        premise,
        setting: String::new(),
        gm_style,
        opening_message: world.first_input,
        setup_text: world.char_select_text,
        objective: world.objective,
        rules_blocks: rules_blocks.clone(),
        cast,
        trait_defs,
        pc_options,
        state_schema,
        win_condition,
        content_flags: ContentFlags {
            mature: world.mature,
 
            warnings: world.content_warnings,
        },
        source_meta: Some(SourceMeta {
            platform: "infinite_worlds".to_string(),
            schema_version: world.schema_version,
            original_version: world.version,
        }),
        scenario_triggers,
        game_elements,
        ..Default::default()
    }
}

fn map_trigger(
    event: crate::iw_types::IwTriggerEvent,
    tracked_id_to_name: &HashMap<String, String>,
) -> ScenarioTrigger {
    let conditions = event
        .trigger_conditions
        .iter()
        .flat_map(|value| extract_conditions(value, tracked_id_to_name))
        .collect();
    let effects = event
        .trigger_effects
        .into_iter()
        .filter_map(map_effect)
        .collect();
    ScenarioTrigger {
        name: event.name,
        conditions,
        effects,
        can_repeat: event.can_trigger_more_than_once,
    }
}

fn extract_conditions(
    value: &serde_json::Value,
    tracked_id_to_name: &HashMap<String, String>,
) -> Vec<TriggerCondition> {
    let mut out = Vec::new();
    if let Some(category) = value.get("category").and_then(|v| v.as_str()) {
        if category == "logic" {
            if let Some(children) = value.get("data").and_then(|d| d.as_array()) {
                for child in children {
                    out.extend(extract_conditions(child, tracked_id_to_name));
                }
            }
            return out;
        }
    }
    if value.get("type").and_then(|v| v.as_str()) == Some("triggerOnTrackedItem") {
        let data = value.get("data").unwrap_or(value);
        let tracked_id = data
            .get("trackedItemID")
            .and_then(|v| v.as_str())
            .unwrap_or("");
        let key = tracked_id_to_name
            .get(tracked_id)
            .cloned()
            .unwrap_or_else(|| tracked_id.to_string());
        let inequality = data
            .get("inequality")
            .and_then(|v| v.as_str())
            .unwrap_or("at_least")
            .to_string();
        let required_value = data
            .get("requiredValue")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        if !key.is_empty() {
            out.push(TriggerCondition {
                key,
                inequality,
                required_value,
            });
        }
    }
    out
}

fn map_effect(effect: crate::iw_types::IwTriggerEffect) -> Option<TriggerEffect> {
    match effect.effect_type.as_str() {
        "effectModifyInstructionBlock" => Some(TriggerEffect::InjectRulesBlock {
            block_name: "Cards and probabilities".to_string(),
        }),
        "effectTellAIWhatToDo" | "effectGiveInfo" => {
            effect
                .data
                .as_str()
                .map(|text| TriggerEffect::AppendGmInstruction {
                    text: text.to_string(),
                })
        }
        _ => None,
    }
}

pub fn parse_iw_world(json: &str) -> Result<IwWorld, serde_json::Error> {
    serde_json::from_str(json)
}

pub fn scenario_create_from_iw_json(json: &str) -> Result<ScenarioCreate, serde_json::Error> {
    let world = parse_iw_world(json)?;
    Ok(iw_world_to_scenario(world))
}

impl Default for IwWorld {
    fn default() -> Self {
        Self {
            title: String::new(),
            description: String::new(),
            background: String::new(),
            instructions: String::new(),
            author_style: String::new(),
            first_input: String::new(),
            objective: String::new(),
            char_select_text: String::new(),
            description_request: String::new(),
            schema_version: 0.0,
            version: String::new(),
            mature: false,
 
            content_warnings: Vec::new(),
            skills: Vec::new(),
            possible_characters: Vec::new(),
            lore_book_entries: Vec::new(),
            tracked_items: Vec::new(),
            instruction_blocks: Vec::new(),
            trigger_events: Vec::new(),
            victory_condition: None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn maps_skills_to_trait_defs() {
        let world = IwWorld {
            title: "Test".into(),
            skills: vec!["Boldness".into(), "Curiosity".into()],
            ..Default::default()
        };
        let scenario = iw_world_to_scenario(world);
        assert_eq!(scenario.trait_defs.len(), 2);
        assert_eq!(scenario.trait_defs[0].name, "Boldness");
    }

    #[test]
    fn iw_board_game_fixture_maps_faithfully() {
        let json = include_str!("../../server/tests/fixtures/iw_board_game_scenario.json");
        let scenario = scenario_create_from_iw_json(json).expect("parse fixture");
        assert_eq!(scenario.title, "Crystal Quest");
        assert_eq!(scenario.cast.len(), 8);
        assert_eq!(scenario.pc_options.len(), 2);
        assert_eq!(scenario.trait_defs.len(), 5);
        assert!(scenario
            .rules_blocks
            .iter()
            .any(|b| b.name == "Game Mechanics"));
        assert!(scenario
            .rules_blocks
            .iter()
            .any(|b| b.name == "Cards and probabilities"));
        assert!(!scenario.state_schema.is_empty());
        assert!(scenario
            .state_schema
            .iter()
            .all(|v| v.key != "Character1" && v.key != "Character2"));
        assert!(scenario.win_condition.is_some());
        assert_eq!(scenario.scenario_triggers.len(), 2);
 
        assert_eq!(
            scenario
                .source_meta
                .as_ref()
                .map(|m| m.original_version.as_str()),
            Some("4.2")
        );
        assert!(scenario.opening_message.contains("<<character1>>"));
        assert!(!scenario.game_elements.boards.is_empty());
        assert_eq!(scenario.game_elements.boards[0].id, "main");
        assert_eq!(scenario.game_elements.decks.len(), 2);
        assert!(!scenario.game_elements.turn_mechanicals.is_empty());
    }
}
