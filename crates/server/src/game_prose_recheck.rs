use dreamwell_types::{substitute_macros, GameTurnCheck, Job, MacroContext, Settings};
use serde_json::json;
use sqlx::SqlitePool;
use tokio::sync::mpsc;

use crate::config;
use crate::db;
use crate::error::{AppError, AppResult};
use crate::game_prompts::{build_characters_block, scenario_context_block};
use crate::game_resolution::tier_str;
use crate::inference::chat_completion;

const PROSE_RECHECK_OK: &str = "OK";

const RECHECK_SYSTEM_PROMPT: &str = r#"You review game turn prose against scene beats, resolved roll tiers, and the defined scenario.

Given the scene beats, roll outcomes, scenario parameters, and current prose, decide whether the prose fully implements the beats and honors the roll tiers.

Rules:
- Every scene beat must be represented in the prose
- A fail tier cannot be narrated as unqualified success; mixed outcomes need visible cost
- Prose must not introduce major events not listed in the scene beats
- Match GM style and setting/tone — do not inject peril or adventure escalation absent from the beats or scenario
- If the prose already matches, output exactly: OK
- If prose misses beats, contradicts tiers, or adds unplanned plot, output corrected full prose
- Match the established tone and second-person POV
- No headings, labels, or meta commentary — only OK or corrected prose"#;

fn recheck_output_tokens(settings: &Settings) -> i64 {
    if settings.context_tokens > 0 {
        settings.max_tokens.clamp(512, settings.context_tokens / 2)
    } else {
        settings.max_tokens.max(512)
    }
}

fn max_retries() -> u32 {
    config::GENERATION_MAX_RETRIES
        .load(std::sync::atomic::Ordering::SeqCst)
        .max(1)
}

fn format_checks_for_recheck(checks: &[GameTurnCheck]) -> String {
    if checks.is_empty() {
        return "No checks — pure narration.".to_string();
    }
    checks
        .iter()
        .map(|c| {
            let tier = c.tier.map(tier_str).unwrap_or("unknown").to_string();
            format!(
                "- {} ({}+{}): {:?} = {} → {tier} — stakes: {}",
                c.label, c.skill, c.modifier, c.rolls, c.total, c.stakes
            )
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn format_scene_beats(beats: &[String]) -> String {
    if beats.is_empty() {
        return "(none)".to_string();
    }
    beats
        .iter()
        .map(|b| format!("- {b}"))
        .collect::<Vec<_>>()
        .join("\n")
}

fn build_recheck_prompt(
    game: &dreamwell_types::Game,
    actors: &[dreamwell_types::GameActor],
    scene_beats: &str,
    checks_text: &str,
    prose: &str,
    guidance: &str,
    ctx: &MacroContext<'_>,
) -> Vec<serde_json::Value> {
    let mut user = format!(
        "Scenario parameters:\n{}\n\nScene beats:\n{scene_beats}\n\nRoll outcomes:\n{checks_text}\n\nCurrent turn prose:\n{prose}",
        scenario_context_block(game, ctx)
    );
    let characters = build_characters_block(actors);
    if !characters.is_empty() {
        user.push_str(&format!("\n\n{characters}"));
    }
    if !guidance.trim().is_empty() {
        user.push_str("\n\nGuidance from the player:\n");
        user.push_str(guidance.trim());
    }
    vec![
        json!({
            "role": "system",
            "content": RECHECK_SYSTEM_PROMPT,
        }),
        json!({
            "role": "user",
            "content": user,
        }),
    ]
}

pub fn prose_recheck_matches(response: &str) -> bool {
    response.trim().eq_ignore_ascii_case(PROSE_RECHECK_OK)
}

pub async fn enqueue_turn_prose_recheck(
    pool: &SqlitePool,
    work_tx: &mpsc::UnboundedSender<()>,
    game_id: i64,
    turn_id: i64,
    guidance_notes: &str,
    settings: &Settings,
) -> AppResult<Job> {
    if settings.model.is_empty() && db::get_game(pool, game_id).await?.model_prose.is_empty() {
        return Err(AppError::bad_request(
            "Configure an inference model in Settings before aligning prose",
        ));
    }
    let turn = db::get_turn(pool, game_id, turn_id).await?;
    if turn.scene_beats.is_empty() {
        return Err(AppError::bad_request(
            "Resolve the turn (scene beats) before aligning prose",
        ));
    }
    if turn.prose.trim().is_empty() {
        return Err(AppError::bad_request("Turn has no prose to align"));
    }
    if db::has_active_turn_job(pool, turn_id).await? {
        return Err(AppError::bad_request(
            "Wait for the current turn job to finish before aligning prose",
        ));
    }

    let job = db::enqueue_game_job(
        pool,
        dreamwell_types::JobType::GameProseRecheck,
        game_id,
        Some(turn_id),
        guidance_notes.to_string(),
    )
    .await?;
    let _ = work_tx.send(());
    Ok(job)
}

pub async fn run_turn_prose_recheck_job(
    pool: &SqlitePool,
    job_id: i64,
    game_id: i64,
    turn_id: i64,
    guidance: &str,
    settings: &Settings,
) -> AppResult<()> {
    let game = db::get_game(pool, game_id).await?;
    let turn = db::get_turn(pool, game_id, turn_id).await?;
    if turn.prose.trim().is_empty() || turn.scene_beats.is_empty() {
        db::complete_job(pool, job_id, dreamwell_types::JobStatus::Completed, None).await?;
        return Ok(());
    }

    let detail = db::get_game_detail(pool, game_id).await?;
    let ctx = MacroContext::from_game_detail_and_settings(&detail, settings);
    let model =
        crate::game_turn::model_for_phase(&game, settings, crate::game_turn::GameModelPhase::Prose);
    let prompt = build_recheck_prompt(
        &game,
        &detail.actors,
        &format_scene_beats(&turn.scene_beats),
        &format_checks_for_recheck(&turn.checks),
        &substitute_macros(turn.prose.trim(), &ctx),
        guidance,
        &ctx,
    );
    let max_attempts = max_retries();
    let mut raw = None;

    for attempt in 1..=max_attempts {
        match chat_completion(
            &settings.inference_url,
            &model,
            &prompt,
            0.2,
            settings.top_p,
            recheck_output_tokens(settings),
        )
        .await
        {
            Ok(response) => {
                raw = Some(response);
                break;
            }
            Err(err) => {
                let last_error = err.to_string();
                if attempt == max_attempts {
                    return Err(AppError::inference(last_error));
                }
                tracing::warn!(
                    attempt,
                    max_attempts,
                    error = %last_error,
                    "retrying game turn prose recheck"
                );
            }
        }
    }

    let response = raw.unwrap_or_default();
    if prose_recheck_matches(&response) {
        db::complete_job(pool, job_id, dreamwell_types::JobStatus::Completed, None).await?;
        return Ok(());
    }

    let corrected = response.trim();
    if corrected.is_empty() {
        db::complete_job(pool, job_id, dreamwell_types::JobStatus::Completed, None).await?;
        return Ok(());
    }

    db::update_turn_prose(pool, turn_id, corrected).await?;
    db::touch_game(pool, game_id).await?;
    db::complete_job(pool, job_id, dreamwell_types::JobStatus::Completed, None).await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn prose_recheck_matches_ok_response() {
        assert!(prose_recheck_matches("OK"));
        assert!(prose_recheck_matches(" ok "));
        assert!(!prose_recheck_matches("You slip inside quietly."));
    }

    #[test]
    fn recheck_prompt_includes_beats_tiers_prose_and_scenario() {
        let game = dreamwell_types::Game {
            id: 1,
            title: "Tea Shop".into(),
            premise: "Quiet afternoon shift.".into(),
            setting: "Cozy and low-stakes.".into(),
            gm_style: "Gentle pacing.".into(),
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
            created_at: chrono::Utc::now(),
            updated_at: chrono::Utc::now(),
            active_job: None,
            queued_jobs: 0,
        };
        let ctx = MacroContext {
            char_name: "Mira",
            user_name: "Alex",
            persona: "",
            description: "",
            personality: "",
            scenario: "",
            first_message: "",
        };
        let prompt = build_recheck_prompt(
            &game,
            &[dreamwell_types::GameActor {
                id: 1,
                game_id: 1,
                role: "pc".into(),
                name: "Mira".into(),
                description: "Shopkeeper".into(),
                skills: Default::default(),
                sort_order: 0,
                created_at: chrono::Utc::now(),
                updated_at: chrono::Utc::now(),
            }],
            "- The lock clicks.\n",
            "- Pick lock: mixed",
            "You hear the lock turn.",
            "Stay cozy.",
            &ctx,
        );
        let user = prompt[1]["content"].as_str().unwrap();
        assert!(user.contains("Scenario parameters:"));
        assert!(user.contains("Cozy and low-stakes"));
        assert!(user.contains("Scene beats:"));
        assert!(user.contains("Roll outcomes:"));
        assert!(user.contains("You hear the lock turn."));
        assert!(user.contains("Stay cozy."));
        assert!(user.contains("Characters:"));
        assert!(user.contains("Mira (PC)"));
    }
}
