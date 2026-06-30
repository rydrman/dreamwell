use std::time::{Duration, Instant};

use dreamwell_types::{
    AppliedStateChange, EngineMode, MechanicalResult, Settings, TurnObservability,
};
use futures_util::StreamExt;
use sqlx::SqlitePool;
use tokio_util::sync::CancellationToken;

use crate::db;
use crate::error::{AppError, AppResult};
use crate::game_mechanics::{flush_turn_mechanicals_streaming, persist_turn_mechanicals};
use crate::game_prompts::{build_inline_prose_agent_messages, build_prose_narration_messages};
use crate::game_tools::{
    format_pc_fork_blockquote, handle_mechanical_tool_call, inline_prose_tool_specs,
    is_author_notes_tool, is_outcome_tool, is_present_fork_tool, is_state_tool,
    parse_author_notes_args, parse_present_fork_args, parse_state_tool_call,
    prose_agent_tool_specs, PcFork, ToolSessionState,
};
use crate::game_turn::{declare_and_roll_checks, model_override_for_phase, GameModelPhase};
use crate::inference::{ToolCall, ToolLoopConfig, ToolStreamChunk};
use crate::model_fallback::stream_chat_completion_with_tools_connection_fallback;
use crate::thoughts::{parse_thought_blocks, thought_timing};
use crate::tool_stream::{
    resolve_tool_parser, salvage_bare_tool_calls, strip_residual_call_syntax,
    tool_definitions_from_specs, JailEvent, ToolStreamJail,
};

/// Outcome of a prose agent pass (inline or narration-only).
struct ProseOutcome {
    prose: String,
    applied_state: Vec<AppliedStateChange>,
    llm_calls: u32,
    tool_calls: u32,
}

pub async fn run_tools_structured_phase(
    pool: &SqlitePool,
    job_id: i64,
    game_id: i64,
    turn_id: i64,
    guidance: &str,
    settings: &Settings,
    token: &CancellationToken,
) -> AppResult<()> {
    let started = Instant::now();
    if token.is_cancelled() {
        return Err(AppError::bad_request("cancelled"));
    }

    // Phase 1 — decide & roll dramatic checks based on the PC's skills.
    db::update_turn_phase(pool, turn_id, "checks").await?;
    let checks =
        declare_and_roll_checks(pool, game_id, turn_id, guidance, settings, token, job_id).await?;
    db::update_turn_phase(pool, turn_id, "rolled").await?;

    if token.is_cancelled() {
        return Err(AppError::bad_request("cancelled"));
    }
    let detail = db::get_game_detail(pool, game_id).await?;
    let turn = db::get_turn(pool, game_id, turn_id).await?;
    let game = detail.game.clone();
    let model_override = model_override_for_phase(&game, settings, GameModelPhase::Prose);
    let parser = resolve_tool_parser(
        &db::get_inference_config(pool).await?.tool_call_parser,
        model_override
            .as_deref()
            .or_else(|| {
                let model = settings.model.trim();
                (!model.is_empty()).then_some(model)
            })
            .unwrap_or(""),
    );

    // Phase 2 — single narration pass that fires scenario mechanics inline.
    if token.is_cancelled() {
        return Err(AppError::bad_request("cancelled"));
    }
    db::update_turn_phase(pool, turn_id, "prose").await?;
    if settings.thought_blocks_enabled {
        db::clear_turn_thoughts(pool, turn_id).await?;
    }
    let mut session = ToolSessionState::new(game.clone());
    let prose = run_inline_prose_pass(
        pool,
        job_id,
        game_id,
        turn_id,
        &game,
        &detail,
        &turn,
        &checks,
        guidance,
        settings,
        model_override.as_deref(),
        parser,
        &mut session,
        token,
    )
    .await?;

    if !session.mechanical_results.is_empty() {
        persist_turn_mechanicals(
            pool,
            game_id,
            turn_id,
            &game,
            &session.mechanical_results,
            &session.instances,
        )
        .await?;
    }

    if !prose.applied_state.is_empty() {
        db::update_turn_state_changes(pool, turn_id, &prose.applied_state).await?;
        db::invalidate_scene_summaries_from(pool, game_id, turn.sort_order).await?;
    }

    let has_content = if settings.thought_blocks_enabled {
        let parsed = parse_thought_blocks(&prose.prose);
        !parsed.reply.trim().is_empty() || !parsed.thought.is_empty()
    } else {
        !prose.prose.trim().is_empty()
    };
    if !has_content {
        db::complete_job(
            pool,
            job_id,
            dreamwell_types::JobStatus::Failed,
            Some("model returned no prose".to_string()),
        )
        .await?;
        db::update_turn_phase(pool, turn_id, "failed").await?;
        return Ok(());
    }

    let elapsed = started.elapsed().as_millis() as u64;
    let llm_calls = prose.llm_calls;
    let tool_calls = prose.tool_calls;
    let obs = TurnObservability {
        engine_mode: EngineMode::ToolsStructured,
        llm_call_count: llm_calls,
        tool_call_count: tool_calls,
        tool_iterations: llm_calls,
        phase_timings_ms: [("structured".to_string(), elapsed)].into(),
    };
    db::merge_turn_observability(pool, turn_id, obs).await?;

    db::update_turn_phase(pool, turn_id, "done").await?;
    db::complete_job(pool, job_id, dreamwell_types::JobStatus::Completed, None).await?;
    db::touch_game(pool, game_id).await?;
    crate::game_summarize::maybe_enqueue_scene_summarize(pool, game_id).await?;
    Ok(())
}

/// Single-pass narration: prose and scenario mechanic tools interleaved inline.
#[allow(clippy::too_many_arguments)]
async fn run_inline_prose_pass(
    pool: &SqlitePool,
    job_id: i64,
    game_id: i64,
    turn_id: i64,
    game: &dreamwell_types::Game,
    detail: &dreamwell_types::GameDetail,
    turn: &dreamwell_types::GameTurn,
    checks: &[dreamwell_types::GameTurnCheck],
    guidance: &str,
    settings: &Settings,
    model_override: Option<&str>,
    parser: Option<&'static str>,
    session: &mut ToolSessionState,
    token: &CancellationToken,
) -> AppResult<ProseOutcome> {
    let mut messages =
        build_inline_prose_agent_messages(game, detail, turn, checks, guidance, settings);
    let tools = inline_prose_tool_specs();
    let tool_defs = tool_definitions_from_specs(&tools);
    let loop_config = ToolLoopConfig::default();

    let mut prose = String::new();
    for check in checks {
        append_inline_marker(
            &mut prose,
            dreamwell_types::prose_check_marker(check.sort_order),
        );
    }
    if !prose.is_empty() {
        db::update_turn_prose(pool, turn_id, &prose).await?;
        db::touch_game(pool, game_id).await?;
    }
    let mut applied_state: Vec<AppliedStateChange> = Vec::new();
    let mut llm_calls = 0u32;
    let mut tool_calls = 0u32;
    let mut end_turn = false;
    let mut prose_stream = ProseStreamState {
        last_flush: Instant::now(),
        thought_started_at: None,
        thought_duration_ms: None,
    };

    for _ in 0..loop_config.max_iterations {
        if token.is_cancelled() {
            return Err(AppError::bad_request("cancelled"));
        }
        let mut stream = stream_chat_completion_with_tools_connection_fallback(
            pool,
            settings,
            &messages,
            &tools,
            &serde_json::json!("auto"),
            Some(job_id),
            None,
            model_override,
        )
        .await?;
        llm_calls += 1;

        let mut jail = ToolStreamJail::new(parser);
        let mut iteration_content = String::new();
        let mut pending: Vec<ToolCall> = Vec::new();

        while let Some(chunk_result) = stream.next().await {
            if token.is_cancelled() {
                return Err(AppError::bad_request("cancelled"));
            }
            match chunk_result? {
                ToolStreamChunk::Content(token_str) => {
                    iteration_content.push_str(&token_str);
                    for event in jail.push(&token_str, Some(&tool_defs)).await? {
                        match event {
                            JailEvent::Prose(piece) => {
                                prose.push_str(&piece);
                                flush_prose_throttled(
                                    pool,
                                    game_id,
                                    turn_id,
                                    &prose,
                                    settings,
                                    &mut prose_stream,
                                    false,
                                )
                                .await?;
                            }
                            JailEvent::ToolCall(tc) => pending.push(tc),
                        }
                    }
                }
                ToolStreamChunk::Done {
                    native_tool_calls, ..
                } => pending.extend(native_tool_calls),
            }
        }
        for event in jail.finish(Some(&tool_defs)).await? {
            match event {
                JailEvent::Prose(piece) => prose.push_str(&piece),
                JailEvent::ToolCall(tc) => pending.push(tc),
            }
        }
        let (salvaged, cleaned_prose) = salvage_bare_tool_calls(&prose, Some(&tool_defs));
        if !salvaged.is_empty() {
            prose = cleaned_prose;
            for call in salvaged {
                if !pending
                    .iter()
                    .any(|tc| tc.name == call.name && tc.arguments == call.arguments)
                {
                    pending.push(call);
                }
            }
        }
        flush_prose_throttled(
            pool,
            game_id,
            turn_id,
            &prose,
            settings,
            &mut prose_stream,
            true,
        )
        .await?;

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
            "content": if iteration_content.is_empty() {
                serde_json::Value::Null
            } else {
                serde_json::json!(iteration_content)
            },
            "tool_calls": assistant_tool_calls
        }));

        for tc in pending {
            tool_calls += 1;
            let tool_result = if is_outcome_tool(&tc.name) {
                let before = session.mechanical_results.len();
                let res = handle_mechanical_tool_call(session, &tc).await?;
                if session.mechanical_results.len() > before {
                    if let Some(last) = session.mechanical_results.last() {
                        append_inline_marker(
                            &mut prose,
                            dreamwell_types::prose_mech_marker(last.sort_order),
                        );
                    }
                    flush_turn_mechanicals_streaming(
                        pool,
                        game_id,
                        turn_id,
                        &session.mechanical_results,
                        &session.instances,
                    )
                    .await?;
                    flush_prose_throttled(
                        pool,
                        game_id,
                        turn_id,
                        &prose,
                        settings,
                        &mut prose_stream,
                        true,
                    )
                    .await?;
                }
                res
            } else if is_state_tool(&tc.name) {
                let changes = parse_state_tool_call(&tc);
                let fresh = db::get_game_detail(pool, game_id).await?;
                let applied = crate::game_state::apply_state_changes(
                    pool,
                    game_id,
                    turn_id,
                    &changes,
                    &fresh.actors,
                    &fresh.state,
                )
                .await?;
                let count = applied.len();
                let start_idx = applied_state.len() as i64;
                applied_state.extend(applied);
                for offset in 0..count {
                    append_inline_marker(
                        &mut prose,
                        dreamwell_types::prose_state_marker(start_idx + offset as i64),
                    );
                }
                if !applied_state.is_empty() {
                    db::update_turn_state_changes(pool, turn_id, &applied_state).await?;
                    db::touch_game(pool, game_id).await?;
                    flush_prose_throttled(
                        pool,
                        game_id,
                        turn_id,
                        &prose,
                        settings,
                        &mut prose_stream,
                        true,
                    )
                    .await?;
                }
                serde_json::json!({ "applied": count })
            } else if is_present_fork_tool(&tc.name) {
                let args: serde_json::Value = serde_json::from_str(&tc.arguments)
                    .unwrap_or(serde_json::Value::Object(Default::default()));
                if let Some(fork) = parse_present_fork_args(&args) {
                    append_fork_blockquote(&mut prose, &fork);
                    flush_prose_throttled(
                        pool,
                        game_id,
                        turn_id,
                        &prose,
                        settings,
                        &mut prose_stream,
                        true,
                    )
                    .await?;
                    end_turn = true;
                    serde_json::json!({ "ended": true })
                } else {
                    serde_json::json!({ "error": "present_fork requires a non-empty situation and at least two options" })
                }
            } else if is_author_notes_tool(&tc.name) {
                let args: serde_json::Value = serde_json::from_str(&tc.arguments)
                    .unwrap_or(serde_json::Value::Object(Default::default()));
                apply_author_notes_tool(pool, game_id, &args).await
            } else {
                serde_json::json!({ "error": format!("unknown tool {}", tc.name) })
            };
            messages.push(serde_json::json!({
                "role": "tool",
                "tool_call_id": tc.id,
                "content": serde_json::to_string(&tool_result).unwrap_or_else(|_| "{}".to_string())
            }));
            if end_turn {
                break;
            }
        }
        if end_turn {
            break;
        }
    }

    let (_, cleaned) = salvage_bare_tool_calls(&prose, Some(&tool_defs));
    prose = strip_residual_call_syntax(&cleaned);

    finalize_turn_prose(
        pool,
        turn_id,
        &prose,
        settings,
        &mut prose_stream.thought_started_at,
        &mut prose_stream.thought_duration_ms,
        true,
    )
    .await?;

    Ok(ProseOutcome {
        prose,
        applied_state,
        llm_calls,
        tool_calls,
    })
}

/// Narrate the turn from the already-resolved mechanics. Outcome-bearing tools are
/// not offered and any the model emits anyway are ignored, so the prose can only
/// reuse the canonical results rather than inventing new ones.
#[allow(clippy::too_many_arguments)]
async fn run_prose_pass(
    pool: &SqlitePool,
    job_id: i64,
    game_id: i64,
    turn_id: i64,
    game: &dreamwell_types::Game,
    detail: &dreamwell_types::GameDetail,
    turn: &dreamwell_types::GameTurn,
    checks: &[dreamwell_types::GameTurnCheck],
    resolved_mechanics: &[MechanicalResult],
    pending_fork: Option<&PcFork>,
    guidance: &str,
    settings: &Settings,
    model_override: Option<&str>,
    parser: Option<&'static str>,
    token: &CancellationToken,
) -> AppResult<ProseOutcome> {
    let mut messages = build_prose_narration_messages(
        game,
        detail,
        turn,
        checks,
        resolved_mechanics,
        guidance,
        settings,
    );
    let tools = prose_agent_tool_specs();
    let tool_defs = tool_definitions_from_specs(&tools);
    let loop_config = ToolLoopConfig::default();

    let mut prose = String::new();
    for check in checks {
        append_inline_marker(
            &mut prose,
            dreamwell_types::prose_check_marker(check.sort_order),
        );
    }
    if !prose.is_empty() {
        db::update_turn_prose(pool, turn_id, &prose).await?;
        db::touch_game(pool, game_id).await?;
    }
    let mut applied_state: Vec<AppliedStateChange> = Vec::new();
    let mut llm_calls = 0u32;
    let mut tool_calls = 0u32;
    let mut end_turn = false;
    let mut prose_stream = ProseStreamState {
        last_flush: Instant::now(),
        thought_started_at: None,
        thought_duration_ms: None,
    };

    for _ in 0..loop_config.max_iterations {
        if token.is_cancelled() {
            return Err(AppError::bad_request("cancelled"));
        }
        let mut stream = stream_chat_completion_with_tools_connection_fallback(
            pool,
            settings,
            &messages,
            &tools,
            &serde_json::json!("auto"),
            Some(job_id),
            None,
            model_override,
        )
        .await?;
        llm_calls += 1;

        let mut jail = ToolStreamJail::new(parser);
        let mut iteration_content = String::new();
        let mut pending: Vec<ToolCall> = Vec::new();

        while let Some(chunk_result) = stream.next().await {
            if token.is_cancelled() {
                return Err(AppError::bad_request("cancelled"));
            }
            match chunk_result? {
                ToolStreamChunk::Content(token_str) => {
                    iteration_content.push_str(&token_str);
                    for event in jail.push(&token_str, Some(&tool_defs)).await? {
                        match event {
                            JailEvent::Prose(piece) => {
                                prose.push_str(&piece);
                                flush_prose_throttled(
                                    pool,
                                    game_id,
                                    turn_id,
                                    &prose,
                                    settings,
                                    &mut prose_stream,
                                    false,
                                )
                                .await?;
                            }
                            JailEvent::ToolCall(tc) => pending.push(tc),
                        }
                    }
                }
                ToolStreamChunk::Done {
                    native_tool_calls, ..
                } => pending.extend(native_tool_calls),
            }
        }
        for event in jail.finish(Some(&tool_defs)).await? {
            match event {
                JailEvent::Prose(piece) => prose.push_str(&piece),
                JailEvent::ToolCall(tc) => pending.push(tc),
            }
        }
        let (salvaged, cleaned_prose) = salvage_bare_tool_calls(&prose, Some(&tool_defs));
        if !salvaged.is_empty() {
            prose = cleaned_prose;
            for call in salvaged {
                if !pending
                    .iter()
                    .any(|tc| tc.name == call.name && tc.arguments == call.arguments)
                {
                    pending.push(call);
                }
            }
        }
        // The narration pass must not re-resolve mechanics. Drop any outcome-tool
        // calls the model emits — the canonical results are already fixed.
        pending.retain(|tc| !is_outcome_tool(&tc.name));
        flush_prose_throttled(
            pool,
            game_id,
            turn_id,
            &prose,
            settings,
            &mut prose_stream,
            true,
        )
        .await?;

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
            "content": if iteration_content.is_empty() {
                serde_json::Value::Null
            } else {
                serde_json::json!(iteration_content)
            },
            "tool_calls": assistant_tool_calls
        }));

        for tc in pending {
            tool_calls += 1;
            let tool_result = if is_state_tool(&tc.name) {
                let changes = parse_state_tool_call(&tc);
                let fresh = db::get_game_detail(pool, game_id).await?;
                let applied = crate::game_state::apply_state_changes(
                    pool,
                    game_id,
                    turn_id,
                    &changes,
                    &fresh.actors,
                    &fresh.state,
                )
                .await?;
                let count = applied.len();
                let start_idx = applied_state.len() as i64;
                applied_state.extend(applied);
                for offset in 0..count {
                    append_inline_marker(
                        &mut prose,
                        dreamwell_types::prose_state_marker(start_idx + offset as i64),
                    );
                }
                if !applied_state.is_empty() {
                    db::update_turn_state_changes(pool, turn_id, &applied_state).await?;
                    db::touch_game(pool, game_id).await?;
                    flush_prose_throttled(
                        pool,
                        game_id,
                        turn_id,
                        &prose,
                        settings,
                        &mut prose_stream,
                        true,
                    )
                    .await?;
                }
                serde_json::json!({ "applied": count })
            } else if is_present_fork_tool(&tc.name) {
                let args: serde_json::Value = serde_json::from_str(&tc.arguments)
                    .unwrap_or(serde_json::Value::Object(Default::default()));
                if let Some(fork) = parse_present_fork_args(&args) {
                    append_fork_blockquote(&mut prose, &fork);
                    flush_prose_throttled(
                        pool,
                        game_id,
                        turn_id,
                        &prose,
                        settings,
                        &mut prose_stream,
                        true,
                    )
                    .await?;
                    end_turn = true;
                    serde_json::json!({ "ended": true })
                } else {
                    serde_json::json!({ "error": "present_fork requires a non-empty situation and at least two options" })
                }
            } else if is_author_notes_tool(&tc.name) {
                let args: serde_json::Value = serde_json::from_str(&tc.arguments)
                    .unwrap_or(serde_json::Value::Object(Default::default()));
                apply_author_notes_tool(pool, game_id, &args).await
            } else {
                serde_json::json!({ "error": format!("unknown tool {}", tc.name) })
            };
            messages.push(serde_json::json!({
                "role": "tool",
                "tool_call_id": tc.id,
                "content": serde_json::to_string(&tool_result).unwrap_or_else(|_| "{}".to_string())
            }));
            if end_turn {
                break;
            }
        }
        if end_turn {
            break;
        }
    }

    // If the mechanics pass paused for a player fork and the narrator did not
    // already surface it, append it so the turn ends on the open choice.
    if let Some(fork) = pending_fork {
        if !end_turn && !prose.contains(&fork.situation) {
            append_fork_blockquote(&mut prose, fork);
        }
    }

    let (_, cleaned) = salvage_bare_tool_calls(&prose, Some(&tool_defs));
    // Final safety net: drop any leaked/truncated `call:` tool-call syntax so it
    // never reaches the player as prose.
    prose = strip_residual_call_syntax(&cleaned);

    finalize_turn_prose(
        pool,
        turn_id,
        &prose,
        settings,
        &mut prose_stream.thought_started_at,
        &mut prose_stream.thought_duration_ms,
        true,
    )
    .await?;

    Ok(ProseOutcome {
        prose,
        applied_state,
        llm_calls,
        tool_calls,
    })
}

fn append_fork_blockquote(prose: &mut String, fork: &PcFork) {
    if !prose.is_empty() {
        prose.push_str("\n\n");
    }
    prose.push_str(&format_pc_fork_blockquote(fork));
}

async fn apply_author_notes_tool(
    pool: &SqlitePool,
    game_id: i64,
    args: &serde_json::Value,
) -> serde_json::Value {
    match parse_author_notes_args(args) {
        Some(notes) => match db::update_author_notes(pool, game_id, &notes).await {
            Ok(()) => serde_json::json!({ "saved": true, "length": notes.len() }),
            Err(err) => serde_json::json!({ "error": err.to_string() }),
        },
        None => serde_json::json!({ "error": "update_author_notes requires a notes string" }),
    }
}

fn append_inline_marker(prose: &mut String, marker: String) {
    if !prose.is_empty() {
        prose.push_str("\n\n");
    }
    prose.push_str(&marker);
}

struct ProseStreamState {
    last_flush: Instant,
    thought_started_at: Option<Instant>,
    thought_duration_ms: Option<i64>,
}

async fn flush_prose_throttled(
    pool: &SqlitePool,
    game_id: i64,
    turn_id: i64,
    prose: &str,
    settings: &Settings,
    state: &mut ProseStreamState,
    force: bool,
) -> AppResult<()> {
    const PROSE_FLUSH_INTERVAL: Duration = Duration::from_millis(120);
    if !force && state.last_flush.elapsed() < PROSE_FLUSH_INTERVAL {
        return Ok(());
    }
    finalize_turn_prose(
        pool,
        turn_id,
        prose,
        settings,
        &mut state.thought_started_at,
        &mut state.thought_duration_ms,
        false,
    )
    .await?;
    db::touch_game(pool, game_id).await?;
    state.last_flush = Instant::now();
    Ok(())
}

async fn finalize_turn_prose(
    pool: &SqlitePool,
    turn_id: i64,
    prose: &str,
    settings: &Settings,
    thought_started_at: &mut Option<Instant>,
    thought_duration_ms: &mut Option<i64>,
    thought_complete: bool,
) -> AppResult<()> {
    if settings.thought_blocks_enabled {
        let parsed = parse_thought_blocks(prose);
        let (duration_ms, in_progress) = if thought_complete {
            let (duration_ms, _) = thought_timing(&parsed, thought_started_at, thought_duration_ms);
            let final_duration = if parsed.thought.is_empty() {
                None
            } else {
                duration_ms
                    .or_else(|| thought_started_at.map(|start| start.elapsed().as_millis() as i64))
            };
            (final_duration, false)
        } else {
            thought_timing(&parsed, thought_started_at, thought_duration_ms)
        };
        db::update_turn_generation(
            pool,
            turn_id,
            &parsed.reply,
            &parsed.thought,
            duration_ms,
            in_progress,
        )
        .await?;
    } else {
        db::update_turn_prose(pool, turn_id, prose).await?;
    }
    Ok(())
}

/// Re-narrate a completed turn from its existing checks and mechanical results.
pub async fn run_prose_regenerate_job(
    pool: &SqlitePool,
    job_id: i64,
    game_id: i64,
    turn_id: i64,
    guidance: &str,
    settings: &Settings,
    token: &CancellationToken,
) -> AppResult<()> {
    let started = Instant::now();
    if token.is_cancelled() {
        return Err(AppError::bad_request("cancelled"));
    }

    let detail = db::get_game_detail(pool, game_id).await?;
    let turn = db::get_turn(pool, game_id, turn_id).await?;
    let game = detail.game.clone();
    let model_override = model_override_for_phase(&game, settings, GameModelPhase::Prose);
    let parser = resolve_tool_parser(
        &db::get_inference_config(pool).await?.tool_call_parser,
        model_override
            .as_deref()
            .or_else(|| {
                let model = settings.model.trim();
                (!model.is_empty()).then_some(model)
            })
            .unwrap_or(""),
    );

    db::update_turn_phase(pool, turn_id, "prose").await?;
    if settings.thought_blocks_enabled {
        db::clear_turn_thoughts(pool, turn_id).await?;
    }

    let prose = run_prose_pass(
        pool,
        job_id,
        game_id,
        turn_id,
        &game,
        &detail,
        &turn,
        &turn.checks,
        &turn.mechanical_results,
        None,
        guidance,
        settings,
        model_override.as_deref(),
        parser,
        token,
    )
    .await?;

    if !prose.applied_state.is_empty() {
        db::update_turn_state_changes(pool, turn_id, &prose.applied_state).await?;
        db::invalidate_scene_summaries_from(pool, game_id, turn.sort_order).await?;
    }

    let has_content = if settings.thought_blocks_enabled {
        let parsed = parse_thought_blocks(&prose.prose);
        !parsed.reply.trim().is_empty() || !parsed.thought.is_empty()
    } else {
        !prose.prose.trim().is_empty()
    };
    if !has_content {
        db::complete_job(
            pool,
            job_id,
            dreamwell_types::JobStatus::Failed,
            Some("model returned no prose".to_string()),
        )
        .await?;
        db::update_turn_phase(pool, turn_id, "failed").await?;
        return Ok(());
    }

    let elapsed = started.elapsed().as_millis() as u64;
    let obs = TurnObservability {
        engine_mode: EngineMode::ToolsStructured,
        llm_call_count: prose.llm_calls,
        tool_call_count: prose.tool_calls,
        tool_iterations: prose.llm_calls,
        phase_timings_ms: [("prose_regenerate".to_string(), elapsed)].into(),
    };
    db::merge_turn_observability(pool, turn_id, obs).await?;

    db::update_turn_phase(pool, turn_id, "done").await?;
    db::complete_job(pool, job_id, dreamwell_types::JobStatus::Completed, None).await?;
    db::touch_game(pool, game_id).await?;
    crate::game_summarize::maybe_enqueue_scene_summarize(pool, game_id).await?;
    Ok(())
}
