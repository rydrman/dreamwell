//! Live reproduction harness for the "dice rolls narrated before the tool runs"
//! problem. Not part of the running server — it is `#[cfg(test)]` only and the
//! live tests are `#[ignore]`d so they never run in CI. Run manually with:
//!
//! ```bash
//! FEATHERLESS_API_KEY=... cargo test -p dreamwell-server --release \
//!     game_repro -- --ignored --nocapture
//! ```
//!
//! The harness rebuilds a minimal in-memory game (no DB) and drives the same
//! inline-prose tool loop the worker uses, recording the *order* in which prose
//! and tool calls arrive so we can see whether the model fabricates a dice
//! result in prose before calling `roll_dice`.
#![cfg(test)]

use std::collections::HashMap;

use chrono::Utc;
use dreamwell_types::{
    BoardDef, CardDef, DeckDef, EngineMode, Game, GameActor, GameDetail, GameElementsConfig,
    GameTurn, GameTurnCheck, JsonFormatStrategy, ResolutionSystem, RulesBlock, Settings,
};

use crate::game_prompts::build_inline_prose_agent_messages;
use crate::game_tools::{
    handle_mechanical_tool_call, inline_prose_tool_specs, is_state_tool, parse_state_tool_call,
    ToolSessionState,
};
use crate::inference::{InferenceConfig, ToolStreamChunk};
use crate::tool_stream::{
    resolve_tool_parser, salvage_bare_tool_calls, tool_definitions_from_specs, JailEvent,
    ToolStreamJail,
};
use futures_util::StreamExt;

const MODEL: &str = "llmfan46/gemma-4-31B-it-qat-q4_0-unquantized-uncensored-heretic";
const BASE_URL: &str = "https://api.featherless.ai/v1";

fn api_key() -> String {
    std::env::var("FEATHERLESS_API_KEY").unwrap_or_else(|_| {
        "rc_8ed7051d5a085e92e40ed92bec64b64e859b5ab24323593546cac55ee0cfb03d".to_string()
    })
}

fn inference_config() -> InferenceConfig {
    InferenceConfig::with_connection(
        BASE_URL.to_string(),
        Some(api_key()),
        None,
        JsonFormatStrategy::Auto,
        "auto".to_string(),
    )
}

fn repro_settings() -> Settings {
    Settings {
        inference_url: BASE_URL.to_string(),
        active_connection_id: None,
        connections: Vec::new(),
        model: MODEL.to_string(),
        temperature: 0.7,
        top_p: 1.0,
        max_tokens: 1200,
        system_prompt_prefix: String::new(),
        system_prompt_suffix: String::new(),
        user_name: "Alex".into(),
        persona_description: String::new(),
        summarize_enabled: false,
        summarize_adaptive: false,
        summarize_after_messages: 12,
        summarize_keep_recent: 4,
        variables_enabled: false,
        thought_blocks_enabled: false,
        max_context_messages: 0,
        context_tokens: 0,
        auto_context_on_model_change: false,
        max_concurrent_jobs: 1,
    }
}

fn base_game(premise: &str, rules: Vec<RulesBlock>, elements: GameElementsConfig) -> Game {
    Game {
        id: 1,
        title: "Repro".into(),
        premise: premise.into(),
        setting: "Pulpy tabletop adventure, second person, brisk pacing.".into(),
        gm_style: "Clear, concise narration. Resolve the action, then stop.".into(),
        opening_message: "The torchlit chamber waits.".into(),
        character_id: None,
        scenario_id: None,
        resolution_system: ResolutionSystem::Pbta2d6,
        modifier_min: -2,
        modifier_max: 3,
        merge_resolve_scene: true,
        step_mode: false,
        engine_mode: EngineMode::ToolsStructured,
        game_elements: elements,
        element_instances: Default::default(),
        model_checks: String::new(),
        model_resolve: String::new(),
        model_prose: String::new(),
        rules_blocks: rules,
        state_schema: vec![],
        win_condition: None,
        scenario_triggers: vec![],
        trait_defs: vec![],
        created_at: Utc::now(),
        updated_at: Utc::now(),
        archived_at: None,
        active_job: None,
        queued_jobs: 0,
    }
}

fn pc_actor(name: &str, description: &str, skills: &[(&str, i64)]) -> GameActor {
    GameActor {
        id: 1,
        game_id: 1,
        role: "pc".into(),
        name: name.into(),
        description: description.into(),
        skills: skills
            .iter()
            .map(|(k, v)| (k.to_string(), *v))
            .collect::<HashMap<_, _>>(),
        sort_order: 0,
        created_at: Utc::now(),
        updated_at: Utc::now(),
    }
}

fn opening_turn(prose: &str) -> GameTurn {
    GameTurn {
        id: 1,
        game_id: 1,
        sort_order: -1,
        player_action: String::new(),
        guidance_notes: String::new(),
        phase: "done".into(),
        scene_beats: vec![],
        prose: prose.into(),
        thought_content: String::new(),
        thought_duration_ms: None,
        thought_in_progress: false,
        state_changes: vec![],
        checks: vec![],
        system_rolls: vec![],
        plan: None,
        mechanical_results: vec![],
        observability: Default::default(),
        is_opening: true,
        generation_error: None,
        created_at: Utc::now(),
        updated_at: Utc::now(),
    }
}

fn action_turn(id: i64, action: &str) -> GameTurn {
    GameTurn {
        id,
        game_id: 1,
        sort_order: id,
        player_action: action.into(),
        guidance_notes: String::new(),
        phase: "prose".into(),
        scene_beats: vec![],
        prose: String::new(),
        thought_content: String::new(),
        thought_duration_ms: None,
        thought_in_progress: false,
        state_changes: vec![],
        checks: vec![],
        system_rolls: vec![],
        plan: None,
        mechanical_results: vec![],
        observability: Default::default(),
        is_opening: false,
        generation_error: None,
        created_at: Utc::now(),
        updated_at: Utc::now(),
    }
}

/// One ordered event in a recorded turn transcript.
#[derive(Debug, Clone)]
enum ReproEvent {
    /// Prose committed to the turn (as the user would see it) before this point.
    Prose(String),
    /// A tool call the model emitted, with the result we fed back.
    ToolCall { name: String, args: String, result: String },
}

struct ReproTurn {
    events: Vec<ReproEvent>,
    final_prose: String,
}

impl ReproTurn {
    /// Heuristic: does any prose chunk that precedes a `roll_dice` / `board_move`
    /// / `draw_card` tool call already commit to an outcome number or card name?
    fn pre_tool_outcome_leaks(&self) -> Vec<String> {
        let mut leaks = Vec::new();
        let mut prose_so_far = String::new();
        for event in &self.events {
            match event {
                ReproEvent::Prose(text) => prose_so_far.push_str(text),
                ReproEvent::ToolCall { name, .. } => {
                    if matches!(name.as_str(), "roll_dice" | "board_move" | "draw_card") {
                        // Look only at the lead-up since the previous tool call.
                        let lead = prose_so_far.clone();
                        if let Some(snippet) = outcome_leak_snippet(&lead, name) {
                            leaks.push(format!("[{name}] leaked: …{snippet}…"));
                        }
                    }
                    prose_so_far.clear();
                }
            }
        }
        leaks
    }
}

/// Crude detector for an outcome committed in lead-up prose. For dice/board, a
/// spelled-out or digit number near roll/step language; for cards, a quoted or
/// capitalized card name. Intentionally conservative — this is a diagnostic aid.
fn outcome_leak_snippet(lead: &str, tool: &str) -> Option<String> {
    let lower = lead.to_lowercase();
    let number_words = [
        "one", "two", "three", "four", "five", "six", "seven", "eight", "nine", "ten", "eleven",
        "twelve",
    ];
    let has_digit = lead.chars().any(|c| c.is_ascii_digit());
    let has_number_word = number_words.iter().any(|w| {
        lower
            .split(|c: char| !c.is_ascii_alphabetic())
            .any(|tok| tok == *w)
    });
    let outcome_hint = match tool {
        "roll_dice" => {
            lower.contains("roll")
                || lower.contains("lands on")
                || lower.contains("comes up")
                || lower.contains("shows")
                || lower.contains("die")
        }
        "board_move" => {
            lower.contains("space") || lower.contains("step") || lower.contains("move")
        }
        _ => false,
    };
    if (has_digit || has_number_word) && outcome_hint {
        let tail: String = lead.chars().rev().take(90).collect::<String>();
        let tail: String = tail.chars().rev().collect();
        return Some(tail.replace('\n', " "));
    }
    None
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Strategy {
    /// Current behavior: read the whole assistant message, then run all batched tools.
    Baseline,
    /// Interrupt the stream the moment an outcome-bearing mechanic tool call is
    /// parsed; drop trailing content; execute; feed back; continue.
    Interrupt,
}

fn is_outcome_tool(name: &str) -> bool {
    matches!(name, "roll_dice" | "board_move" | "draw_card")
}

async fn exec_tool(
    session: &mut ToolSessionState,
    tc: &crate::inference::ToolCall,
) -> serde_json::Value {
    if is_outcome_tool(&tc.name) {
        handle_mechanical_tool_call(session, tc)
            .await
            .unwrap_or_else(|e| serde_json::json!({ "error": e.to_string() }))
    } else if is_state_tool(&tc.name) {
        serde_json::json!({ "applied": parse_state_tool_call(tc).len() })
    } else if tc.name == "ask_pc_decision" {
        serde_json::json!({ "ended": true })
    } else {
        serde_json::json!({ "error": "unknown" })
    }
}

/// Run a single inline-prose turn against the live model, with no DB, recording
/// the order of prose vs tool calls. Mirrors the worker's loop closely enough to
/// reproduce the batching/ordering behavior.
async fn run_repro_turn(
    game: &Game,
    detail: &GameDetail,
    turn: &GameTurn,
    checks: &[GameTurnCheck],
    guidance: &str,
    settings: &Settings,
    strategy: Strategy,
) -> ReproTurn {
    let config = inference_config();
    let mut messages =
        build_inline_prose_agent_messages(game, detail, turn, checks, guidance, settings);
    let tools = inline_prose_tool_specs();
    let tool_defs = tool_definitions_from_specs(&tools);
    let parser = resolve_tool_parser(&config.tool_call_parser, MODEL);
    let mut session = ToolSessionState::new(game.clone());

    let mut events: Vec<ReproEvent> = Vec::new();
    let mut prose = String::new();
    let mut ended = false;

    for _iteration in 0..12 {
        if ended {
            break;
        }
        let mut stream = match crate::inference::stream_chat_completion_with_tools(
            &config,
            MODEL,
            &messages,
            &tools,
            &serde_json::json!("auto"),
            settings.temperature,
            settings.top_p,
            settings.max_tokens,
        )
        .await
        {
            Ok(s) => s,
            Err(err) => {
                eprintln!("stream error: {err}");
                break;
            }
        };

        let mut jail = ToolStreamJail::new(parser);
        let mut iteration_content = String::new();
        let mut pending: Vec<crate::inference::ToolCall> = Vec::new();
        let mut prose_before_this_chunk = String::new();
        let mut interrupted = false;

        'stream: while let Some(chunk) = stream.next().await {
            match chunk {
                Ok(ToolStreamChunk::Content(tok)) => {
                    iteration_content.push_str(&tok);
                    for event in jail.push(&tok, Some(&tool_defs)).await.unwrap_or_default() {
                        match event {
                            JailEvent::Prose(piece) => {
                                prose.push_str(&piece);
                                prose_before_this_chunk.push_str(&piece);
                            }
                            JailEvent::ToolCall(tc) => {
                                let outcome = is_outcome_tool(&tc.name);
                                pending.push(tc);
                                if strategy == Strategy::Interrupt && outcome {
                                    interrupted = true;
                                    break 'stream;
                                }
                            }
                        }
                    }
                }
                Ok(ToolStreamChunk::Done { native_tool_calls, .. }) => {
                    pending.extend(native_tool_calls);
                }
                Err(err) => {
                    eprintln!("chunk error: {err}");
                    break;
                }
            }
        }
        // Drop the stream connection to stop further generation when interrupted.
        drop(stream);

        if !interrupted {
            for event in jail.finish(Some(&tool_defs)).await.unwrap_or_default() {
                match event {
                    JailEvent::Prose(piece) => {
                        prose.push_str(&piece);
                        prose_before_this_chunk.push_str(&piece);
                    }
                    JailEvent::ToolCall(tc) => pending.push(tc),
                }
            }
            let (salvaged, cleaned) = salvage_bare_tool_calls(&prose, Some(&tool_defs));
            if !salvaged.is_empty() {
                prose = cleaned;
                for call in salvaged {
                    if !pending
                        .iter()
                        .any(|tc| tc.name == call.name && tc.arguments == call.arguments)
                    {
                        pending.push(call);
                    }
                }
            }
        }

        if !prose_before_this_chunk.trim().is_empty() {
            events.push(ReproEvent::Prose(prose_before_this_chunk.clone()));
        }

        if pending.is_empty() {
            break;
        }

        let assistant_tool_calls: Vec<serde_json::Value> = pending
            .iter()
            .map(|tc| {
                serde_json::json!({
                    "id": tc.id,
                    "type": "function",
                    "function": { "name": tc.name, "arguments": tc.arguments }
                })
            })
            .collect();
        messages.push(serde_json::json!({
            "role": "assistant",
            "content": if iteration_content.is_empty() { serde_json::Value::Null } else { serde_json::json!(iteration_content) },
            "tool_calls": assistant_tool_calls
        }));

        for tc in &pending {
            let result = exec_tool(&mut session, tc).await;
            let result_str = serde_json::to_string(&result).unwrap_or_default();
            events.push(ReproEvent::ToolCall {
                name: tc.name.clone(),
                args: tc.arguments.clone(),
                result: result_str.clone(),
            });
            messages.push(serde_json::json!({
                "role": "tool",
                "tool_call_id": tc.id,
                "content": result_str
            }));
            if tc.name == "ask_pc_decision" {
                ended = true;
            }
        }
    }

    let (_, cleaned) = salvage_bare_tool_calls(&prose, Some(&tool_defs));
    ReproTurn {
        events,
        final_prose: cleaned,
    }
}

const MECHANICS_PASS_SYSTEM: &str = r#"You are the rules engine for ONE turn of a tabletop scenario. Your ONLY job is to resolve the mechanical steps the player's action triggers by calling tools. You do not narrate.

Hard rules:
- Output tool calls ONLY. Write NO prose, NO narration, NO commentary, NO numbers in text.
- Call board_move, draw_card, and roll_dice exactly as the scenario rules require, ONE per step, and wait for each tool's real returned result before deciding the next call.
- Never state or guess an outcome — the tool decides it. React to the tool's actual result.
- For movement on a board use board_move (it rolls the move die for you). Never pair roll_dice with board_move for the same move. Use roll_dice only for dice the rules call for outside board movement (card effects, damage, skill rolls).
- draw_card needs an explicit deck_id chosen per the scenario rules.
- Resolve only the mechanics for THIS player action / pending effect, then STOP. Do not start a new beat, extra moves, or extra draws the player did not take.
- If the player must make a choice before a mechanic can resolve, call ask_pc_decision and stop.
- When every required mechanic is resolved (or none is needed), reply with exactly DONE and call no tool.

Tool call syntax (use this exact format): call:tool_name{key:value,key2:value2}"#;

const PROSE_PASS_SYSTEM: &str = r#"You are the narrator for ONE turn of a tabletop scenario. The turn's mechanical outcomes have ALREADY been rolled and resolved — they are listed under "Resolved mechanics (canonical)" in the user message.

Hard rules:
- Narrate in second person ("you"/"your") for the PC. NPCs use third person.
- Use the resolved mechanics EXACTLY: the same dice numbers, the same drawn card names/text, the same board spaces. Do NOT invent, change, re-roll, or add any dice roll, card draw, or board move. There is nothing left to roll.
- If "Resolved mechanics (canonical)" is empty, this is pure narration with no mechanical outcomes.
- Honor any resolved check tiers — a fail must not read as a clean success.
- Advance ONE beat: resolve the player's action and the resolved mechanics, then stop. Do not invent new arrivals, scene changes, or follow-on actions.
- Record durable state with the state tools (set_fact / set_condition / adjust_resource / advance_clock, etc.) — the tool is the source of truth, not the prose. One attribute per call.
- When the PC owes a choice the player did not make, call ask_pc_decision with a direct question, then stop.
- Plain prose and state/ask tool calls only — no JSON, no headers, no meta commentary.

Tool call syntax (use this exact format): call:tool_name{key:value,key2:value2}"#;

fn tool_named(name: &str) -> Option<serde_json::Value> {
    inline_prose_tool_specs()
        .into_iter()
        .find(|t| t["function"]["name"].as_str() == Some(name))
}

fn mechanics_pass_tools() -> Vec<serde_json::Value> {
    let mut tools = crate::game_tools::inline_mechanical_tool_specs();
    if let Some(ask) = tool_named("ask_pc_decision") {
        tools.push(ask);
    }
    tools
}

fn prose_pass_tools() -> Vec<serde_json::Value> {
    let mut tools = crate::game_tools::simple_state_tool_specs();
    if let Some(ask) = tool_named("ask_pc_decision") {
        tools.push(ask);
    }
    tools
}

fn format_resolved_mechanics(results: &[dreamwell_types::MechanicalResult]) -> String {
    use dreamwell_types::MechanicalData;
    if results.is_empty() {
        return "Resolved mechanics (canonical):\n(none — pure narration)".to_string();
    }
    let lines: Vec<String> = results
        .iter()
        .map(|r| match &r.data {
            MechanicalData::DiceRoll { dice_expr, rolls, total } => {
                format!("- {} ({dice_expr}): rolled {rolls:?} = {total}", r.label)
            }
            MechanicalData::BoardMove { actor, roll, from_space, to_space, space_tags, .. } => {
                format!(
                    "- Board move: {actor} rolled {roll}, moved space {from_space} → {to_space} (tags: {})",
                    space_tags.join(", ")
                )
            }
            MechanicalData::CardDraw { name, text, deck_id, .. } => {
                format!("- Card drawn from {deck_id}: {name} — {text}")
            }
        })
        .collect();
    format!("Resolved mechanics (canonical):\n{}", lines.join("\n"))
}

/// Two-pass turn: resolve all mechanics first (tools only, no prose), then
/// narrate from the canonical results (no outcome tools offered).
async fn run_repro_turn_two_pass(
    game: &Game,
    detail: &GameDetail,
    turn: &GameTurn,
    checks: &[GameTurnCheck],
    guidance: &str,
    settings: &Settings,
) -> ReproTurn {
    let config = inference_config();
    let parser = resolve_tool_parser(&config.tool_call_parser, MODEL);
    let base = build_inline_prose_agent_messages(game, detail, turn, checks, guidance, settings);
    let base_user = base[1]["content"].as_str().unwrap_or_default().to_string();
    let mut session = ToolSessionState::new(game.clone());
    let mut events: Vec<ReproEvent> = Vec::new();

    // ---- Pass 1: mechanics only ----
    let mech_tools = mechanics_pass_tools();
    let mech_defs = tool_definitions_from_specs(&mech_tools);
    let mut mech_messages = vec![
        serde_json::json!({ "role": "system", "content": MECHANICS_PASS_SYSTEM }),
        serde_json::json!({ "role": "user", "content": format!("{base_user}\n\nResolve this turn's mechanics now. Tool calls only — no prose.") }),
    ];
    let mut asked = false;
    'mech: for _ in 0..10 {
        let mut stream = match crate::inference::stream_chat_completion_with_tools(
            &config, MODEL, &mech_messages, &mech_tools, &serde_json::json!("auto"),
            settings.temperature, settings.top_p, settings.max_tokens,
        ).await {
            Ok(s) => s,
            Err(err) => { eprintln!("mech stream error: {err}"); break; }
        };
        let mut jail = ToolStreamJail::new(parser);
        let mut content = String::new();
        let mut pending: Vec<crate::inference::ToolCall> = Vec::new();
        let mut leaked_prose = String::new();
        while let Some(chunk) = stream.next().await {
            match chunk {
                Ok(ToolStreamChunk::Content(tok)) => {
                    content.push_str(&tok);
                    for ev in jail.push(&tok, Some(&mech_defs)).await.unwrap_or_default() {
                        match ev {
                            JailEvent::Prose(p) => leaked_prose.push_str(&p),
                            JailEvent::ToolCall(tc) => pending.push(tc),
                        }
                    }
                }
                Ok(ToolStreamChunk::Done { native_tool_calls, .. }) => pending.extend(native_tool_calls),
                Err(err) => { eprintln!("mech chunk error: {err}"); break; }
            }
        }
        for ev in jail.finish(Some(&mech_defs)).await.unwrap_or_default() {
            match ev {
                JailEvent::Prose(p) => leaked_prose.push_str(&p),
                JailEvent::ToolCall(tc) => pending.push(tc),
            }
        }
        let (salvaged, _) = salvage_bare_tool_calls(&leaked_prose, Some(&mech_defs));
        for c in salvaged {
            if !pending.iter().any(|tc| tc.name == c.name && tc.arguments == c.arguments) {
                pending.push(c);
            }
        }
        if !leaked_prose.trim().is_empty() {
            events.push(ReproEvent::Prose(format!("[mech-pass leaked prose] {}", leaked_prose.trim())));
        }
        if pending.is_empty() {
            break;
        }
        let assistant_calls: Vec<serde_json::Value> = pending.iter().map(|tc| serde_json::json!({
            "id": tc.id, "type": "function", "function": { "name": tc.name, "arguments": tc.arguments }
        })).collect();
        mech_messages.push(serde_json::json!({
            "role": "assistant",
            "content": if content.is_empty() { serde_json::Value::Null } else { serde_json::json!(content) },
            "tool_calls": assistant_calls
        }));
        for tc in &pending {
            let result = exec_tool(&mut session, tc).await;
            let result_str = serde_json::to_string(&result).unwrap_or_default();
            events.push(ReproEvent::ToolCall { name: tc.name.clone(), args: tc.arguments.clone(), result: result_str.clone() });
            mech_messages.push(serde_json::json!({ "role": "tool", "tool_call_id": tc.id, "content": result_str }));
            if tc.name == "ask_pc_decision" { asked = true; break 'mech; }
        }
    }

    // ---- Pass 2: prose from canonical results ----
    let results_block = format_resolved_mechanics(&session.mechanical_results);
    let prose_tools = prose_pass_tools();
    let prose_defs = tool_definitions_from_specs(&prose_tools);
    let mut prose_messages = vec![
        serde_json::json!({ "role": "system", "content": PROSE_PASS_SYSTEM }),
        serde_json::json!({ "role": "user", "content": format!("{base_user}\n\n{results_block}\n\nNarrate this turn now using exactly the resolved mechanics above. Do not roll, draw, or move anything — that is already done.") }),
    ];
    let mut prose = String::new();
    if asked {
        // The mechanics pass paused for a player choice; surface that as the turn end.
        events.push(ReproEvent::Prose("[turn paused for ask_pc_decision in mechanics pass]".into()));
    }
    for _ in 0..4 {
        let mut stream = match crate::inference::stream_chat_completion_with_tools(
            &config, MODEL, &prose_messages, &prose_tools, &serde_json::json!("auto"),
            settings.temperature, settings.top_p, settings.max_tokens,
        ).await {
            Ok(s) => s,
            Err(err) => { eprintln!("prose stream error: {err}"); break; }
        };
        let mut jail = ToolStreamJail::new(parser);
        let mut content = String::new();
        let mut pending: Vec<crate::inference::ToolCall> = Vec::new();
        let mut chunk_prose = String::new();
        while let Some(chunk) = stream.next().await {
            match chunk {
                Ok(ToolStreamChunk::Content(tok)) => {
                    content.push_str(&tok);
                    for ev in jail.push(&tok, Some(&prose_defs)).await.unwrap_or_default() {
                        match ev {
                            JailEvent::Prose(p) => { prose.push_str(&p); chunk_prose.push_str(&p); }
                            JailEvent::ToolCall(tc) => pending.push(tc),
                        }
                    }
                }
                Ok(ToolStreamChunk::Done { native_tool_calls, .. }) => pending.extend(native_tool_calls),
                Err(err) => { eprintln!("prose chunk error: {err}"); break; }
            }
        }
        for ev in jail.finish(Some(&prose_defs)).await.unwrap_or_default() {
            match ev {
                JailEvent::Prose(p) => { prose.push_str(&p); chunk_prose.push_str(&p); }
                JailEvent::ToolCall(tc) => pending.push(tc),
            }
        }
        let (salvaged, cleaned) = salvage_bare_tool_calls(&prose, Some(&prose_defs));
        if !salvaged.is_empty() {
            prose = cleaned;
            for c in salvaged {
                if !pending.iter().any(|tc| tc.name == c.name && tc.arguments == c.arguments) { pending.push(c); }
            }
        }
        // Prose pass must not re-resolve mechanics. Drop any outcome-tool calls
        // the model emits here (the fallback parser would otherwise execute them).
        pending.retain(|tc| !is_outcome_tool(&tc.name));
        if !chunk_prose.trim().is_empty() {
            events.push(ReproEvent::Prose(chunk_prose.clone()));
        }
        if pending.is_empty() { break; }
        let assistant_calls: Vec<serde_json::Value> = pending.iter().map(|tc| serde_json::json!({
            "id": tc.id, "type": "function", "function": { "name": tc.name, "arguments": tc.arguments }
        })).collect();
        prose_messages.push(serde_json::json!({
            "role": "assistant",
            "content": if content.is_empty() { serde_json::Value::Null } else { serde_json::json!(content) },
            "tool_calls": assistant_calls
        }));
        let mut ended = false;
        for tc in &pending {
            let result = exec_tool(&mut session, tc).await;
            let result_str = serde_json::to_string(&result).unwrap_or_default();
            events.push(ReproEvent::ToolCall { name: tc.name.clone(), args: tc.arguments.clone(), result: result_str.clone() });
            prose_messages.push(serde_json::json!({ "role": "tool", "tool_call_id": tc.id, "content": result_str }));
            if tc.name == "ask_pc_decision" { ended = true; }
        }
        if ended { break; }
    }

    let (_, cleaned) = salvage_bare_tool_calls(&prose, Some(&prose_defs));
    ReproTurn { events, final_prose: cleaned }
}

fn print_transcript(label: &str, turn: &ReproTurn) {
    println!("\n========== {label} ==========");
    for (i, event) in turn.events.iter().enumerate() {
        match event {
            ReproEvent::Prose(text) => {
                println!("  [{i}] PROSE: {}", text.trim().replace('\n', " ⏎ "));
            }
            ReproEvent::ToolCall { name, args, result } => {
                println!("  [{i}] TOOL  {name}({args}) -> {result}");
            }
        }
    }
    let leaks = turn.pre_tool_outcome_leaks();
    if leaks.is_empty() {
        println!("  >>> no pre-tool outcome leaks detected");
    } else {
        println!("  !!! {} pre-tool outcome leak(s):", leaks.len());
        for leak in &leaks {
            println!("      {leak}");
        }
    }
    println!("  --- final prose ---\n{}", turn.final_prose.trim());
}

fn combat_scenario() -> (Game, GameDetail, GameTurn) {
    let rules = vec![
        RulesBlock {
            name: "Combat".into(),
            content: "When the PC attacks an enemy, resolve the attack by rolling 1d6 with roll_dice: 1-2 miss, 3-4 graze (light hit), 5-6 solid hit. Narrate the result from the rolled number.".into(),
        },
        RulesBlock {
            name: "Damage".into(),
            content: "On a hit, roll 1d6 for damage and subtract it from the enemy's HP track.".into(),
        },
    ];
    let game = base_game(
        "A dungeon crawl. The PC, a sellsword, faces monsters in a torchlit ruin.",
        rules,
        GameElementsConfig::default(),
    );
    let mut detail = GameDetail {
        game: game.clone(),
        actors: vec![pc_actor(
            "Kael",
            "A scarred sellsword with a notched longsword.",
            &[("Force", 2), ("Finesse", 1)],
        )],
        state: vec![],
        turns: vec![opening_turn(
            "A goblin sentry hisses and raises a rusty blade across the chamber.",
        )],
        scenes: vec![],
    };
    let turn = action_turn(2, "I charge the goblin and swing my longsword at it.");
    detail.turns.push(turn.clone());
    (game, detail, turn)
}

fn board_scenario() -> (Game, GameDetail, GameTurn) {
    let elements = GameElementsConfig {
        boards: vec![BoardDef {
            id: "main".into(),
            spaces: 80,
            move_dice: "1d6".into(),
            tag_rules: vec![dreamwell_types::BoardTagRule {
                tag: "event".into(),
                spaces: (1..=80).collect(),
            }],
            default_tag: "event".into(),
        }],
        decks: vec![DeckDef {
            id: "events".into(),
            consume_on_draw: true,
            cards: vec![
                CardDef { id: "events:1".into(), name: "Boost".into(), text: "Move forward 2 extra spaces.".into() },
                CardDef { id: "events:2".into(), name: "Snake".into(), text: "Roll 1d6; on 4+ you slide back that many spaces.".into() },
            ],
        }],
    };
    let rules = vec![RulesBlock {
        name: "Turn sequence".into(),
        content: "On your turn: call board_move to roll the move die and advance, then draw_card from the events deck for the space you land on, then resolve the card.".into(),
    }];
    let game = base_game(
        "A cursed board game. Reach space 80. Each turn you move then draw an event.",
        rules,
        elements,
    );
    let mut detail = GameDetail {
        game: game.clone(),
        actors: vec![pc_actor("Jordan", "A curious player.", &[("Boldness", 2)])],
        state: vec![],
        turns: vec![opening_turn("The board glimmers. It is your turn to move.")],
        scenes: vec![],
    };
    let turn = action_turn(2, "I take my turn.");
    detail.turns.push(turn.clone());
    (game, detail, turn)
}

#[tokio::test]
#[ignore = "live model call; run manually"]
async fn repro_combat_dice() {
    let settings = repro_settings();
    let (game, detail, turn) = combat_scenario();
    for run in 0..3 {
        let result =
            run_repro_turn(&game, &detail, &turn, &[], "", &settings, Strategy::Baseline).await;
        print_transcript(&format!("combat BASELINE run {run}"), &result);
    }
}

#[tokio::test]
#[ignore = "live model call; run manually"]
async fn repro_board_then_card() {
    let settings = repro_settings();
    let (game, detail, turn) = board_scenario();
    for run in 0..3 {
        let result =
            run_repro_turn(&game, &detail, &turn, &[], "", &settings, Strategy::Baseline).await;
        print_transcript(&format!("board+card BASELINE run {run}"), &result);
    }
}

#[tokio::test]
#[ignore = "live model call; run manually"]
async fn experiment_interrupt_combat() {
    let settings = repro_settings();
    let (game, detail, turn) = combat_scenario();
    for run in 0..3 {
        let result =
            run_repro_turn(&game, &detail, &turn, &[], "", &settings, Strategy::Interrupt).await;
        print_transcript(&format!("combat INTERRUPT run {run}"), &result);
    }
}

#[tokio::test]
#[ignore = "live model call; run manually"]
async fn experiment_interrupt_board() {
    let settings = repro_settings();
    let (game, detail, turn) = board_scenario();
    for run in 0..3 {
        let result =
            run_repro_turn(&game, &detail, &turn, &[], "", &settings, Strategy::Interrupt).await;
        print_transcript(&format!("board+card INTERRUPT run {run}"), &result);
    }
}

#[tokio::test]
#[ignore = "live model call; run manually"]
async fn experiment_two_pass_combat() {
    let settings = repro_settings();
    let (game, detail, turn) = combat_scenario();
    for run in 0..3 {
        let result = run_repro_turn_two_pass(&game, &detail, &turn, &[], "", &settings).await;
        print_transcript(&format!("combat TWO-PASS run {run}"), &result);
    }
}

#[tokio::test]
#[ignore = "live model call; run manually"]
async fn experiment_two_pass_board() {
    let settings = repro_settings();
    let (game, detail, turn) = board_scenario();
    for run in 0..3 {
        let result = run_repro_turn_two_pass(&game, &detail, &turn, &[], "", &settings).await;
        print_transcript(&format!("board+card TWO-PASS run {run}"), &result);
    }
}
