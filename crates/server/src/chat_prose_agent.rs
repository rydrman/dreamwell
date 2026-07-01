use std::time::{Duration, Instant};

use dreamwell_types::{AppliedStateChange, Settings};
use futures_util::StreamExt;
use sqlx::SqlitePool;
use tokio_util::sync::CancellationToken;

use crate::chat_state;
use crate::db;
use crate::error::{AppError, AppResult};
use crate::game_tools::{
    format_pc_fork_blockquote, is_author_notes_tool, is_present_fork_tool, is_state_tool,
    parse_present_fork_args, parse_state_tool_call, prose_agent_tool_specs, PcFork,
};
use crate::inference::{ToolCall, ToolLoopConfig, ToolStreamChunk};
use crate::model_fallback::stream_chat_completion_with_tools_connection_fallback;
use crate::thoughts::{parse_thought_blocks, thought_timing};
use crate::tool_stream::{
    resolve_tool_parser, salvage_bare_tool_calls, strip_residual_call_syntax,
    tool_definitions_from_specs, JailEvent, ToolStreamJail,
};

pub struct ChatProseOutcome {
    pub prose: String,
    pub applied_state: Vec<AppliedStateChange>,
}

/// Run the chat prose pass with the same state tools as game narration.
#[allow(clippy::too_many_arguments)]
pub async fn run_chat_prose_pass(
    pool: &SqlitePool,
    job_id: i64,
    chat_id: i64,
    message_id: i64,
    messages: Vec<serde_json::Value>,
    plan_state: &[AppliedStateChange],
    settings: &Settings,
    token: &CancellationToken,
) -> AppResult<ChatProseOutcome> {
    let tools = prose_agent_tool_specs();
    let tool_defs = tool_definitions_from_specs(&tools);
    let loop_config = ToolLoopConfig::default();
    let parser = resolve_tool_parser(
        &db::get_inference_config(pool).await?.tool_call_parser,
        settings.model.trim(),
    );

    let mut messages = messages;
    let mut prose = String::new();
    let mut applied_state = plan_state.to_vec();
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
            Some(message_id),
            None,
        )
        .await?;

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
                                flush_message_prose_throttled(
                                    pool,
                                    chat_id,
                                    message_id,
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
        flush_message_prose_throttled(
            pool,
            chat_id,
            message_id,
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
            let tool_result = if is_state_tool(&tc.name) {
                let changes = parse_state_tool_call(&tc);
                let actors = db::list_chat_actors(pool, chat_id).await?;
                let current = db::list_chat_state_entries(pool, chat_id).await?;
                let applied = chat_state::apply_state_changes(
                    pool, chat_id, message_id, &changes, &actors, &current,
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
                    db::update_message_state_changes(pool, message_id, &applied_state).await?;
                    db::touch_chat(pool, chat_id).await?;
                    flush_message_prose_throttled(
                        pool,
                        chat_id,
                        message_id,
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
                    flush_message_prose_throttled(
                        pool,
                        chat_id,
                        message_id,
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
                serde_json::json!({ "ok": true, "skipped": "author notes are not used in chat" })
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

    finalize_message_prose(
        pool,
        message_id,
        &prose,
        settings,
        &mut prose_stream.thought_started_at,
        &mut prose_stream.thought_duration_ms,
        true,
    )
    .await?;

    Ok(ChatProseOutcome {
        prose,
        applied_state,
    })
}

fn append_fork_blockquote(prose: &mut String, fork: &PcFork) {
    if !prose.is_empty() {
        prose.push_str("\n\n");
    }
    prose.push_str(&format_pc_fork_blockquote(fork));
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

async fn flush_message_prose_throttled(
    pool: &SqlitePool,
    chat_id: i64,
    message_id: i64,
    prose: &str,
    settings: &Settings,
    state: &mut ProseStreamState,
    force: bool,
) -> AppResult<()> {
    const PROSE_FLUSH_INTERVAL: Duration = Duration::from_millis(120);
    if !force && state.last_flush.elapsed() < PROSE_FLUSH_INTERVAL {
        return Ok(());
    }
    finalize_message_prose(
        pool,
        message_id,
        prose,
        settings,
        &mut state.thought_started_at,
        &mut state.thought_duration_ms,
        false,
    )
    .await?;
    db::touch_chat(pool, chat_id).await?;
    state.last_flush = Instant::now();
    Ok(())
}

async fn finalize_message_prose(
    pool: &SqlitePool,
    message_id: i64,
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
        db::update_message_generation(
            pool,
            message_id,
            &parsed.reply,
            &parsed.thought,
            duration_ms,
            in_progress,
        )
        .await?;
    } else {
        db::update_message_content(pool, message_id, prose).await?;
    }
    Ok(())
}
