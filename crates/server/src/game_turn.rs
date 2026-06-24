use dreamwell_types::{
    structured_output_tokens, DeclareChecksResponse, DeclaredCheck, GameTurnCheck, Job, JobType,
    Settings,
};
use sqlx::SqlitePool;
use tokio_util::sync::CancellationToken;

use crate::config;
use crate::db;
use crate::error::{AppError, AppResult};
use crate::game_prompts::{build_declare_checks_messages, declare_checks_schema};
use crate::game_resolution::{clamp_modifier, roll_dice};
use crate::game_state::{skill_modifier, validate_skill};

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

pub async fn run_game_job(
    pool: &SqlitePool,
    job_id: i64,
    job: &Job,
    settings: &Settings,
    token: CancellationToken,
) -> AppResult<()> {
    match job.job_type {
        JobType::GameTurnStructuredAgent => {
            let game_id = job
                .game_id
                .ok_or_else(|| AppError::internal("game job missing game_id"))?;
            let turn_id = job
                .turn_id
                .ok_or_else(|| AppError::internal("turn job missing turn_id"))?;
            crate::game_turn_agent::run_tools_structured_phase(
                pool,
                job_id,
                game_id,
                turn_id,
                &job.guidance_notes,
                settings,
                &token,
            )
            .await
        }
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

/// Ask the model which dramatic checks (if any) the action needs based on the PC's
/// skills, then validate, roll, and persist them. Used by the structured inline-prose agent.
pub async fn declare_and_roll_checks(
    pool: &SqlitePool,
    game_id: i64,
    turn_id: i64,
    guidance: &str,
    settings: &Settings,
    token: &CancellationToken,
) -> AppResult<Vec<GameTurnCheck>> {
    let inference = db::get_inference_config(pool).await?;
    let detail = db::get_game_detail(pool, game_id).await?;
    let turn = db::get_turn(pool, game_id, turn_id).await?;
    let game = detail.game.clone();

    let messages = build_declare_checks_messages(&game, &detail, &turn, guidance, settings);
    let checks_model = model_for_phase(&game, settings, GameModelPhase::Checks);
    let declared: DeclareChecksResponse = db::chat_completion_json_for_connection(
        pool,
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
    Ok(rolled_checks)
}

pub fn validate_declared_check(
    check: &DeclaredCheck,
    pc: &dreamwell_types::GameActor,
    game: &dreamwell_types::Game,
) -> DeclaredCheck {
    let allowed_traits: Vec<String> = game.trait_defs.iter().map(|t| t.name.clone()).collect();
    let validated_skill = validate_skill(&check.skill, pc, &allowed_traits);
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
            engine_mode: dreamwell_types::EngineMode::ToolsStructured,
            game_elements: dreamwell_types::GameElementsConfig::default(),
            element_instances: dreamwell_types::ElementInstances::default(),
            model_checks: String::new(),
            model_resolve: String::new(),
            model_prose: String::new(),
            rules_blocks: vec![],
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
