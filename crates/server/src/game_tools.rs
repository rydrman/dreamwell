use dreamwell_types::{ElementInstances, Game, MechanicalResult};
use serde_json::Value;

use crate::error::{AppError, AppResult};
use crate::game_mechanics::{execute_board_move, execute_card_draw, execute_dice_roll};
use crate::game_prompts::PRESENT_FORK_RULES;
use crate::game_resolution::parse_dice_expr;
use crate::inference::ToolCall;

/// Mechanical tools with descriptions tuned for the inline prose agent.
pub fn inline_mechanical_tool_specs() -> Vec<Value> {
    vec![
        tool_spec(
            "roll_dice",
            "Roll one die (e.g. 1d6, 1d20). Returns the face and total. Call once per die or per person — multi-die expressions like 4d6 are not allowed.",
            serde_json::json!({
                "type": "object",
                "properties": {
                    "dice_expr": { "type": "string", "description": "Single-die expression only: 1d6, 1d20, etc. (not 2d6 or 4d6)" },
                    "label": { "type": "string", "description": "Short label for this roll (e.g. card effect, encounter, actor name)" }
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
    "set_variable",
    "clear_variable",
    "set_condition",
    "clear_condition",
    "set_measurement",
    "set_measurement_min",
    "set_measurement_max",
    "clear_measurement",
    "set_sequence",
    "step_sequence",
    "clear_sequence",
];

/// Legacy tool names kept for backward compatibility with older model outputs.
const LEGACY_SIMPLE_STATE_TOOL_ALIASES: &[&str] = &[
    "set_fact",
    "clear_fact",
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

fn float_state_tool(name: &str, description: &str, amount_field: &str, amount_desc: &str) -> Value {
    let value_prop = serde_json::json!({
        amount_field: { "type": "number", "description": amount_desc }
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

fn measurement_tool(name: &str, description: &str) -> Value {
    tool_spec(
        name,
        description,
        serde_json::json!({
            "type": "object",
            "properties": state_target_key_props(serde_json::json!({
                "value": { "type": "number", "description": "The measurement value (a float)." },
                "unit": { "type": "string", "description": "Optional unit label (UCUM code like cm, kg, or custom like stress)." }
            })),
            "required": ["target", "key", "value"]
        }),
    )
}

fn sequence_set_tool() -> Value {
    tool_spec(
        "set_sequence",
        "Define or replace an ordered sequence with a cursor (turn order, quest steps, queue). items must be a non-empty array of string labels.",
        serde_json::json!({
            "type": "object",
            "properties": state_target_key_props(serde_json::json!({
                "items": { "type": "array", "items": { "type": "string" }, "description": "Ordered labels, at least one." },
                "position": { "type": "integer", "description": "Active index (defaults to 0)." },
                "loop": { "type": "boolean", "description": "Whether step_sequence wraps at the ends." }
            })),
            "required": ["target", "key", "items"]
        }),
    )
}

/// Specs for the simple per-intent state tools offered to the inline prose agent.
pub fn simple_state_tool_specs() -> Vec<Value> {
    vec![
        text_state_tool(
            "set_variable",
            "Default for most state updates. Record or update a durable text variable (location, mood, inventory, traits, quest stage, appearance, body measurements). Call whenever narration establishes or changes a lasting attribute — the tool is the source of truth, not the prose.",
        ),
        clear_state_tool(
            "clear_variable",
            "Remove a durable variable that is no longer true (an item is dropped, a location is left behind).",
        ),
        text_state_tool(
            "set_condition",
            "Record or update an ephemeral status expected to clear soon (hidden, bleeding, suspicious, inspired) — not durable mood, location, or inventory (use set_variable for those).",
        ),
        clear_state_tool(
            "clear_condition",
            "Remove a temporary status once it ends (no longer hidden, bleeding stops).",
        ),
        measurement_tool(
            "set_measurement",
            "Set a float measurement (stress, height, arousal, distance). Unbounded by default — use set_measurement_min/max only when bounds matter. Never for text attributes like mood or appearance (use set_variable).",
        ),
        float_state_tool(
            "set_measurement_min",
            "Set the minimum bound for a measurement. Cleared only by clear_measurement.",
            "value",
            "Minimum allowed value.",
        ),
        float_state_tool(
            "set_measurement_max",
            "Set the maximum bound for a measurement. Cleared only by clear_measurement.",
            "value",
            "Maximum allowed value.",
        ),
        clear_state_tool(
            "clear_measurement",
            "Remove a measurement and its value, bounds, and unit.",
        ),
        sequence_set_tool(),
        float_state_tool(
            "step_sequence",
            "Advance (or rewind) the sequence cursor by delta steps. Wraps when loop=true on the sequence.",
            "delta",
            "Signed steps to move the cursor, e.g. 1 or -1.",
        ),
        clear_state_tool(
            "clear_sequence",
            "Remove a sequence entirely.",
        ),
    ]
}

/// Tools for the legacy single-pass inline-prose agent: scenario mechanics fired inline
/// plus tracked-state updates. Retained for the reproduction harness and tests.
#[cfg(test)]
pub fn inline_prose_tool_specs() -> Vec<Value> {
    let mut tools = inline_mechanical_tool_specs();
    tools.extend(simple_state_tool_specs());
    tools.push(present_fork_spec());
    tools
}

/// Tools for the mechanics-resolution pass: only the scenario mechanic primitives
/// (which return real outcomes) plus `present_fork`. No prose and no state tools —
/// this pass exists purely to roll/draw/move so the prose pass never has to guess an
/// outcome.
pub fn mechanics_agent_tool_specs() -> Vec<Value> {
    let mut tools = inline_mechanical_tool_specs();
    tools.push(present_fork_spec());
    tools
}

/// Tools for the narration pass: tracked-state updates plus `present_fork`. The
/// outcome-bearing mechanic tools are intentionally excluded — every roll/draw/move was
/// already resolved in the mechanics pass, so the narration must reuse those canonical
/// results rather than producing new ones.
pub fn prose_agent_tool_specs() -> Vec<Value> {
    let mut tools = simple_state_tool_specs();
    tools.push(present_fork_spec());
    tools
}

pub const PRESENT_FORK_TOOL: &str = "present_fork";

/// A CYOA-style branch: in-world situation plus concrete PC options.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PcFork {
    pub situation: String,
    pub options: Vec<String>,
}

pub fn is_present_fork_tool(name: &str) -> bool {
    name == PRESENT_FORK_TOOL
}

/// Parse `present_fork` tool arguments. Requires a non-empty situation and at least two options.
pub fn parse_present_fork_args(args: &Value) -> Option<PcFork> {
    let situation = args
        .get("situation")
        .and_then(|v| v.as_str())
        .map(str::trim)
        .filter(|s| !s.is_empty())?
    let options: Vec<String> = args
        .get("options")
        .and_then(|v| v.as_array())?
        .iter()
        .filter_map(|v| v.as_str().map(str::trim).filter(|s| !s.is_empty()))
        .map(str::to_string)
        .collect();
    if options.len() < 2 {
        return None;
    }
    Some(PcFork {
        situation: situation.to_string(),
        options,
    })
}

/// Render a fork as a blockquoted situation with numbered choices.
pub fn format_pc_fork_blockquote(fork: &PcFork) -> String {
    let mut lines: Vec<String> = fork
        .situation
        .lines()
        .map(|line| format!("> {line}"))
        .collect();
    lines.push(">".to_string());
    for (i, option) in fork.options.iter().enumerate() {
        lines.push(format!("> {}. {option}", i + 1));
    }
    lines.join("\n")
}

/// Whether a tool name resolves a scenario mechanic with a real, server-decided outcome
/// (dice/board/card). These must never be narrated before the tool runs.
pub fn is_outcome_tool(name: &str) -> bool {
    matches!(name, "roll_dice" | "board_move" | "draw_card")
}

fn present_fork_spec() -> Value {
    tool_spec(
        PRESENT_FORK_TOOL,
        &format!(
            "End the turn at a concrete in-world fork when the PC must choose something not specified in the player action or GM guidance. Call only after narrating up to the decision point. Provide the situation in second person and at least two concrete PC actions — never open-ended meta questions. Ends the turn immediately; do not narrate the PC's choice or continue past the fork. Never present choices for an NPC.\n\n{PRESENT_FORK_RULES}"
        ),
        serde_json::json!({
            "type": "object",
            "properties": {
                "situation": {
                    "type": "string",
                    "description": "Second-person description of the fork the PC faces (e.g. 'The corridor splits; voices echo from the right, torchlight flickers left.')."
                },
                "options": {
                    "type": "array",
                    "items": { "type": "string" },
                    "minItems": 2,
                    "description": "At least two concrete actions the PC could take (e.g. 'Sneak down the left passage', 'Call out down the right'). PC actions only — never NPC choices."
                }
            },
            "required": ["situation", "options"]
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
                let message = parse_dice_expr(dice_expr)
                    .filter(|(count, _)| *count > 1)
                    .map(|_| {
                        "only single-die expressions (e.g. 1d6) are allowed — call roll_dice once per die or person"
                    })
                    .unwrap_or("invalid dice expression");
                return Ok(error_result(message));
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
    name == "apply_state_changes"
        || SIMPLE_STATE_TOOLS.contains(&name)
        || LEGACY_SIMPLE_STATE_TOOL_ALIASES.contains(&name)
}

/// Parse any state tool call into validated state-change requests. Handles both the
/// batch `apply_state_changes` array and the simple per-intent tools (set_variable, etc).
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

/// Translate a simple per-intent state tool call (set_variable, adjust_resource, …) into
/// a single `StateChangeRequest`. Returns `None` for unknown tools or missing key.
fn parse_simple_state_tool(call: &ToolCall) -> Option<dreamwell_types::StateChangeRequest> {
    use dreamwell_types::{StateChangeRequest, StateKind, StateOp};
    use dreamwell_units::normalize_unit;

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
    let float_value = args.get("value").and_then(value_as_f64);
    let float_min = args.get("value").and_then(value_as_f64);
    let float_max = args.get("value").and_then(value_as_f64);
    let unit = args
        .get("unit")
        .and_then(value_as_string)
        .and_then(|u| normalize_unit(Some(&u)));
    let sequence_items = args.get("items").and_then(|v| {
        v.as_array().map(|arr| {
            arr.iter()
                .filter_map(|item| item.as_str().map(str::to_string))
                .collect::<Vec<_>>()
        })
    });
    let sequence_position = args.get("position").and_then(value_as_i64);
    let sequence_loop = args.get("loop").and_then(|v| v.as_bool());

    let req = |kind: StateKind, op: StateOp| StateChangeRequest {
        target: target.clone(),
        kind,
        key: key.clone(),
        op,
        value: None,
        delta: None,
        float_value: None,
        float_min: None,
        float_max: None,
        unit: None,
        sequence_items: None,
        sequence_position: None,
        sequence_loop: None,
    };

    match call.name.as_str() {
        "set_variable" | "set_fact" => {
            let mut r = req(StateKind::Variable, StateOp::Set);
            r.value = value;
            Some(r)
        }
        "clear_variable" | "clear_fact" => Some(req(StateKind::Variable, StateOp::Remove)),
        "set_condition" => {
            let mut r = req(StateKind::Condition, StateOp::Set);
            r.value = value;
            Some(r)
        }
        "clear_condition" => Some(req(StateKind::Condition, StateOp::Remove)),
        "set_measurement" | "set_resource" => {
            let mut r = req(StateKind::Measurement, StateOp::Set);
            r.float_value = float_value.or_else(|| delta_field.map(|d| d as f64));
            r.unit = unit;
            Some(r)
        }
        "set_measurement_min" => {
            let mut r = req(StateKind::Measurement, StateOp::SetMin);
            r.float_min = float_min;
            Some(r)
        }
        "set_measurement_max" => {
            let mut r = req(StateKind::Measurement, StateOp::SetMax);
            r.float_max = float_max;
            Some(r)
        }
        "clear_measurement" => Some(req(StateKind::Measurement, StateOp::Remove)),
        "set_sequence" | "set_clock" => {
            let mut r = req(StateKind::Sequence, StateOp::Set);
            r.sequence_items = sequence_items;
            r.sequence_position = sequence_position
                .or_else(|| value.as_deref().and_then(|s| s.trim().parse::<i64>().ok()));
            r.sequence_loop = sequence_loop;
            Some(r)
        }
        "step_sequence" | "advance_clock" => {
            let mut r = req(StateKind::Sequence, StateOp::Step);
            r.delta = delta_field.or(Some(1));
            Some(r)
        }
        "clear_sequence" => Some(req(StateKind::Sequence, StateOp::Remove)),
        "adjust_resource" => {
            let mut r = req(StateKind::Measurement, StateOp::Add);
            r.delta = delta_field;
            Some(r)
        }
        _ => None,
    }
}

fn value_as_f64(value: &Value) -> Option<f64> {
    match value {
        Value::Number(n) => n.as_f64(),
        Value::String(s) => s.trim().parse().ok(),
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

    #[tokio::test]
    async fn roll_dice_rejects_multi_die_expression() {
        let mut state = sample_state();
        let call = ToolCall {
            id: "t1".into(),
            name: "roll_dice".into(),
            arguments: r#"{"dice_expr":"4d6","label":"group roll"}"#.into(),
        };
        let result = handle_mechanical_tool_call(&mut state, &call)
            .await
            .unwrap();
        assert_eq!(
            result.get("error").and_then(|v| v.as_str()),
            Some("only single-die expressions (e.g. 1d6) are allowed — call roll_dice once per die or person")
        );
        assert!(state.mechanical_results.is_empty());
    }

    fn state_call(name: &str, args: serde_json::Value) -> ToolCall {
        ToolCall {
            id: "s1".into(),
            name: name.into(),
            arguments: args.to_string(),
        }
    }

    #[test]
    fn present_fork_spec_forbids_npc_options() {
        let spec = present_fork_spec();
        let description = spec["function"]["description"].as_str().unwrap();
        assert!(description.contains("Never present choices for an NPC"));
        assert!(description.contains("What should Sarah do next?"));
        let options_desc = spec["function"]["parameters"]["properties"]["options"]["description"]
            .as_str()
            .unwrap();
        assert!(options_desc.contains("Never NPC choices"));
    }

    #[test]
    fn parse_present_fork_args_requires_two_options() {
        assert!(parse_present_fork_args(&serde_json::json!({
            "situation": "The path splits.",
            "options": ["Go left", "Go right"]
        }))
        .is_some());
        assert!(parse_present_fork_args(&serde_json::json!({
            "situation": "The path splits.",
            "options": ["Go left"]
        }))
        .is_none());
    }

    #[test]
    fn format_pc_fork_blockquote_numbers_options() {
        let fork = PcFork {
            situation: "The corridor splits.".into(),
            options: vec!["Sneak left".into(), "Call out right".into()],
        };
        let block = format_pc_fork_blockquote(&fork);
        assert!(block.contains("> The corridor splits."));
        assert!(block.contains("> 1. Sneak left"));
        assert!(block.contains("> 2. Call out right"));
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
        assert!(names.iter().any(|n| n == "present_fork"));
    }

    #[test]
    fn is_state_tool_recognizes_simple_and_batch() {
        assert!(is_state_tool("apply_state_changes"));
        assert!(is_state_tool("set_variable"));
        assert!(is_state_tool("set_fact"));
        assert!(is_state_tool("advance_clock"));
        assert!(!is_state_tool("roll_dice"));
    }

    #[test]
    fn is_outcome_tool_recognizes_mechanics_only() {
        assert!(is_outcome_tool("roll_dice"));
        assert!(is_outcome_tool("board_move"));
        assert!(is_outcome_tool("draw_card"));
        assert!(!is_outcome_tool("set_fact"));
        assert!(!is_outcome_tool("present_fork"));
    }

    #[test]
    fn mechanics_specs_offer_only_mechanics_and_ask() {
        let names: Vec<String> = mechanics_agent_tool_specs()
            .iter()
            .filter_map(|t| t["function"]["name"].as_str().map(str::to_string))
            .collect();
        assert!(names.iter().any(|n| n == "roll_dice"));
        assert!(names.iter().any(|n| n == "board_move"));
        assert!(names.iter().any(|n| n == "draw_card"));
        assert!(names.iter().any(|n| n == "present_fork"));
        // No state tools in the mechanics pass — state is recorded during narration.
        for tool in SIMPLE_STATE_TOOLS {
            assert!(!names.iter().any(|n| n == tool), "unexpected {tool}");
        }
    }

    #[test]
    fn prose_specs_offer_state_and_ask_but_no_outcome_tools() {
        let names: Vec<String> = prose_agent_tool_specs()
            .iter()
            .filter_map(|t| t["function"]["name"].as_str().map(str::to_string))
            .collect();
        for tool in SIMPLE_STATE_TOOLS {
            assert!(names.iter().any(|n| n == tool), "missing {tool}");
        }
        assert!(names.iter().any(|n| n == "present_fork"));
        // Outcome tools are intentionally excluded from the narration pass.
        assert!(!names.iter().any(|n| is_outcome_tool(n)));
    }

    #[test]
    fn set_variable_maps_to_variable_set_request() {
        use dreamwell_types::{StateKind, StateOp};
        let call = state_call(
            "set_variable",
            serde_json::json!({ "target": "world", "key": "location", "value": "tavern" }),
        );
        let reqs = parse_state_tool_call(&call);
        assert_eq!(reqs.len(), 1);
        assert_eq!(reqs[0].target, "world");
        assert_eq!(reqs[0].kind, StateKind::Variable);
        assert_eq!(reqs[0].op, StateOp::Set);
        assert_eq!(reqs[0].key, "location");
        assert_eq!(reqs[0].value.as_deref(), Some("tavern"));
    }

    #[test]
    fn set_fact_alias_maps_to_variable_set_request() {
        use dreamwell_types::{StateKind, StateOp};
        let call = state_call(
            "set_fact",
            serde_json::json!({ "target": "world", "key": "location", "value": "tavern" }),
        );
        let reqs = parse_state_tool_call(&call);
        assert_eq!(reqs.len(), 1);
        assert_eq!(reqs[0].kind, StateKind::Variable);
        assert_eq!(reqs[0].op, StateOp::Set);
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
        assert_eq!(reqs[0].kind, StateKind::Measurement);
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
        assert_eq!(reqs[0].float_value, Some(3.0));
    }

    #[test]
    fn set_clock_accepts_stringified_number() {
        use dreamwell_types::StateKind;
        let call = state_call(
            "set_clock",
            serde_json::json!({ "target": "world", "key": "alarm", "value": "2" }),
        );
        let reqs = parse_state_tool_call(&call);
        assert_eq!(reqs[0].kind, StateKind::Sequence);
        assert_eq!(reqs[0].sequence_position, Some(2));
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
