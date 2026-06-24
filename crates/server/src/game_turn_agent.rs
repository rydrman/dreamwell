use std::time::{Duration, Instant};

use dreamwell_types::{AppliedStateChange, EngineMode, Settings, TurnObservability};
use futures_util::StreamExt;
use sqlx::SqlitePool;
use tokio_util::sync::CancellationToken;

use crate::db;
use crate::error::{AppError, AppResult};
use crate::game_mechanics::{flush_turn_mechanicals_streaming, persist_turn_mechanicals};
use crate::game_prompts::build_inline_prose_agent_messages;
use crate::game_tools::{
    handle_mechanical_tool_call, inline_prose_tool_specs, parse_state_change_args, ToolSessionState,
};
use crate::game_turn::{declare_and_roll_checks, model_for_phase, GameModelPhase};
use crate::inference::{stream_chat_completion_with_tools, ToolLoopConfig, ToolStreamChunk};
use crate::tool_stream::{
    resolve_tool_parser, salvage_bare_tool_calls, tool_definitions_from_specs, JailEvent,
    ToolStreamJail,
};

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
    let checks = declare_and_roll_checks(pool, game_id, turn_id, guidance, settings, token).await?;
    db::update_turn_phase(pool, turn_id, "rolled").await?;

    // Phase 2 — single narration pass that fires scenario mechanics inline.
    if token.is_cancelled() {
        return Err(AppError::bad_request("cancelled"));
    }
    db::update_turn_phase(pool, turn_id, "prose").await?;
    let detail = db::get_game_detail(pool, game_id).await?;
    let turn = db::get_turn(pool, game_id, turn_id).await?;
    let game = detail.game.clone();
    let inference = db::get_inference_config(pool).await?;
    let model = model_for_phase(&game, settings, GameModelPhase::Prose);

    let mut session = ToolSessionState::new(game.clone());
    let mut messages =
        build_inline_prose_agent_messages(&game, &detail, &turn, &checks, guidance, settings);
    let tools = inline_prose_tool_specs();
    let loop_config = ToolLoopConfig::default();

    let mut prose = String::new();
    for check in &checks {
        append_check_marker(&mut prose, check.sort_order);
    }
    if !prose.is_empty() {
        db::update_turn_prose(pool, turn_id, &prose).await?;
        db::touch_game(pool, game_id).await?;
    }
    let mut applied_state: Vec<AppliedStateChange> = Vec::new();
    let mut llm_calls = 0u32;
    let mut tool_call_count = 0u32;
    let mut end_turn_early = false;
    let parser = resolve_tool_parser(&inference.tool_call_parser, &model);
    let tool_defs = tool_definitions_from_specs(&tools);
    let mut jail = ToolStreamJail::new(parser);
    let mut last_prose_flush = Instant::now();
    const PROSE_FLUSH_INTERVAL: Duration = Duration::from_millis(120);

    for _ in 0..loop_config.max_iterations {
        if token.is_cancelled() {
            return Err(AppError::bad_request("cancelled"));
        }
        let mut stream = stream_chat_completion_with_tools(
            &inference,
            &model,
            &messages,
            &tools,
            &serde_json::json!("auto"),
            settings.temperature,
            settings.top_p,
            loop_config.max_tokens_per_call,
        )
        .await?;
        llm_calls += 1;

        let mut iteration_content = String::new();
        let mut pending_tool_calls = Vec::new();

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
                                    &mut last_prose_flush,
                                    PROSE_FLUSH_INTERVAL,
                                    false,
                                )
                                .await?;
                            }
                            JailEvent::ToolCall(tc) => pending_tool_calls.push(tc),
                        }
                    }
                }
                ToolStreamChunk::Done {
                    native_tool_calls, ..
                } => {
                    pending_tool_calls.extend(native_tool_calls);
                }
            }
        }

        for event in jail.finish(Some(&tool_defs)).await? {
            match event {
                JailEvent::Prose(piece) => prose.push_str(&piece),
                JailEvent::ToolCall(tc) => pending_tool_calls.push(tc),
            }
        }
        let (salvaged, cleaned_prose) = salvage_bare_tool_calls(&prose, Some(&tool_defs));
        if !salvaged.is_empty() {
            prose = cleaned_prose;
            for call in salvaged {
                let duplicate = pending_tool_calls
                    .iter()
                    .any(|tc| tc.name == call.name && tc.arguments == call.arguments);
                if !duplicate {
                    pending_tool_calls.push(call);
                }
            }
        }
        flush_prose_throttled(
            pool,
            game_id,
            turn_id,
            &prose,
            &mut last_prose_flush,
            PROSE_FLUSH_INTERVAL,
            true,
        )
        .await?;

        if pending_tool_calls.is_empty() {
            break;
        }

        let assistant_tool_calls: Vec<serde_json::Value> = pending_tool_calls
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

        for tc in pending_tool_calls {
            tool_call_count += 1;
            let tool_result = match tc.name.as_str() {
                "roll_dice" | "board_move" | "draw_card" => {
                    let before = session.mechanical_results.len();
                    let res = handle_mechanical_tool_call(&mut session, &tc).await?;
                    if session.mechanical_results.len() > before {
                        if let Some(last) = session.mechanical_results.last() {
                            append_marker(&mut prose, last.sort_order);
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
                                &mut last_prose_flush,
                                PROSE_FLUSH_INTERVAL,
                                true,
                            )
                            .await?;
                        }
                    }
                    res
                }
                "apply_state_changes" => {
                    let changes = parse_state_change_args(&tc);
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
                    for (offset, _) in (0..count).enumerate() {
                        append_state_marker(&mut prose, start_idx + offset as i64);
                    }
                    if !applied_state.is_empty() {
                        db::update_turn_state_changes(pool, turn_id, &applied_state).await?;
                        db::touch_game(pool, game_id).await?;
                        flush_prose_throttled(
                            pool,
                            game_id,
                            turn_id,
                            &prose,
                            &mut last_prose_flush,
                            PROSE_FLUSH_INTERVAL,
                            true,
                        )
                        .await?;
                    }
                    serde_json::json!({ "applied": count })
                }
                "ask_pc_decision" => {
                    let args: serde_json::Value = serde_json::from_str(&tc.arguments)
                        .unwrap_or(serde_json::Value::Object(Default::default()));
                    let question = args["question"]
                        .as_str()
                        .filter(|s| !s.trim().is_empty())
                        .unwrap_or("What do you do?");
                    if !prose.is_empty() {
                        prose.push_str("\n\n");
                    }
                    prose.push_str(&format_blockquote(question));
                    flush_prose_throttled(
                        pool,
                        game_id,
                        turn_id,
                        &prose,
                        &mut last_prose_flush,
                        PROSE_FLUSH_INTERVAL,
                        true,
                    )
                    .await?;
                    end_turn_early = true;
                    serde_json::json!({ "ended": true })
                }
                other => serde_json::json!({ "error": format!("unknown tool {other}") }),
            };
            messages.push(serde_json::json!({
                "role": "tool",
                "tool_call_id": tc.id,
                "content": serde_json::to_string(&tool_result).unwrap_or_else(|_| "{}".to_string())
            }));
            if end_turn_early {
                break;
            }
        }
        if end_turn_early {
            break;
        }
    }

    // Persist mechanics fired inline (with updated board/deck instances) only.
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

    if !applied_state.is_empty() {
        db::update_turn_state_changes(pool, turn_id, &applied_state).await?;
        db::invalidate_scene_summaries_from(pool, game_id, turn.sort_order).await?;
    }

    if prose.trim().is_empty() {
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

    let (_, cleaned_prose) = salvage_bare_tool_calls(&prose, Some(&tool_defs));
    prose = cleaned_prose;
    db::update_turn_prose(pool, turn_id, &prose).await?;

    let elapsed = started.elapsed().as_millis() as u64;
    let obs = TurnObservability {
        engine_mode: EngineMode::ToolsStructured,
        llm_call_count: llm_calls,
        tool_call_count,
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

/// Append an inline mechanic-block marker anchoring the result at this point in the prose.
fn append_marker(prose: &mut String, sort_order: i64) {
    if !prose.is_empty() {
        prose.push_str("\n\n");
    }
    prose.push_str(&dreamwell_types::prose_mech_marker(sort_order));
}

fn append_state_marker(prose: &mut String, index: i64) {
    if !prose.is_empty() {
        prose.push_str("\n\n");
    }
    prose.push_str(&dreamwell_types::prose_state_marker(index));
}

fn append_check_marker(prose: &mut String, sort_order: i64) {
    if !prose.is_empty() {
        prose.push_str("\n\n");
    }
    prose.push_str(&dreamwell_types::prose_check_marker(sort_order));
}

fn format_blockquote(text: &str) -> String {
    text.lines()
        .map(|line| format!("> {line}"))
        .collect::<Vec<_>>()
        .join("\n")
}

async fn flush_prose_throttled(
    pool: &SqlitePool,
    game_id: i64,
    turn_id: i64,
    prose: &str,
    last_flush: &mut Instant,
    interval: Duration,
    force: bool,
) -> AppResult<()> {
    if !force && last_flush.elapsed() < interval {
        return Ok(());
    }
    db::update_turn_prose(pool, turn_id, prose).await?;
    db::touch_game(pool, game_id).await?;
    *last_flush = Instant::now();
    Ok(())
}
