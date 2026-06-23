use std::time::Instant;

use dreamwell_types::{EngineMode, Settings, TurnObservability};
use sqlx::SqlitePool;
use tokio_util::sync::CancellationToken;

use crate::db;
use crate::error::{AppError, AppResult};
use crate::game_mechanics::{execute_mechanicals, persist_turn_mechanicals};
use crate::game_prompts::build_structured_agent_messages;
use crate::game_tools::{
    handle_mechanical_tool_call, handle_structured_tool_call, mechanical_tool_specs,
    structured_tool_specs, ToolSessionState,
};
use crate::game_turn::{model_for_phase, GameModelPhase};
use crate::inference::ToolLoopConfig;

pub async fn run_tools_mechanics_phase(
    pool: &SqlitePool,
    game_id: i64,
    turn_id: i64,
    settings: &Settings,
    token: &CancellationToken,
) -> AppResult<TurnObservability> {
    let started = Instant::now();
    if token.is_cancelled() {
        return Err(AppError::bad_request("cancelled"));
    }
    let detail = db::get_game_detail(pool, game_id).await?;
    let turn = db::get_turn(pool, game_id, turn_id).await?;
    let game = detail.game.clone();
    if game.game_elements.turn_mechanicals.is_empty() {
        return Ok(TurnObservability {
            engine_mode: game.engine_mode,
            ..Default::default()
        });
    }

    let inference = db::get_inference_config(pool).await?;
    let model = model_for_phase(&game, settings, GameModelPhase::Resolve);
    let mut session = ToolSessionState::new(game.clone(), detail, turn);
    let tools = mechanical_tool_specs();
    let loop_config = ToolLoopConfig::default();

    let mut messages = build_structured_agent_messages(
        &session.game,
        &session.detail,
        &session.turn,
        "Execute the mechanical steps for this turn using the provided tools.",
        settings,
        false,
    );
    let mut iterations = 0u32;
    let mut tool_call_count = 0u32;
    for _ in 0..loop_config.max_iterations {
        if token.is_cancelled() {
            return Err(AppError::bad_request("cancelled"));
        }
        let result = crate::inference::chat_completion_with_tools(
            &inference,
            &model,
            &messages,
            &tools,
            &serde_json::json!("auto"),
            0.2,
            settings.top_p,
            loop_config.max_tokens_per_call,
        )
        .await?;
        iterations += 1;
        if result.tool_calls.is_empty() {
            break;
        }
        let assistant_tool_calls: Vec<serde_json::Value> = result
            .tool_calls
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
            "content": result.content,
            "tool_calls": assistant_tool_calls
        }));
        for tc in result.tool_calls {
            tool_call_count += 1;
            let tool_result = handle_mechanical_tool_call(&mut session, &tc).await?;
            messages.push(serde_json::json!({
                "role": "tool",
                "tool_call_id": tc.id,
                "content": serde_json::to_string(&tool_result).unwrap_or_else(|_| "{}".to_string())
            }));
        }
    }

    persist_turn_mechanicals(
        pool,
        game_id,
        turn_id,
        &game,
        &session.mechanical_results,
        &session.instances,
    )
    .await?;

    let elapsed = started.elapsed().as_millis() as u64;
    let obs = TurnObservability {
        engine_mode: EngineMode::ToolsMechanics,
        llm_call_count: iterations,
        tool_call_count,
        tool_iterations: iterations,
        phase_timings_ms: [("mechanics".to_string(), elapsed)].into(),
    };
    db::merge_turn_observability(pool, turn_id, obs.clone()).await?;
    Ok(obs)
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
    db::update_turn_phase(pool, turn_id, "structured").await?;
    let detail = db::get_game_detail(pool, game_id).await?;
    let turn = db::get_turn(pool, game_id, turn_id).await?;
    let game = detail.game.clone();
    let inference = db::get_inference_config(pool).await?;
    let model = model_for_phase(&game, settings, GameModelPhase::Resolve);

    let mut session = ToolSessionState::new(game.clone(), detail, turn);
    let mut messages = build_structured_agent_messages(
        &session.game,
        &session.detail,
        &session.turn,
        guidance,
        settings,
        true,
    );
    let tools = structured_tool_specs();
    let loop_config = ToolLoopConfig::default();
    let mut iterations = 0u32;
    let mut tool_call_count = 0u32;

    for _ in 0..loop_config.max_iterations {
        if token.is_cancelled() {
            return Err(AppError::bad_request("cancelled"));
        }
        let result = crate::inference::chat_completion_with_tools(
            &inference,
            &model,
            &messages,
            &tools,
            &serde_json::json!("auto"),
            0.3,
            settings.top_p,
            loop_config.max_tokens_per_call,
        )
        .await?;
        iterations += 1;
        if session.structured_complete {
            break;
        }
        if result.tool_calls.is_empty() {
            break;
        }
        let assistant_tool_calls: Vec<serde_json::Value> = result
            .tool_calls
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
            "content": result.content,
            "tool_calls": assistant_tool_calls
        }));
        for tc in result.tool_calls {
            tool_call_count += 1;
            let tool_result = handle_structured_tool_call(pool, &mut session, &tc, turn_id).await?;
            messages.push(serde_json::json!({
                "role": "tool",
                "tool_call_id": tc.id,
                "content": serde_json::to_string(&tool_result).unwrap_or_else(|_| "{}".to_string())
            }));
            if session.structured_complete {
                break;
            }
        }
    }

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
    } else if !game.game_elements.turn_mechanicals.is_empty() {
        let (results, instances) = execute_mechanicals(
            &game.game_elements,
            game.element_instances.clone(),
            &game.game_elements.turn_mechanicals,
            "pc",
        );
        persist_turn_mechanicals(pool, game_id, turn_id, &game, &results, &instances).await?;
    }

    db::clear_turn_checks(pool, turn_id).await?;
    for check in &session.rolled_checks {
        db::insert_turn_check(pool, turn_id, check).await?;
    }
    db::update_turn_scene_beats(pool, turn_id, &session.scene_beats).await?;
    if !session.state_changes.is_empty() {
        let applied = crate::game_state::apply_state_changes(
            pool,
            game_id,
            turn_id,
            &session.state_changes,
            &session.detail.actors,
            &session.detail.state,
        )
        .await?;
        db::update_turn_state_changes(pool, turn_id, &applied).await?;
    }

    let elapsed = started.elapsed().as_millis() as u64;
    let obs = TurnObservability {
        engine_mode: EngineMode::ToolsStructured,
        llm_call_count: iterations,
        tool_call_count,
        tool_iterations: iterations,
        phase_timings_ms: [("structured".to_string(), elapsed)].into(),
    };
    db::merge_turn_observability(pool, turn_id, obs).await?;

    db::complete_job(pool, job_id, dreamwell_types::JobStatus::Completed, None).await?;
    db::enqueue_game_job(
        pool,
        dreamwell_types::JobType::GameTurnProse,
        game_id,
        Some(turn_id),
        guidance.to_string(),
    )
    .await?;
    db::touch_game(pool, game_id).await?;
    Ok(())
}

pub async fn run_bulk_mechanicals(
    pool: &SqlitePool,
    game_id: i64,
    turn_id: i64,
    game: &dreamwell_types::Game,
) -> AppResult<()> {
    if game.game_elements.turn_mechanicals.is_empty() {
        return Ok(());
    }
    let ctx = game.element_instances.clone();
    let (results, instances) = execute_mechanicals(
        &game.game_elements,
        ctx,
        &game.game_elements.turn_mechanicals,
        "pc",
    );
    persist_turn_mechanicals(pool, game_id, turn_id, game, &results, &instances).await
}
