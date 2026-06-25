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

/// Convenience state tools (one per intent) that each map to a single
/// `StateChangeRequest`. They keep the kind/op implicit in the tool name so the
/// inline prose agent only ever supplies target/key/value — far simpler than the
/// batch `apply_state_changes` blob.
pub const SIMPLE_STATE_TOOLS: &[&str] = &[
    "set_fact",
    "clear_fact",
    "set_condition",
    "clear_condition",
    "adjust_resource",
    "set_resource",
    "advance_clock",
    "set_clock",
];

const STATE_TARGET_DESC: &str =
    "Who the value belongs to: \"pc\" for the player character, \"world\" for scene-wide facts, or an NPC's name. An unknown name auto-creates that NPC.";

fn state_target_key_props(value_prop: Value) -> Value {
    let mut props = serde_json::json!({
        "target": { "type": "string", "description": STATE_TARGET_DESC },
        "key": { "type": "string", "description": "Short snake_case attribute name (e.g. location, mood, shirt_color) — one attribute only." }
    });
    if let (Value::Object(props_map), Value::Object(value_map)) = (&mut props, value_prop) {
        for (k, v) in value_map {
            props_map.insert(k, v);
        }
    }
    props
}

fn text_state_tool(name: &str, description: &str) -> Value {
    tool_spec(
        name,
        description,
        serde_json::json!({
            "type": "object",
            "properties": state_target_key_props(serde_json::json!({
                "value": { "type": "string", "description": "The attribute's new value — just the value itself (e.g. \"tavern\", \"green\"), not a full sentence." }
            })),
            "required": ["target", "key", "value"]
        }),
    )
}

fn clear_state_tool(name: &str, description: &str) -> Value {
    tool_spec(
        name,
        description,
        serde_json::json!({
            "type": "object",
            "properties": state_target_key_props(serde_json::json!({})),
            "required": ["target", "key"]
        }),
    )
}

fn numeric_state_tool(
    name: &str,
    description: &str,
    amount_field: &str,
    amount_desc: &str,
) -> Value {
    let value_prop = serde_json::json!({
        amount_field: { "type": "integer", "description": amount_desc }
    });
    tool_spec(
        name,
        description,
        serde_json::json!({
            "type": "object",
            "properties": state_target_key_props(value_prop),
            "required": ["target", "key", amount_field]
        }),
    )
}

/// Specs for the simple per-intent state tools offered to the inline prose agent.
pub fn simple_state_tool_specs() -> Vec<Value> {
    vec![
        text_state_tool(
            "set_fact",
            "Record or update a durable tracked fact (location, inventory item, NPC trait, quest stage). Call whenever narration establishes or changes a lasting fact — the tool is the source of truth, not the prose.",
        ),
        clear_state_tool(
            "clear_fact",
            "Remove a durable fact that is no longer true (an item is dropped, a location is left behind).",
        ),
        text_state_tool(
            "set_condition",
            "Record or update a temporary status that will likely clear soon (hidden, bleeding, suspicious, inspired).",
        ),
        clear_state_tool(
            "clear_condition",
            "Remove a temporary status once it ends (no longer hidden, bleeding stops).",
        ),
        numeric_state_tool(
            "adjust_resource",
            "Change a numeric resource track by a relative amount (e.g. stress +1, hit points -2). Values clamp to 0..max.",
            "delta",
            "Signed amount to add (negative to subtract), e.g. 1 or -2.",
        ),
        numeric_state_tool(
            "set_resource",
            "Set a numeric resource track to an exact value (e.g. supply = 3). Values clamp to 0..max.",
            "value",
            "The exact value to set the resource to.",
        ),
        numeric_state_tool(
            "advance_clock",
            "Advance (or rewind) a segmented progress clock by a relative number of segments (e.g. investigation +1).",
            "delta",
            "Signed number of segments to add (negative to rewind), e.g. 1 or -1.",
        ),
        numeric_state_tool(
            "set_clock",
            "Set a segmented progress clock to an exact number of filled segments.",
            "value",
            "The exact number of filled segments to set.",
        ),
    ]
}

/// Tools for the single-pass inline-prose agent: scenario mechanics fired inline plus
/// tracked-state updates. Dramatic checks are declared and rolled before prose begins,
/// so check tools are intentionally excluded here.
pub fn inline_prose_tool_specs() -> Vec<Value> {
    let mut tools = inline_mechanical_tool_specs();
    tools.extend(simple_state_tool_specs());
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

/// Whether a tool name updates tracked state (either the batch tool or one of the
/// simple per-intent convenience tools).
pub fn is_state_tool(name: &str) -> bool {
    name == "apply_state_changes" || SIMPLE_STATE_TOOLS.contains(&name)
}

/// Parse any state tool call into validated state-change requests. Handles both the
/// batch `apply_state_changes` array and the simple per-intent tools (set_fact, etc).
pub fn parse_state_tool_call(call: &ToolCall) -> Vec<dreamwell_types::StateChangeRequest> {
    if call.name == "apply_state_changes" {
        return parse_state_change_args(call);
    }
    parse_simple_state_tool(call).into_iter().collect()
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

/// Translate a simple per-intent state tool call (set_fact, adjust_resource, …) into
/// a single `StateChangeRequest`. Returns `None` for unknown tools or missing key.
fn parse_simple_state_tool(call: &ToolCall) -> Option<dreamwell_types::StateChangeRequest> {
    use dreamwell_types::{StateChangeRequest, StateKind, StateOp};

    let args: Value =
        serde_json::from_str(&call.arguments).unwrap_or(Value::Object(Default::default()));
    let target = args
        .get("target")
        .and_then(value_as_string)
        .filter(|s| !s.trim().is_empty())
        .unwrap_or_else(|| "world".to_string());
    let key = args
        .get("key")
        .and_then(value_as_string)
        .filter(|s| !s.trim().is_empty())?;
    let value = args.get("value").and_then(value_as_string);
    let delta_field = args.get("delta").and_then(value_as_i64);
    let value_as_num = args.get("value").and_then(value_as_i64);

    let request = |kind: StateKind, op: StateOp, value: Option<String>, delta: Option<i64>| {
        Some(StateChangeRequest {
            target: target.clone(),
            kind,
            key: key.clone(),
            op,
            value,
            delta,
        })
    };

    match call.name.as_str() {
        "set_fact" => request(StateKind::Fact, StateOp::Set, value, None),
        "clear_fact" => request(StateKind::Fact, StateOp::Remove, None, None),
        "set_condition" => request(StateKind::Condition, StateOp::Set, value, None),
        "clear_condition" => request(StateKind::Condition, StateOp::Remove, None, None),
        "adjust_resource" => request(StateKind::Resource, StateOp::Add, None, delta_field),
        "set_resource" => request(StateKind::Resource, StateOp::Set, None, value_as_num),
        "advance_clock" => request(StateKind::Clock, StateOp::Add, None, delta_field),
        "set_clock" => request(StateKind::Clock, StateOp::Set, None, value_as_num),
        _ => None,
    }
}

fn value_as_string(value: &Value) -> Option<String> {
    match value {
        Value::String(s) => Some(s.clone()),
        Value::Number(n) => Some(n.to_string()),
        Value::Bool(b) => Some(b.to_string()),
        _ => None,
    }
}

fn value_as_i64(value: &Value) -> Option<i64> {
    match value {
        Value::Number(n) => n.as_i64(),
        Value::String(s) => s.trim().parse().ok(),
        _ => None,
    }
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

    fn state_call(name: &str, args: serde_json::Value) -> ToolCall {
        ToolCall {
            id: "s1".into(),
            name: name.into(),
            arguments: args.to_string(),
        }
    }

    #[test]
    fn inline_specs_offer_simple_tools_and_hide_batch() {
        let names: Vec<String> = inline_prose_tool_specs()
            .iter()
            .filter_map(|t| t["function"]["name"].as_str().map(str::to_string))
            .collect();
        for tool in SIMPLE_STATE_TOOLS {
            assert!(names.iter().any(|n| n == tool), "missing tool {tool}");
        }
        assert!(!names.iter().any(|n| n == "apply_state_changes"));
        assert!(names.iter().any(|n| n == "ask_pc_decision"));
    }

    #[test]
    fn is_state_tool_recognizes_simple_and_batch() {
        assert!(is_state_tool("apply_state_changes"));
        assert!(is_state_tool("set_fact"));
        assert!(is_state_tool("advance_clock"));
        assert!(!is_state_tool("roll_dice"));
    }

    #[test]
    fn set_fact_maps_to_fact_set_request() {
        use dreamwell_types::{StateKind, StateOp};
        let call = state_call(
            "set_fact",
            serde_json::json!({ "target": "world", "key": "location", "value": "tavern" }),
        );
        let reqs = parse_state_tool_call(&call);
        assert_eq!(reqs.len(), 1);
        assert_eq!(reqs[0].target, "world");
        assert_eq!(reqs[0].kind, StateKind::Fact);
        assert_eq!(reqs[0].op, StateOp::Set);
        assert_eq!(reqs[0].key, "location");
        assert_eq!(reqs[0].value.as_deref(), Some("tavern"));
    }

    #[test]
    fn clear_condition_maps_to_condition_remove() {
        use dreamwell_types::{StateKind, StateOp};
        let call = state_call(
            "clear_condition",
            serde_json::json!({ "target": "pc", "key": "hidden" }),
        );
        let reqs = parse_state_tool_call(&call);
        assert_eq!(reqs.len(), 1);
        assert_eq!(reqs[0].kind, StateKind::Condition);
        assert_eq!(reqs[0].op, StateOp::Remove);
        assert!(reqs[0].value.is_none());
    }

    #[test]
    fn adjust_resource_carries_signed_delta() {
        use dreamwell_types::{StateKind, StateOp};
        let call = state_call(
            "adjust_resource",
            serde_json::json!({ "target": "pc", "key": "stress", "delta": -2 }),
        );
        let reqs = parse_state_tool_call(&call);
        assert_eq!(reqs[0].kind, StateKind::Resource);
        assert_eq!(reqs[0].op, StateOp::Add);
        assert_eq!(reqs[0].delta, Some(-2));
    }

    #[test]
    fn set_resource_reads_value_as_numeric_delta() {
        use dreamwell_types::StateOp;
        let call = state_call(
            "set_resource",
            serde_json::json!({ "target": "pc", "key": "supply", "value": 3 }),
        );
        let reqs = parse_state_tool_call(&call);
        assert_eq!(reqs[0].op, StateOp::Set);
        assert_eq!(reqs[0].delta, Some(3));
    }

    #[test]
    fn set_clock_accepts_stringified_number() {
        use dreamwell_types::StateKind;
        let call = state_call(
            "set_clock",
            serde_json::json!({ "target": "world", "key": "alarm", "value": "2" }),
        );
        let reqs = parse_state_tool_call(&call);
        assert_eq!(reqs[0].kind, StateKind::Clock);
        assert_eq!(reqs[0].delta, Some(2));
    }

    #[test]
    fn simple_tool_defaults_target_to_world_and_drops_empty_key() {
        let call = state_call("set_fact", serde_json::json!({ "value": "noon" }));
        assert!(parse_state_tool_call(&call).is_empty());

        let call = state_call(
            "set_fact",
            serde_json::json!({ "key": "time", "value": "noon" }),
        );
        let reqs = parse_state_tool_call(&call);
        assert_eq!(reqs[0].target, "world");
    }

    #[test]
    fn batch_tool_still_parses_changes_array() {
        let call = state_call(
            "apply_state_changes",
            serde_json::json!({
                "changes": [
                    { "target": "pc", "kind": "fact", "key": "mood", "op": "set", "value": "calm" }
                ]
            }),
        );
        let reqs = parse_state_tool_call(&call);
        assert_eq!(reqs.len(), 1);
        assert_eq!(reqs[0].key, "mood");
    }
}
