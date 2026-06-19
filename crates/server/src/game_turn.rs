use dreamwell_types::{
    DeclareChecksResponse, DeclaredCheck, GameTurnCheck, Job, JobType, Settings,
};
use futures_util::StreamExt;
use sqlx::SqlitePool;
use tokio_util::sync::CancellationToken;

use crate::config;
use crate::db;
use crate::error::{AppError, AppResult};
use crate::game_prompts::{
    build_declare_checks_messages, build_prose_messages, build_resolve_messages,
    declare_checks_schema, resolve_schema,
};
use crate::game_resolution::{clamp_modifier, roll_dice, tier_str};
use crate::game_state::{apply_state_changes, skill_modifier, validate_skill};
use crate::game_summarize::maybe_enqueue_scene_summarize;
use crate::inference::{chat_completion_json, stream_chat_completion};

fn max_retries() -> u32 {
    config::GENERATION_MAX_RETRIES
        .load(std::sync::atomic::Ordering::SeqCst)
        .max(1)
}

fn structured_output_tokens(settings: &Settings) -> i64 {
    if settings.context_tokens > 0 {
        (settings.context_tokens / 8).clamp(256, 768)
    } else {
        512
    }
}

pub async fn run_game_job(
    pool: &SqlitePool,
    job_id: i64,
    job: &Job,
    settings: &Settings,
    token: CancellationToken,
) -> AppResult<()> {
    match job.job_type {
        JobType::GameTurnCheck => run_turn_from_checks(pool, job_id, job, settings, &token).await,
        JobType::GameTurnResolve | JobType::GameTurnScenePlan => {
            run_turn_resolve(pool, job_id, job, settings, &token).await
        }
        JobType::GameTurnProse => run_turn_prose(pool, job_id, job, settings, &token).await,
        JobType::GameSceneSummarize => {
            crate::game_summarize::run_game_scene_summarize_job(
                pool,
                job_id,
                job.game_id.unwrap_or(0),
                settings,
            )
            .await
        }
        JobType::GameProseRecheck | JobType::GameStateRecheck => {
            db::complete_job(pool, job_id, dreamwell_types::JobStatus::Completed, None).await?;
            Ok(())
        }
        _ => Err(AppError::internal("not a game job")),
    }
}

async fn run_turn_from_checks(
    pool: &SqlitePool,
    job_id: i64,
    job: &Job,
    settings: &Settings,
    token: &CancellationToken,
) -> AppResult<()> {
    let game_id = job
        .game_id
        .ok_or_else(|| AppError::internal("game job missing game_id"))?;
    let turn_id = job
        .turn_id
        .ok_or_else(|| AppError::internal("turn job missing turn_id"))?;

    if token.is_cancelled() {
        return cancel_turn_job(pool, job).await;
    }

    db::update_turn_phase(pool, turn_id, "checks").await?;
    let detail = db::get_game_detail(pool, game_id).await?;
    let turn = db::get_turn(pool, game_id, turn_id).await?;
    let game = detail.game.clone();

    let messages = build_declare_checks_messages(&game, &detail, &turn, &job.guidance_notes);
    let declared: DeclareChecksResponse = chat_completion_json(
        &settings.inference_url,
        &settings.model,
        &messages,
        0.4,
        settings.top_p,
        structured_output_tokens(settings),
        Some(&declare_checks_schema()),
        max_retries(),
        token,
    )
    .await?;

    db::clear_turn_checks(pool, turn_id).await?;
    let pc = detail
        .actors
        .iter()
        .find(|a| a.role == "pc")
        .ok_or_else(|| AppError::internal("no PC actor"))?;

    let mut rolled_checks = Vec::new();
    for (i, check) in declared.checks.iter().enumerate() {
        let validated = validate_declared_check(check, pc, &game);
        let seed = turn_id * 1000 + i as i64 + 1;
        let roll = roll_dice("2d6", validated.modifier, seed);
        let game_check = GameTurnCheck {
            id: 0,
            turn_id,
            label: validated.label.clone(),
            skill: validated.skill.clone(),
            modifier: validated.modifier,
            stakes: validated.stakes.clone(),
            justification: validated.justification.clone(),
            dice_expr: "2d6".to_string(),
            seed,
            rolls: roll.as_ref().map(|r| r.rolls.clone()).unwrap_or_default(),
            total: roll.as_ref().map(|r| r.total).unwrap_or(0),
            tier: roll.as_ref().map(|r| r.tier),
            margin: roll.as_ref().map(|r| r.margin).unwrap_or(0),
            sort_order: i as i64,
            created_at: chrono::Utc::now(),
        };
        db::insert_turn_check(pool, turn_id, &game_check).await?;
        rolled_checks.push(game_check);
    }

    db::update_turn_phase(pool, turn_id, "rolled").await?;

    if game.step_mode {
        db::update_turn_phase(pool, turn_id, "rolled_pause").await?;
        db::complete_job(pool, job_id, dreamwell_types::JobStatus::Completed, None).await?;
        db::touch_game(pool, game_id).await?;
        return Ok(());
    }

    run_resolve_and_prose(pool, job_id, job, settings, token, game_id, turn_id).await
}

async fn run_turn_resolve(
    pool: &SqlitePool,
    job_id: i64,
    job: &Job,
    settings: &Settings,
    token: &CancellationToken,
) -> AppResult<()> {
    let game_id = job.game_id.unwrap_or(0);
    let turn_id = job.turn_id.unwrap_or(0);
    run_resolve_and_prose(pool, job_id, job, settings, token, game_id, turn_id).await
}

async fn run_resolve_and_prose(
    pool: &SqlitePool,
    job_id: i64,
    job: &Job,
    settings: &Settings,
    token: &CancellationToken,
    game_id: i64,
    turn_id: i64,
) -> AppResult<()> {
    if token.is_cancelled() {
        return cancel_turn_job(pool, job).await;
    }

    db::update_turn_phase(pool, turn_id, "resolved").await?;
    let detail = db::get_game_detail(pool, game_id).await?;
    let turn = db::get_turn(pool, game_id, turn_id).await?;
    let game = detail.game.clone();
    let checks = turn.checks.clone();

    let messages = build_resolve_messages(&game, &detail, &turn, &checks, &job.guidance_notes);
    let resolved: dreamwell_types::ResolveTurnResponse = chat_completion_json(
        &settings.inference_url,
        &settings.model,
        &messages,
        0.5,
        settings.top_p,
        structured_output_tokens(settings),
        Some(&resolve_schema()),
        max_retries(),
        token,
    )
    .await?;

    db::update_turn_scene_beats(pool, turn_id, &resolved.scene_beats).await?;

    let state_detail = db::get_game_detail(pool, game_id).await?;
    let applied = apply_state_changes(
        pool,
        game_id,
        turn_id,
        &resolved.state_changes,
        &state_detail.actors,
        &state_detail.state,
    )
    .await?;
    db::update_turn_state_changes(pool, turn_id, &applied).await?;
    db::invalidate_scene_summaries_from(pool, game_id, turn.sort_order).await?;

    if game.step_mode {
        db::update_turn_phase(pool, turn_id, "resolved_pause").await?;
        db::complete_job(pool, job_id, dreamwell_types::JobStatus::Completed, None).await?;
        db::touch_game(pool, game_id).await?;
        return Ok(());
    }

    stream_turn_prose(pool, job_id, job, settings, token, game_id, turn_id).await
}

async fn run_turn_prose(
    pool: &SqlitePool,
    job_id: i64,
    job: &Job,
    settings: &Settings,
    token: &CancellationToken,
) -> AppResult<()> {
    let game_id = job.game_id.unwrap_or(0);
    let turn_id = job.turn_id.unwrap_or(0);
    stream_turn_prose(pool, job_id, job, settings, token, game_id, turn_id).await
}

async fn stream_turn_prose(
    pool: &SqlitePool,
    job_id: i64,
    job: &Job,
    settings: &Settings,
    token: &CancellationToken,
    game_id: i64,
    turn_id: i64,
) -> AppResult<()> {
    if token.is_cancelled() {
        return cancel_turn_job(pool, job).await;
    }

    db::update_turn_phase(pool, turn_id, "prose").await?;
    let detail = db::get_game_detail(pool, game_id).await?;
    let turn = db::get_turn(pool, game_id, turn_id).await?;
    let messages = build_prose_messages(
        &detail.game,
        &detail,
        &turn,
        &turn.checks,
        &job.guidance_notes,
        settings,
    );

    let mut stream = stream_chat_completion(
        &settings.inference_url,
        &settings.model,
        &messages,
        settings.temperature,
        settings.top_p,
        settings.max_tokens,
    )
    .await?;

    let mut accumulated = String::new();
    while let Some(token_result) = stream.next().await {
        if token.is_cancelled() {
            return cancel_turn_job(pool, job).await;
        }
        match token_result {
            Ok(piece) => {
                accumulated.push_str(&piece);
                db::update_turn_prose(pool, turn_id, &accumulated).await?;
                db::touch_game(pool, game_id).await?;
            }
            Err(err) => {
                db::complete_job(
                    pool,
                    job_id,
                    dreamwell_types::JobStatus::Failed,
                    Some(err.to_string()),
                )
                .await?;
                db::update_turn_phase(pool, turn_id, "failed").await?;
                return Ok(());
            }
        }
    }

    if accumulated.trim().is_empty() {
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

    db::update_turn_phase(pool, turn_id, "done").await?;
    db::complete_job(pool, job_id, dreamwell_types::JobStatus::Completed, None).await?;
    db::touch_game(pool, game_id).await?;
    maybe_enqueue_scene_summarize(pool, game_id).await?;
    Ok(())
}

fn validate_declared_check(
    check: &DeclaredCheck,
    pc: &dreamwell_types::GameActor,
    game: &dreamwell_types::Game,
) -> DeclaredCheck {
    let validated_skill = validate_skill(&check.skill, pc);
    let sheet_mod = skill_modifier(&validated_skill, pc);
    let total_mod = clamp_modifier(
        check.modifier + sheet_mod,
        game.modifier_min,
        game.modifier_max,
    );
    DeclaredCheck {
        label: check.label.clone(),
        skill: validated_skill,
        modifier: total_mod,
        stakes: check.stakes.clone(),
        justification: check.justification.clone(),
    }
}

async fn cancel_turn_job(pool: &SqlitePool, job: &Job) -> AppResult<()> {
    db::complete_job(pool, job.id, dreamwell_types::JobStatus::Cancelled, None).await?;
    if let Some(turn_id) = job.turn_id {
        db::update_turn_phase(pool, turn_id, "failed").await?;
    }
    Ok(())
}

#[allow(dead_code)]
fn format_tier(check: &GameTurnCheck) -> String {
    check
        .tier
        .map(|t| tier_str(t).to_string())
        .unwrap_or_default()
}
