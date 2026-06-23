use dreamwell_types::{
    ElementInstances, Game, GameDetail, GameTurn, MechanicalData, MechanicalResult,
};
use serde_json::Value;

use crate::error::{AppError, AppResult};
use crate::game_mechanics::{
    execute_board_move, execute_card_draw, execute_dice_roll, resolve_deck_from_tags,
};
use crate::inference::ToolCall;

pub fn mechanical_tool_specs() -> Vec<Value> {
    vec![
        tool_spec(
            "roll_dice",
            "Roll dice with a standard expression (e.g. 1d6, 2d6).",
            serde_json::json!({
                "type": "object",
                "properties": {
                    "dice_expr": { "type": "string", "description": "Dice expression such as 1d6 or 2d6" },
                    "label": { "type": "string", "description": "Short label for this roll" }
                },
                "required": ["dice_expr"]
            }),
        ),
        tool_spec(
            "board_move",
            "Move the active player on the main board using the board's move dice.",
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
            "Draw a card from a deck. When deck_id is omitted, uses the landed space tag.",
            serde_json::json!({
                "type": "object",
                "properties": {
                    "deck_id": { "type": "string" },
                    "consume": { "type": "boolean" }
                }
            }),
        ),
    ]
}

/// Mechanical tools with descriptions tuned for the inline prose agent.
pub fn inline_mechanical_tool_specs() -> Vec<Value> {
    vec![
        tool_spec(
            "roll_dice",
            "Roll the die a drawn card's effect requires, AFTER the player has chosen the card's required targets/options. \
             Do NOT use for board movement — board_move rolls the move die automatically.",
            serde_json::json!({
                "type": "object",
                "properties": {
                    "dice_expr": { "type": "string", "description": "Dice expression such as 1d6 or 2d6" },
                    "label": { "type": "string", "description": "Short label for this roll (e.g. card effect name)" }
                },
                "required": ["dice_expr"]
            }),
        ),
        tool_spec(
            "board_move",
            "Begin a NEW game turn by advancing the active player: rolls the board move die and moves the piece (never call roll_dice first for the move). \
             Call AT MOST ONCE per turn, and ONLY when no previously drawn card is still awaiting the PC's choice or effect roll. \
             Do NOT call this to resolve a card the player just answered — finish that card first.",
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
            "Draw the card for the space just landed on (deck inferred from the space tag when deck_id is omitted). Call once, right after board_move. \
             Do NOT draw a new card while a previously drawn card's effect is still unresolved. \
             If the returned card text says Choose/Name and the player did not specify, call ask_pc_decision before resolving the effect.",
            serde_json::json!({
                "type": "object",
                "properties": {
                    "deck_id": { "type": "string" },
                    "consume": { "type": "boolean" }
                }
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
        "Pause and ask the player a concrete decision question when a card or the scene requires a choice the player has not made (e.g. which target or option the card or rule requires). Ends the turn immediately — do not narrate the PC's choice or continue past the question.",
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
    pub detail: GameDetail,
    pub turn: GameTurn,
    pub instances: ElementInstances,
    pub mechanical_results: Vec<MechanicalResult>,
    pub last_space_tags: Vec<String>,
    pub last_card_requires_roll: bool,
}

impl ToolSessionState {
    pub fn new(game: Game, detail: GameDetail, turn: GameTurn) -> Self {
        let instances = game.element_instances.clone();
        Self {
            game,
            detail,
            turn,
            instances,
            mechanical_results: Vec::new(),
            last_space_tags: Vec::new(),
            last_card_requires_roll: false,
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
            if is_redundant_board_move_roll(state, dice_expr) {
                return Ok(error_result(
                    "board_move rolls the move die automatically — do not call roll_dice for board movement; call board_move instead",
                ));
            }
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
            if let MechanicalData::BoardMove { space_tags, .. } = &result.data {
                state.last_space_tags = space_tags.clone();
            }
            result.sort_order = state.mechanical_results.len() as i64;
            let payload = mechanical_result_json(&result);
            state.mechanical_results.push(result);
            Ok(payload)
        }
        "draw_card" => {
            let consume = args["consume"].as_bool().unwrap_or(true);
            let deck_id = if let Some(id) = args["deck_id"].as_str() {
                id.to_string()
            } else {
                resolve_deck_from_tags(&state.last_space_tags, &state.game.game_elements)
                    .ok_or_else(|| {
                        AppError::bad_request("could not resolve deck from space tags")
                    })?
            };
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
            if let MechanicalData::CardDraw { text, .. } = &result.data {
                state.last_card_requires_roll =
                    text.contains("roll one die") || text.contains("roll a six-sided die");
            }
            result.sort_order = state.mechanical_results.len() as i64;
            let mut payload = mechanical_result_json(&result);
            if let MechanicalData::CardDraw { text, .. } = &result.data {
                let needs_choice = card_text_needs_pc_choice(text);
                if let Some(obj) = payload.as_object_mut() {
                    obj.insert("requires_roll".into(), state.last_card_requires_roll.into());
                    obj.insert("needs_pc_choice".into(), needs_choice.into());
                    if needs_choice {
                        obj.insert(
                            "hint".into(),
                            "Card requires the PC to choose targets/options — call ask_pc_decision before roll_dice or apply_state_changes unless the player action already specified the choice".into(),
                        );
                    }
                }
            }
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

fn card_text_needs_pc_choice(text: &str) -> bool {
    let lower = text.to_lowercase();
    lower.contains("choose") || lower.contains("name a ")
}

/// Reject roll_dice when it duplicates the board move die (board_move rolls internally).
fn is_redundant_board_move_roll(state: &ToolSessionState, dice_expr: &str) -> bool {
    if state.last_card_requires_roll {
        return false;
    }
    state
        .game
        .game_elements
        .boards
        .iter()
        .any(|board| board.move_dice.eq_ignore_ascii_case(dice_expr))
}

fn error_result(message: &str) -> Value {
    serde_json::json!({ "error": message })
}

#[cfg(test)]
mod tests {
    use super::*;
    use dreamwell_types::{
        BoardDef, Game, GameDetail, GameElementsConfig, GameTurn, ResolutionSystem,
    };

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
                    default_tag: "transformation".into(),
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
        ToolSessionState::new(
            game.clone(),
            GameDetail {
                game,
                turns: vec![],
                actors: vec![],
                state: vec![],
                scenes: vec![],
            },
            GameTurn {
                id: 1,
                game_id: 1,
                sort_order: 0,
                player_action: "go".into(),
                phase: "prose".into(),
                scene_beats: vec![],
                prose: String::new(),
                state_changes: vec![],
                checks: vec![],
                system_rolls: vec![],
                plan: None,
                mechanical_results: vec![],
                observability: Default::default(),
                generation_error: None,
                is_opening: false,
                created_at: chrono::Utc::now(),
                updated_at: chrono::Utc::now(),
            },
        )
    }

    #[test]
    fn redundant_move_roll_blocked_before_card() {
        let state = sample_state();
        assert!(is_redundant_board_move_roll(&state, "1d6"));
    }

    #[test]
    fn card_effect_roll_allowed_after_requires_roll_card() {
        let mut state = sample_state();
        state.last_card_requires_roll = true;
        assert!(!is_redundant_board_move_roll(&state, "1d6"));
    }

    #[test]
    fn grow_card_needs_pc_choice() {
        assert!(card_text_needs_pc_choice(
            "Choose a player and a body part, then roll one die."
        ));
    }

    #[tokio::test]
    async fn roll_dice_rejects_move_die_before_card() {
        let mut state = sample_state();
        let call = ToolCall {
            id: "t1".into(),
            name: "roll_dice".into(),
            arguments: r#"{"dice_expr":"1d6","label":"first move"}"#.into(),
        };
        let result = handle_mechanical_tool_call(&mut state, &call)
            .await
            .unwrap();
        assert!(result.get("error").is_some());
        assert!(state.mechanical_results.is_empty());
    }
}
