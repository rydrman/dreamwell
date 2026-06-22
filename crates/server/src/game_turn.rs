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
use crate::game_resolution::{clamp_modifier, roll_dice};
use crate::game_state::{apply_state_changes, skill_modifier, validate_skill};
use crate::game_summarize::maybe_enqueue_scene_summarize;
use crate::inference::{chat_completion_json, stream_chat_completion};

#[derive(Debug, Clone, Copy)]
pub enum GameModelPhase {
    Checks,
    Resolve,
    Prose,
}

pub fn model_for_phase(
    game: &dreamwell_types::Game,
    settings: &Settings,
    phase: GameModelPhase,
) -> String {
    let override_model = match phase {
        GameModelPhase::Checks => &game.model_checks,
        GameModelPhase::Resolve => &game.model_resolve,
        GameModelPhase::Prose => &game.model_prose,
    };
    if !override_model.trim().is_empty() {
        override_model.clone()
    } else {
        settings.model.clone()
    }
}

pub fn ensure_model_for_phase(
    game: &dreamwell_types::Game,
    settings: &Settings,
    phase: GameModelPhase,
) -> AppResult<()> {
    if model_for_phase(game, settings, phase).trim().is_empty() {
        return Err(AppError::bad_request(
            "No model selected in settings (or game phase override)",
        ));
    }
    Ok(())
}

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
        JobType::GameProseRecheck => {
            let game_id = job
                .game_id
                .ok_or_else(|| AppError::internal("prose recheck job missing game_id"))?;
            let turn_id = job
                .turn_id
                .ok_or_else(|| AppError::internal("prose recheck job missing turn_id"))?;
            crate::game_prose_recheck::run_turn_prose_recheck_job(
                pool,
                job_id,
                game_id,
                turn_id,
                &job.guidance_notes,
                settings,
            )
            .await
        }
        JobType::GameStateRecheck => {
            let game_id = job
                .game_id
                .ok_or_else(|| AppError::internal("state recheck job missing game_id"))?;
            let turn_id = job
                .turn_id
                .ok_or_else(|| AppError::internal("state recheck job missing turn_id"))?;
            crate::game_state_recheck::run_turn_state_recheck_job(
                pool,
                job_id,
                game_id,
                turn_id,
                &job.guidance_notes,
                settings,
            )
            .await
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
    let inference = db::get_inference_config(pool).await?;
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

    let messages =
        build_declare_checks_messages(&game, &detail, &turn, &job.guidance_notes, settings);
    let checks_model = model_for_phase(&game, settings, GameModelPhase::Checks);
    let declared: DeclareChecksResponse = chat_completion_json(
        &inference,
        &checks_model,
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
    let inference = db::get_inference_config(pool).await?;
    if token.is_cancelled() {
        return cancel_turn_job(pool, job).await;
    }

    db::update_turn_phase(pool, turn_id, "resolved").await?;
    let detail = db::get_game_detail(pool, game_id).await?;
    let turn = db::get_turn(pool, game_id, turn_id).await?;
    let game = detail.game.clone();
    let checks = turn.checks.clone();

    let messages = build_resolve_messages(
        &game,
        &detail,
        &turn,
        &checks,
        &job.guidance_notes,
        settings,
    );
    let resolve_model = model_for_phase(&game, settings, GameModelPhase::Resolve);
    let resolved: dreamwell_types::ResolveTurnResponse = chat_completion_json(
        &inference,
        &resolve_model,
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
    let inference = db::get_inference_config(pool).await?;
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

    let prose_model = model_for_phase(&detail.game, settings, GameModelPhase::Prose);
    let mut stream = stream_chat_completion(
        &inference,
        &prose_model,
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
    let situational = clamp_modifier(check.modifier, game.modifier_min, game.modifier_max);
    let total_mod = situational + sheet_mod;
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

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;
    use dreamwell_types::{Game, GameActor};

    fn sample_game() -> Game {
        Game {
            id: 1,
            title: "Test".into(),
            premise: String::new(),
            setting: String::new(),
            gm_style: String::new(),
            opening_message: String::new(),
            character_id: None,
            scenario_id: None,
            resolution_system: dreamwell_types::ResolutionSystem::Pbta2d6,
            modifier_min: -2,
            modifier_max: 3,
            merge_resolve_scene: true,
            step_mode: false,
            model_checks: String::new(),
            model_resolve: String::new(),
            model_prose: String::new(),
            created_at: Utc::now(),
            updated_at: Utc::now(),
            active_job: None,
            queued_jobs: 0,
        }
    }

    fn sample_pc() -> GameActor {
        GameActor {
            id: 1,
            game_id: 1,
            role: "pc".into(),
            name: "Alex".into(),
            description: String::new(),
            skills: [("Finesse".into(), 2)].into_iter().collect(),
            sort_order: 0,
            created_at: Utc::now(),
            updated_at: Utc::now(),
        }
    }

    #[test]
    fn validate_declared_check_clamps_situational_only() {
        let check = DeclaredCheck {
            label: "Pick lock".into(),
            skill: "Finesse".into(),
            modifier: 10,
            stakes: String::new(),
            justification: String::new(),
        };
        let validated = validate_declared_check(&check, &sample_pc(), &sample_game());
        // situational clamped to 3, sheet +2 → total 5
        assert_eq!(validated.modifier, 5);
    }

    #[test]
    fn validate_declared_check_applies_negative_sheet_mod() {
        let mut pc = sample_pc();
        pc.skills.insert("Force".into(), -1);
        let check = DeclaredCheck {
            label: "Break door".into(),
            skill: "Force".into(),
            modifier: -2,
            stakes: String::new(),
            justification: String::new(),
        };
        let validated = validate_declared_check(&check, &pc, &sample_game());
        assert_eq!(validated.modifier, -3);
    }
}
