use dreamwell_types::{ElementInstances, Game, MechanicalResult};
use serde_json::Value;

use crate::error::{AppError, AppResult};
use crate::game_mechanics::{execute_board_move, execute_card_draw, execute_dice_roll};
use crate::inference::ToolCall;

/// Mechanical tools with descriptions tuned for the inline prose agent.
pub fn inline_mechanical_tool_specs() -> Vec<Value> {
    vec![
        tool_spec(
            "roll_dice",
            "Roll dice using a dice expression (e.g. 1d6, 2d6). Returns the individual rolls and total.",
            serde_json::json!({
                "type": "object",
                "properties": {
                    "dice_expr": { "type": "string", "description": "Dice expression such as 1d6 or 2d6" },
                    "label": { "type": "string", "description": "Short label for this roll (e.g. card effect, encounter)" }
                },
                "required": ["dice_expr"]
            }),
        ),
        tool_spec(
            "board_move",
            "Advance an actor on a board: rolls the board's move die, updates position, and returns from/to space plus space tags.",
            serde_json::json!({
                "type": "object",
                "properties": {
                    "board_id": { "type": "string", "description": "Board element id (default main)" },
                    "actor": { "type": "string", "description": "Actor key (default pc)" }
                }
            }),
        ),
        tool_spec(
            "draw_card",
            "Draw the top card from a named deck. Returns canonical card id, name, and text.",
            serde_json::json!({
                "type": "object",
                "properties": {
                    "deck_id": { "type": "string", "description": "Deck element id to draw from" },
                    "consume": { "type": "boolean", "description": "Whether to remove the card from the draw pile (default true)" }
                },
                "required": ["deck_id"]
            }),
        ),
    ]
}

fn apply_state_changes_spec() -> Value {
    tool_spec(
        "apply_state_changes",
        "Record durable tracked state updates for this turn — including the resolved outcome of a card or mechanic effect, as well as location, mood, inventory, NPC facts, resources, and clocks. Call whenever narration establishes or changes any tracked value — use the changes array with target/kind/key/op/value per STATE_CHANGE_PROMPT.",
        serde_json::json!({
            "type": "object",
            "properties": {
                "changes": {
                    "type": "array",
                    "items": {
                        "type": "object",
                        "properties": {
                            "target": { "type": "string" },
                            "kind": { "type": "string" },
                            "key": { "type": "string" },
                            "op": { "type": "string" },
                            "value": { "type": ["string", "null"] },
                            "delta": { "type": ["integer", "null"] }
                        },
                        "required": ["target", "kind", "key", "op"]
                    }
                }
            },
            "required": ["changes"]
        }),
    )
}

/// Tools for the single-pass inline-prose agent: scenario mechanics fired inline plus
/// tracked-state updates. Dramatic checks are declared and rolled before prose begins,
/// so check tools are intentionally excluded here.
pub fn inline_prose_tool_specs() -> Vec<Value> {
    let mut tools = inline_mechanical_tool_specs();
    tools.push(apply_state_changes_spec());
    tools.push(ask_pc_decision_spec());
    tools
}

fn ask_pc_decision_spec() -> Value {
    tool_spec(
        "ask_pc_decision",
        "Pause and ask the player a concrete decision question when a card or the scene requires a choice the player has not made. Ends the turn immediately — do not narrate the PC's choice or continue past the question.",
        serde_json::json!({
            "type": "object",
            "properties": {
                "question": {
                    "type": "string",
                    "description": "Direct second-person question for the player (e.g. 'Who do you target?')"
                }
            },
            "required": ["question"]
        }),
    )
}

fn tool_spec(name: &str, description: &str, parameters: Value) -> Value {
    serde_json::json!({
        "type": "function",
        "function": {
            "name": name,
            "description": description,
            "parameters": parameters
        }
    })
}

pub struct ToolSessionState {
    pub game: Game,
    pub instances: ElementInstances,
    pub mechanical_results: Vec<MechanicalResult>,
}

impl ToolSessionState {
    pub fn new(game: Game) -> Self {
        let instances = game.element_instances.clone();
        Self {
            game,
            instances,
            mechanical_results: Vec::new(),
        }
    }

    fn board_id(default: &str, args: &Value) -> String {
        args.get("board_id")
            .and_then(|v| v.as_str())
            .unwrap_or(default)
            .to_string()
    }

    fn actor_key(args: &Value) -> String {
        args.get("actor")
            .and_then(|v| v.as_str())
            .unwrap_or("pc")
            .to_string()
    }
}

pub async fn handle_mechanical_tool_call(
    state: &mut ToolSessionState,
    call: &ToolCall,
) -> AppResult<Value> {
    let args: Value =
        serde_json::from_str(&call.arguments).unwrap_or(Value::Object(Default::default()));
    match call.name.as_str() {
        "roll_dice" => {
            let dice_expr = args["dice_expr"].as_str().unwrap_or("1d6");
            let label = args["label"].as_str().unwrap_or("roll").to_string();
            let Some(mut result) = execute_dice_roll(dice_expr, &label) else {
                return Ok(error_result("invalid dice expression"));
            };
            result.sort_order = state.mechanical_results.len() as i64;
            let payload = mechanical_result_json(&result);
            state.mechanical_results.push(result);
            Ok(payload)
        }
        "board_move" => {
            let board_id = ToolSessionState::board_id("main", &args);
            let actor = ToolSessionState::actor_key(&args);
            let board = state
                .game
                .game_elements
                .boards
                .iter()
                .find(|b| b.id == board_id)
                .ok_or_else(|| AppError::bad_request(format!("unknown board {board_id}")))?;
            let Some(mut result) = execute_board_move(board, &mut state.instances, &actor) else {
                return Ok(error_result("board move failed"));
            };
            result.sort_order = state.mechanical_results.len() as i64;
            let payload = mechanical_result_json(&result);
            state.mechanical_results.push(result);
            Ok(payload)
        }
        "draw_card" => {
            let Some(deck_id) = args["deck_id"].as_str() else {
                return Ok(error_result("deck_id is required"));
            };
            let consume = args["consume"].as_bool().unwrap_or(true);
            let deck = state
                .game
                .game_elements
                .decks
                .iter()
                .find(|d| d.id == deck_id)
                .ok_or_else(|| AppError::bad_request(format!("unknown deck {deck_id}")))?;
            let Some(mut result) = execute_card_draw(deck, &mut state.instances, consume) else {
                return Ok(error_result("deck empty or draw failed"));
            };
            result.sort_order = state.mechanical_results.len() as i64;
            let payload = mechanical_result_json(&result);
            state.mechanical_results.push(result);
            Ok(payload)
        }
        other => Ok(error_result(&format!("unknown tool {other}"))),
    }
}

/// Parse `apply_state_changes` tool arguments into validated state-change requests.
pub fn parse_state_change_args(call: &ToolCall) -> Vec<dreamwell_types::StateChangeRequest> {
    let args: Value =
        serde_json::from_str(&call.arguments).unwrap_or(Value::Object(Default::default()));
    args.get("changes")
        .and_then(|v| v.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|v| serde_json::from_value(v.clone()).ok())
                .collect()
        })
        .unwrap_or_default()
}

fn mechanical_result_json(result: &MechanicalResult) -> Value {
    serde_json::to_value(result).unwrap_or_else(|_| serde_json::json!({ "ok": true }))
}

fn error_result(message: &str) -> Value {
    serde_json::json!({ "error": message })
}

#[cfg(test)]
mod tests {
    use super::*;
    use dreamwell_types::{BoardDef, Game, GameElementsConfig, ResolutionSystem};

    fn sample_state() -> ToolSessionState {
        let game = Game {
            id: 1,
            title: "Test".into(),
            premise: String::new(),
            setting: String::new(),
            gm_style: String::new(),
            opening_message: String::new(),
            character_id: None,
            scenario_id: None,
            resolution_system: ResolutionSystem::Pbta2d6,
            modifier_min: -2,
            modifier_max: 3,
            merge_resolve_scene: true,
            step_mode: false,
            engine_mode: dreamwell_types::EngineMode::ToolsStructured,
            game_elements: GameElementsConfig {
                boards: vec![BoardDef {
                    id: "main".into(),
                    spaces: 80,
                    move_dice: "1d6".into(),
                    tag_rules: vec![],
                    default_tag: "space".into(),
                }],
                ..Default::default()
            },
            element_instances: Default::default(),
            model_checks: String::new(),
            model_resolve: String::new(),
            model_prose: String::new(),
            rules_blocks: vec![],
            state_schema: vec![],
            win_condition: None,
            scenario_triggers: vec![],
            trait_defs: vec![],
            created_at: chrono::Utc::now(),
            updated_at: chrono::Utc::now(),
            archived_at: None,
            active_job: None,
            queued_jobs: 0,
        };
        ToolSessionState::new(game)
    }

    #[tokio::test]
    async fn draw_card_requires_deck_id() {
        let mut state = sample_state();
        let call = ToolCall {
            id: "t1".into(),
            name: "draw_card".into(),
            arguments: r#"{}"#.into(),
        };
        let result = handle_mechanical_tool_call(&mut state, &call)
            .await
            .unwrap();
        assert!(result.get("error").is_some());
        assert!(state.mechanical_results.is_empty());
    }

    #[tokio::test]
    async fn roll_dice_executes() {
        let mut state = sample_state();
        let call = ToolCall {
            id: "t1".into(),
            name: "roll_dice".into(),
            arguments: r#"{"dice_expr":"1d6","label":"test"}"#.into(),
        };
        let result = handle_mechanical_tool_call(&mut state, &call)
            .await
            .unwrap();
        assert!(result.get("error").is_none());
        assert_eq!(state.mechanical_results.len(), 1);
    }
}
