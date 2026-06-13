use dreamwell_types::{Job, Settings};
use serde_json::json;
use sqlx::SqlitePool;
use tokio::sync::mpsc;

use crate::config;
use crate::db;
use crate::error::{AppError, AppResult};
use crate::inference::chat_completion;
use crate::story_variables::filter_meaningful_story_updates;
use crate::variables::{
    merge_variable_tags_into_message, parse_variable_updates, strip_variables_for_display,
};

const RECHECK_SYSTEM_PROMPT: &str = r#"You review story beat prose for story variable state.

Given the prose and current story variables, output ONLY <var> XML tags to correct, add, or remove variables that should persist (character names, objects, locations, relationships, etc.).

Rules:
- Use <var key="name">value</var> to set or replace a value
- Use <var key="name" delete/> to remove a variable
- Fix values that contradict the prose
- Add variables for facts in prose but missing tags
- Do not repeat tags for values already correct
- Output nothing if no corrections are needed
- Only var tags — no prose or explanations"#;

fn recheck_output_tokens(settings: &Settings) -> i64 {
    if settings.context_tokens > 0 {
        (settings.context_tokens / 16).clamp(128, 512)
    } else {
        256
    }
}

fn max_retries() -> u32 {
    config::GENERATION_MAX_RETRIES
        .load(std::sync::atomic::Ordering::SeqCst)
        .max(1)
}

fn format_current_variables(variables: &[dreamwell_types::StoryVariable]) -> String {
    if variables.is_empty() {
        "(none)".to_string()
    } else {
        variables
            .iter()
            .map(|v| format!("- {}: {}", v.key, v.value))
            .collect::<Vec<_>>()
            .join("\n")
    }
}

fn build_recheck_prompt(
    prose: &str,
    variables: &[dreamwell_types::StoryVariable],
    guidance: &str,
) -> Vec<serde_json::Value> {
    let mut user = format!(
        "Current story variables:\n{}\n\nBeat prose to review:\n{prose}",
        format_current_variables(variables),
    );
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

pub async fn enqueue_beat_variable_recheck(
    pool: &SqlitePool,
    work_tx: &mpsc::UnboundedSender<()>,
    story_id: i64,
    chapter_id: i64,
    beat_id: i64,
    guidance_notes: &str,
    settings: &Settings,
) -> AppResult<Job> {
    if !settings.variables_enabled {
        return Err(AppError::bad_request(
            "Story variables are disabled in settings",
        ));
    }
    if settings.model.is_empty() {
        return Err(AppError::bad_request(
            "Configure an inference model in Settings before rechecking variables",
        ));
    }
    let beat = db::get_beat(pool, story_id, chapter_id, beat_id).await?;
    if strip_variables_for_display(&beat.content, false)
        .trim()
        .is_empty()
    {
        return Err(AppError::bad_request("Beat has no prose to recheck"));
    }
    if db::has_active_beat_job(pool, beat_id).await? {
        return Err(AppError::bad_request(
            "Wait for the current beat job to finish before rechecking variables",
        ));
    }

    let job = db::enqueue_beat_variable_recheck_job(
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
pub async fn run_beat_variable_recheck_job(
    pool: &SqlitePool,
    job_id: i64,
    story_id: i64,
    chapter_id: i64,
    beat_id: i64,
    chapter_order: i64,
    beat_order: i64,
    guidance: &str,
    settings: &Settings,
) -> AppResult<()> {
    if !settings.variables_enabled {
        db::complete_job(pool, job_id, dreamwell_types::JobStatus::Completed, None).await?;
        return Ok(());
    }

    let beat = db::get_beat(pool, story_id, chapter_id, beat_id).await?;
    let visible = strip_variables_for_display(&beat.content, false);
    if visible.trim().is_empty() {
        db::complete_job(pool, job_id, dreamwell_types::JobStatus::Completed, None).await?;
        return Ok(());
    }

    let variables = db::list_story_variables(pool, story_id).await?;
    let prompt = build_recheck_prompt(&visible, &variables, guidance);
    let max_attempts = max_retries();
    let mut raw = None;

    for attempt in 1..=max_attempts {
        match chat_completion(
            &settings.inference_url,
            &settings.model,
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
                    "retrying story beat variable recheck"
                );
            }
        }
    }

    let parsed = parse_variable_updates(&raw.unwrap_or_default());
    if parsed.is_empty() {
        db::complete_job(pool, job_id, dreamwell_types::JobStatus::Completed, None).await?;
        return Ok(());
    }

    let meaningful = filter_meaningful_story_updates(&parsed, &variables);
    if meaningful.is_empty() {
        db::complete_job(pool, job_id, dreamwell_types::JobStatus::Completed, None).await?;
        return Ok(());
    }

    let beat_updates = db::build_beat_variable_updates(pool, story_id, &meaningful).await?;
    db::apply_story_variable_updates(pool, story_id, chapter_order, beat_order, &meaningful)
        .await?;

    let merged = merge_variable_tags_into_message(&beat.content, &meaningful);
    let display = strip_variables_for_display(&merged, false);
    let mut combined_updates = beat.variable_updates.clone();
    combined_updates.extend(beat_updates);
    db::finalize_beat_prose(
        pool,
        story_id,
        chapter_order,
        beat_id,
        &display,
        &combined_updates,
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
    fn recheck_prompt_includes_guidance_when_provided() {
        let prompt = build_recheck_prompt("She opened the door.", &[], "Track the key as an item.");
        let user = prompt[1]["content"].as_str().unwrap();
        assert!(user.contains("She opened the door."));
        assert!(user.contains("Guidance from the author:"));
        assert!(user.contains("Track the key as an item."));
    }

    #[test]
    fn recheck_prompt_omits_guidance_when_empty() {
        let prompt = build_recheck_prompt("She opened the door.", &[], "  ");
        let user = prompt[1]["content"].as_str().unwrap();
        assert!(!user.contains("Guidance from the author:"));
    }
}
