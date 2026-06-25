use dreamwell_types::{Job, Settings};
use serde_json::json;
use sqlx::SqlitePool;
use tokio::sync::mpsc;

use crate::config;
use crate::db;
use crate::error::{AppError, AppResult};
use crate::model_fallback::chat_completion_with_connection_fallback;
use crate::variables::strip_variables_for_display;

const PROSE_RECHECK_OK: &str = "OK";

const RECHECK_SYSTEM_PROMPT: &str = r#"You review beat prose against a mechanical beat plan.

Given the mechanical plan and the current prose, decide whether the prose fully implements the plan without extra plot events.

Rules:
- Every bullet in the mechanical plan must be represented in the prose
- Prose must not introduce major events, characters, or resolutions not listed in the plan
- If the prose already matches, output exactly: OK
- If the prose is missing planned events or adds unplanned plot, output the corrected full prose
- Corrected prose: match the story tone and POV, cover exactly the mechanical plan in order
- Preserve any <var key="...">...</var> tags from the original prose when still accurate
- No headings, labels, or meta commentary — only OK or corrected prose"#;

fn max_retries() -> u32 {
    config::GENERATION_MAX_RETRIES
        .load(std::sync::atomic::Ordering::SeqCst)
        .max(1)
}

fn build_recheck_prompt(mechanical: &str, prose: &str, guidance: &str) -> Vec<serde_json::Value> {
    let mut user = format!("Mechanical beat plan:\n{mechanical}\n\nCurrent beat prose:\n{prose}",);
    if !guidance.trim().is_empty() {
        user.push_str("\n\nGuidance from the author:\n");
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

fn prose_recheck_matches(response: &str) -> bool {
    response.trim().eq_ignore_ascii_case(PROSE_RECHECK_OK)
}

pub async fn enqueue_beat_prose_recheck(
    pool: &SqlitePool,
    work_tx: &mpsc::UnboundedSender<()>,
    story_id: i64,
    chapter_id: i64,
    beat_id: i64,
    guidance_notes: &str,
    settings: &Settings,
) -> AppResult<Job> {
    if settings.model.is_empty() {
        return Err(AppError::bad_request(
            "Configure an inference model in Settings before aligning prose",
        ));
    }
    let beat = db::get_beat(pool, story_id, chapter_id, beat_id).await?;
    if beat.mechanical.trim().is_empty() {
        return Err(AppError::bad_request(
            "Generate a mechanical beat plan before aligning prose",
        ));
    }
    if strip_variables_for_display(&beat.content, false)
        .trim()
        .is_empty()
    {
        return Err(AppError::bad_request("Beat has no prose to align"));
    }
    if db::has_active_beat_job(pool, beat_id).await? {
        return Err(AppError::bad_request(
            "Wait for the current beat job to finish before aligning prose",
        ));
    }

    let job = db::enqueue_beat_prose_recheck_job(
        pool,
        story_id,
        chapter_id,
        beat_id,
        guidance_notes.to_string(),
    )
    .await?;
    let _ = work_tx.send(());
    Ok(job)
}

#[allow(clippy::too_many_arguments)]
pub async fn run_beat_prose_recheck_job(
    pool: &SqlitePool,
    job_id: i64,
    story_id: i64,
    chapter_id: i64,
    beat_id: i64,
    chapter_order: i64,
    guidance: &str,
    settings: &Settings,
) -> AppResult<()> {
    let beat = db::get_beat(pool, story_id, chapter_id, beat_id).await?;
    let visible = strip_variables_for_display(&beat.content, false);
    if visible.trim().is_empty() || beat.mechanical.trim().is_empty() {
        db::complete_job(pool, job_id, dreamwell_types::JobStatus::Completed, None).await?;
        return Ok(());
    }

    let prompt = build_recheck_prompt(&beat.mechanical, &beat.content, guidance);
    let max_attempts = max_retries();
    let mut raw = None;

    for attempt in 1..=max_attempts {
        match chat_completion_with_connection_fallback(
            pool,
            settings,
            &prompt,
            Some(job_id),
            None,
            None,
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
                    "retrying story beat prose recheck"
                );
            }
        }
    }

    let response = raw.unwrap_or_default();
    if prose_recheck_matches(&response) {
        db::complete_job(pool, job_id, dreamwell_types::JobStatus::Completed, None).await?;
        return Ok(());
    }

    let corrected = strip_variables_for_display(&response, false);
    if corrected.trim().is_empty() {
        db::complete_job(pool, job_id, dreamwell_types::JobStatus::Completed, None).await?;
        return Ok(());
    }

    db::finalize_beat_prose(
        pool,
        story_id,
        chapter_order,
        beat_id,
        &corrected,
        &beat.variable_updates,
    )
    .await?;
    db::touch_story(pool, story_id).await?;
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
        assert!(!prose_recheck_matches("She opened the door."));
    }

    #[test]
    fn recheck_prompt_includes_mechanical_and_prose() {
        let prompt =
            build_recheck_prompt("- She enters.\n", "She stepped inside.", "Keep it tense.");
        let user = prompt[1]["content"].as_str().unwrap();
        assert!(user.contains("Mechanical beat plan:"));
        assert!(user.contains("She stepped inside."));
        assert!(user.contains("Keep it tense."));
    }
}
