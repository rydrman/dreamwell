use dreamwell_types::{Settings, StoryChapter};
use serde_json::json;
use sqlx::SqlitePool;

use crate::config;
use crate::db;
use crate::error::{AppError, AppResult};
use crate::inference::chat_completion;
use crate::summarize::process_summarize_output;

const CHAPTER_SUMMARIZE_SYSTEM: &str = r#"Compress written chapter prose into a dense fact summary for downstream story generation.

Rules:
- Short clauses or bullet lines only — no scene narration, dialogue, or literary prose
- Include: key events, character names/state, objects, locations, unresolved threads
- Target ≤150 words
- Output only the summary text"#;

fn summarize_output_tokens(settings: &Settings) -> i64 {
    if settings.context_tokens > 0 {
        (settings.context_tokens / 8).clamp(256, 512)
    } else {
        384
    }
}

fn max_retries() -> u32 {
    config::GENERATION_MAX_RETRIES
        .load(std::sync::atomic::Ordering::SeqCst)
        .max(1)
}

fn build_chapter_transcript(chapter: &StoryChapter) -> String {
    chapter
        .beats
        .iter()
        .filter(|beat| beat.content.trim().len() > 80)
        .map(|beat| {
            format!(
                "Beat {} — {}:\n{}",
                beat.sort_order + 1,
                beat.title,
                beat.content.trim()
            )
        })
        .collect::<Vec<_>>()
        .join("\n\n")
}

pub fn build_chapter_summarize_messages(
    chapter: &StoryChapter,
) -> AppResult<Vec<serde_json::Value>> {
    let transcript = build_chapter_transcript(chapter);
    if transcript.trim().is_empty() {
        return Err(AppError::bad_request(
            "Chapter has no substantial prose to summarize",
        ));
    }
    Ok(vec![
        json!({
            "role": "system",
            "content": CHAPTER_SUMMARIZE_SYSTEM,
        }),
        json!({
            "role": "user",
            "content": format!(
                "Chapter \"{}\" (planning synopsis for context only): {}\n\nWritten prose:\n{transcript}",
                if chapter.title.is_empty() {
                    "Untitled"
                } else {
                    &chapter.title
                },
                if chapter.synopsis.is_empty() {
                    "(none)"
                } else {
                    &chapter.synopsis
                },
            ),
        }),
    ])
}

pub async fn run_story_chapter_summarize_job(
    pool: &SqlitePool,
    job_id: i64,
    story_id: i64,
    chapter_id: i64,
    settings: &Settings,
) -> AppResult<()> {
    let inference = db::get_inference_config(pool).await?;
    let detail = db::get_story_detail(pool, story_id).await?;
    let chapter = detail
        .chapters
        .iter()
        .find(|c| c.id == chapter_id)
        .ok_or_else(|| AppError::internal("chapter not found"))?;

    let messages = build_chapter_summarize_messages(chapter)?;
    let max_attempts = max_retries();
    let mut raw = None;

    for attempt in 1..=max_attempts {
        match chat_completion(
            &inference,
            &settings.model,
            &messages,
            0.2,
            settings.top_p,
            summarize_output_tokens(settings),
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
                    "retrying chapter prose summarize"
                );
            }
        }
    }

    let summary =
        process_summarize_output(&raw.unwrap_or_default()).map_err(AppError::bad_request)?;
    db::set_chapter_prose_summary(pool, chapter_id, &summary).await?;
    db::touch_story(pool, story_id).await?;
    db::complete_job(pool, job_id, dreamwell_types::JobStatus::Completed, None).await?;
    Ok(())
}
