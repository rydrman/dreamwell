use dreamwell_types::{
    DeclaredCheck, ElementInstances, Game, GameDetail, GameTurn, GameTurnCheck, MechanicalData,
    MechanicalResult, StateChangeRequest,
};
use serde_json::Value;
use sqlx::SqlitePool;

use crate::error::{AppError, AppResult};
use crate::game_mechanics::{
    execute_board_move, execute_card_draw, execute_dice_roll, resolve_deck_from_tags,
};
use crate::game_resolution::{roll_dice, tier_str};
use crate::game_state::apply_state_changes;
use crate::game_turn::validate_declared_check;
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

pub fn structured_tool_specs() -> Vec<Value> {
    let mut tools = mechanical_tool_specs();
    tools.extend([
        tool_spec(
            "declare_check",
            "Declare a dramatic 2d6 check for the player character.",
            serde_json::json!({
                "type": "object",
                "properties": {
                    "label": { "type": "string" },
                    "skill": { "type": "string" },
                    "modifier": { "type": "integer" },
                    "stakes": { "type": "string" },
                    "justification": { "type": "string" }
                },
                "required": ["label", "skill", "modifier", "stakes", "justification"]
            }),
        ),
        tool_spec(
            "roll_dramatic_check",
            "Roll 2d6 for the most recently declared check.",
            serde_json::json!({
                "type": "object",
                "properties": {
                    "label": { "type": "string", "description": "Check label to roll (optional; rolls latest if omitted)" }
                }
            }),
        ),
        tool_spec(
            "apply_state_changes",
            "Apply validated state changes for this turn.",
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
        ),
        tool_spec(
            "set_scene_beats",
            "Set ordered scene beats for prose generation.",
            serde_json::json!({
                "type": "object",
                "properties": {
                    "beats": { "type": "array", "items": { "type": "string" } }
                },
                "required": ["beats"]
            }),
        ),
        tool_spec(
            "complete_structured_phase",
            "Signal that structured planning is complete and prose may begin.",
            serde_json::json!({ "type": "object", "properties": {} }),
        ),
    ]);
    tools
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
    pub pending_checks: Vec<DeclaredCheck>,
    pub rolled_checks: Vec<GameTurnCheck>,
    pub scene_beats: Vec<String>,
    pub state_changes: Vec<StateChangeRequest>,
    pub structured_complete: bool,
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
            pending_checks: Vec::new(),
            rolled_checks: Vec::new(),
            scene_beats: Vec::new(),
            state_changes: Vec::new(),
            structured_complete: false,
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
            let payload = mechanical_result_json(&result);
            state.mechanical_results.push(result);
            Ok(payload)
        }
        other => Ok(error_result(&format!("unknown tool {other}"))),
    }
}

pub async fn handle_structured_tool_call(
    pool: &SqlitePool,
    state: &mut ToolSessionState,
    call: &ToolCall,
    turn_id: i64,
) -> AppResult<Value> {
    let args: Value =
        serde_json::from_str(&call.arguments).unwrap_or(Value::Object(Default::default()));
    match call.name.as_str() {
        "roll_dice" | "board_move" | "draw_card" => handle_mechanical_tool_call(state, call).await,
        "declare_check" => {
            let check: DeclaredCheck = serde_json::from_value(args.clone())
                .map_err(|e| AppError::bad_request(format!("declare_check args: {e}")))?;
            state.pending_checks.push(check);
            Ok(serde_json::json!({ "ok": true, "pending": state.pending_checks.len() }))
        }
        "roll_dramatic_check" => {
            let label = args["label"].as_str();
            let pc = state
                .detail
                .actors
                .iter()
                .find(|a| a.role == "pc")
                .ok_or_else(|| AppError::internal("no PC actor"))?;
            let idx = if let Some(label) = label {
                state.pending_checks.iter().position(|c| c.label == label)
            } else {
                Some(state.pending_checks.len().saturating_sub(1))
            };
            let Some(idx) = idx else {
                return Ok(error_result("no matching declared check"));
            };
            let declared = state.pending_checks.remove(idx);
            let validated = validate_declared_check(&declared, pc, &state.game);
            let roll = roll_dice("2d6", validated.modifier);
            let game_check = GameTurnCheck {
                id: 0,
                turn_id,
                label: validated.label.clone(),
                skill: validated.skill.clone(),
                modifier: validated.modifier,
                stakes: validated.stakes.clone(),
                justification: validated.justification.clone(),
                dice_expr: "2d6".to_string(),
                seed: 0,
                rolls: roll.as_ref().map(|r| r.rolls.clone()).unwrap_or_default(),
                total: roll.as_ref().map(|r| r.total).unwrap_or(0),
                tier: roll.as_ref().map(|r| r.tier),
                margin: roll.as_ref().map(|r| r.margin).unwrap_or(0),
                sort_order: state.rolled_checks.len() as i64,
                created_at: chrono::Utc::now(),
            };
            state.rolled_checks.push(game_check.clone());
            Ok(serde_json::json!({
                "label": game_check.label,
                "total": game_check.total,
                "tier": game_check.tier.map(tier_str),
                "rolls": game_check.rolls
            }))
        }
        "apply_state_changes" => {
            let changes: Vec<StateChangeRequest> = args["changes"]
                .as_array()
                .map(|arr| {
                    arr.iter()
                        .filter_map(|v| serde_json::from_value(v.clone()).ok())
                        .collect()
                })
                .unwrap_or_default();
            let applied = apply_state_changes(
                pool,
                state.game.id,
                turn_id,
                &changes,
                &state.detail.actors,
                &state.detail.state,
            )
            .await?;
            state.state_changes.extend(changes);
            Ok(serde_json::json!({ "applied": applied.len() }))
        }
        "set_scene_beats" => {
            state.scene_beats = args["beats"]
                .as_array()
                .map(|arr| {
                    arr.iter()
                        .filter_map(|v| v.as_str().map(str::to_string))
                        .collect()
                })
                .unwrap_or_default();
            Ok(serde_json::json!({ "beats": state.scene_beats.len() }))
        }
        "complete_structured_phase" => {
            state.structured_complete = true;
            Ok(serde_json::json!({ "complete": true }))
        }
        other => Ok(error_result(&format!("unknown tool {other}"))),
    }
}

fn mechanical_result_json(result: &MechanicalResult) -> Value {
    serde_json::to_value(result).unwrap_or_else(|_| serde_json::json!({ "ok": true }))
}

fn error_result(message: &str) -> Value {
    serde_json::json!({ "error": message })
}
