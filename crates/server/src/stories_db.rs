use chrono::{DateTime, Utc};
use dreamwell_types::{
    BeatVariableUpdate, GenerateRequest, Job, JobType, LengthPreset, Story, StoryBeat,
    StoryBeatCreate, StoryBeatUpdate, StoryChapter, StoryChapterCreate, StoryChapterUpdate,
    StoryCreate, StoryDetail, StoryUpdate, StoryVariable, StoryVariableUpdate,
};
use sqlx::SqlitePool;

use crate::db::{get_job, job_type_str, parse_dt, parse_job_status, JobRow};
use crate::error::{AppError, AppResult};

pub async fn list_stories(pool: &SqlitePool) -> AppResult<Vec<Story>> {
    let rows = sqlx::query_as::<_, StoryRow>(
        "SELECT id, title, premise, tone, genre, pov, length_preset, notes, created_at, updated_at FROM stories ORDER BY updated_at DESC",
    )
    .fetch_all(pool)
    .await?;
    let mut stories = Vec::with_capacity(rows.len());
    for row in rows {
        stories.push(story_from_row(pool, row).await?);
    }
    Ok(stories)
}

pub async fn get_story(pool: &SqlitePool, id: i64) -> AppResult<Story> {
    let row = sqlx::query_as::<_, StoryRow>(
        "SELECT id, title, premise, tone, genre, pov, length_preset, notes, created_at, updated_at FROM stories WHERE id = ?1",
    )
    .bind(id)
    .fetch_optional(pool)
    .await?
    .ok_or_else(|| AppError::not_found("Story not found"))?;
    story_from_row(pool, row).await
}

pub async fn get_story_detail(pool: &SqlitePool, id: i64) -> AppResult<StoryDetail> {
    let story = get_story(pool, id).await?;
    let chapters = list_chapters_for_story(pool, id).await?;
    Ok(StoryDetail { story, chapters })
}

async fn story_from_row(pool: &SqlitePool, row: StoryRow) -> AppResult<Story> {
    let active_job = get_active_story_job(pool, row.id).await?;
    let queued_jobs: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM generation_jobs WHERE story_id = ?1 AND status = 'queued'",
    )
    .bind(row.id)
    .fetch_one(pool)
    .await?;
    Ok(Story {
        id: row.id,
        title: row.title,
        premise: row.premise,
        tone: row.tone,
        genre: row.genre,
        pov: row.pov,
        length_preset: parse_length_preset(&row.length_preset),
        notes: row.notes,
        created_at: parse_dt(&row.created_at)?,
        updated_at: parse_dt(&row.updated_at)?,
        active_job,
        queued_jobs,
    })
}

fn parse_length_preset(s: &str) -> LengthPreset {
    match s {
        "flash" => LengthPreset::Flash,
        "novella" => LengthPreset::Novella,
        "novel" => LengthPreset::Novel,
        _ => LengthPreset::Short,
    }
}

fn length_preset_str(preset: LengthPreset) -> &'static str {
    match preset {
        LengthPreset::Flash => "flash",
        LengthPreset::Short => "short",
        LengthPreset::Novella => "novella",
        LengthPreset::Novel => "novel",
    }
}

pub async fn create_story(pool: &SqlitePool, payload: StoryCreate) -> AppResult<Story> {
    let now = Utc::now().to_rfc3339();
    let id = sqlx::query_scalar::<_, i64>(
        "INSERT INTO stories (title, premise, tone, genre, pov, length_preset, notes, created_at, updated_at) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?8) RETURNING id",
    )
    .bind(&payload.title)
    .bind(&payload.premise)
    .bind(&payload.tone)
    .bind(&payload.genre)
    .bind(&payload.pov)
    .bind(length_preset_str(payload.length_preset))
    .bind(&payload.notes)
    .bind(&now)
    .fetch_one(pool)
    .await?;
    get_story(pool, id).await
}

pub async fn update_story(pool: &SqlitePool, id: i64, payload: StoryUpdate) -> AppResult<Story> {
    let existing = get_story(pool, id).await?;
    let updated = Story {
        title: payload.title.unwrap_or(existing.title),
        premise: payload.premise.unwrap_or(existing.premise),
        tone: payload.tone.unwrap_or(existing.tone),
        genre: payload.genre.unwrap_or(existing.genre),
        pov: payload.pov.unwrap_or(existing.pov),
        length_preset: payload.length_preset.unwrap_or(existing.length_preset),
        notes: payload.notes.unwrap_or(existing.notes),
        updated_at: Utc::now(),
        ..existing
    };
    sqlx::query(
        "UPDATE stories SET title=?1, premise=?2, tone=?3, genre=?4, pov=?5, length_preset=?6, notes=?7, updated_at=?8 WHERE id=?9",
    )
    .bind(&updated.title)
    .bind(&updated.premise)
    .bind(&updated.tone)
    .bind(&updated.genre)
    .bind(&updated.pov)
    .bind(length_preset_str(updated.length_preset))
    .bind(&updated.notes)
    .bind(updated.updated_at.to_rfc3339())
    .bind(id)
    .execute(pool)
    .await?;
    get_story(pool, id).await
}

pub async fn delete_story(pool: &SqlitePool, id: i64) -> AppResult<()> {
    let result = sqlx::query("DELETE FROM stories WHERE id = ?1")
        .bind(id)
        .execute(pool)
        .await?;
    if result.rows_affected() == 0 {
        return Err(AppError::not_found("Story not found"));
    }
    Ok(())
}

pub async fn touch_story(pool: &SqlitePool, story_id: i64) -> AppResult<()> {
    let now = Utc::now().to_rfc3339();
    sqlx::query("UPDATE stories SET updated_at = ?1 WHERE id = ?2")
        .bind(&now)
        .bind(story_id)
        .execute(pool)
        .await?;
    Ok(())
}

async fn list_chapters_for_story(pool: &SqlitePool, story_id: i64) -> AppResult<Vec<StoryChapter>> {
    let rows = sqlx::query_as::<_, ChapterRow>(
        "SELECT id, story_id, title, synopsis, prose_summary, prose_summary_valid, prose_summary_at, sort_order, created_at, updated_at FROM story_chapters WHERE story_id = ?1 ORDER BY sort_order ASC, id ASC",
    )
    .bind(story_id)
    .fetch_all(pool)
    .await?;
    let mut chapters = Vec::with_capacity(rows.len());
    for row in rows {
        chapters.push(chapter_from_row(pool, row).await?);
    }
    Ok(chapters)
}

async fn chapter_from_row(pool: &SqlitePool, row: ChapterRow) -> AppResult<StoryChapter> {
    let beats = list_beats_for_chapter(pool, row.id).await?;
    Ok(StoryChapter {
        id: row.id,
        story_id: row.story_id,
        title: row.title,
        synopsis: row.synopsis,
        prose_summary: row.prose_summary,
        prose_summary_valid: row.prose_summary_valid != 0,
        prose_summary_at: row.prose_summary_at.as_deref().map(parse_dt).transpose()?,
        sort_order: row.sort_order,
        created_at: parse_dt(&row.created_at)?,
        updated_at: parse_dt(&row.updated_at)?,
        beats,
    })
}

pub async fn get_chapter(
    pool: &SqlitePool,
    story_id: i64,
    chapter_id: i64,
) -> AppResult<StoryChapter> {
    let row = sqlx::query_as::<_, ChapterRow>(
        "SELECT id, story_id, title, synopsis, prose_summary, prose_summary_valid, prose_summary_at, sort_order, created_at, updated_at FROM story_chapters WHERE id = ?1 AND story_id = ?2",
    )
    .bind(chapter_id)
    .bind(story_id)
    .fetch_optional(pool)
    .await?
    .ok_or_else(|| AppError::not_found("Chapter not found"))?;
    chapter_from_row(pool, row).await
}

pub async fn create_chapter(
    pool: &SqlitePool,
    story_id: i64,
    payload: StoryChapterCreate,
) -> AppResult<StoryChapter> {
    let _ = get_story(pool, story_id).await?;
    let sort_order =
        match payload.sort_order {
            Some(order) => order,
            None => sqlx::query_scalar::<_, i64>(
                "SELECT COALESCE(MAX(sort_order), -1) + 1 FROM story_chapters WHERE story_id = ?1",
            )
            .bind(story_id)
            .fetch_one(pool)
            .await?,
        };
    let now = Utc::now().to_rfc3339();
    let id = sqlx::query_scalar::<_, i64>(
        "INSERT INTO story_chapters (story_id, title, synopsis, sort_order, created_at, updated_at) VALUES (?1,?2,?3,?4,?5,?5) RETURNING id",
    )
    .bind(story_id)
    .bind(&payload.title)
    .bind(&payload.synopsis)
    .bind(sort_order)
    .bind(&now)
    .fetch_one(pool)
    .await?;
    touch_story(pool, story_id).await?;
    get_chapter(pool, story_id, id).await
}

pub async fn update_chapter(
    pool: &SqlitePool,
    story_id: i64,
    chapter_id: i64,
    payload: StoryChapterUpdate,
) -> AppResult<StoryChapter> {
    let existing = get_chapter(pool, story_id, chapter_id).await?;
    let title = payload.title.unwrap_or(existing.title);
    let synopsis = payload.synopsis.unwrap_or(existing.synopsis);
    let sort_order = payload.sort_order.unwrap_or(existing.sort_order);
    let now = Utc::now().to_rfc3339();
    sqlx::query(
        "UPDATE story_chapters SET title=?1, synopsis=?2, sort_order=?3, updated_at=?4 WHERE id=?5",
    )
    .bind(&title)
    .bind(&synopsis)
    .bind(sort_order)
    .bind(&now)
    .bind(chapter_id)
    .execute(pool)
    .await?;
    touch_story(pool, story_id).await?;
    get_chapter(pool, story_id, chapter_id).await
}

pub async fn delete_chapter(pool: &SqlitePool, story_id: i64, chapter_id: i64) -> AppResult<()> {
    let result = sqlx::query("DELETE FROM story_chapters WHERE id = ?1 AND story_id = ?2")
        .bind(chapter_id)
        .bind(story_id)
        .execute(pool)
        .await?;
    if result.rows_affected() == 0 {
        return Err(AppError::not_found("Chapter not found"));
    }
    touch_story(pool, story_id).await?;
    Ok(())
}

async fn list_beats_for_chapter(pool: &SqlitePool, chapter_id: i64) -> AppResult<Vec<StoryBeat>> {
    let rows = sqlx::query_as::<_, BeatRow>(
        "SELECT b.id, b.chapter_id, b.title, b.synopsis, b.mechanical, b.content, b.variable_updates, b.sort_order, b.created_at, b.updated_at, j.status as job_status FROM story_beats b LEFT JOIN generation_jobs j ON j.beat_id = b.id AND j.status IN ('queued','running') WHERE b.chapter_id = ?1 ORDER BY b.sort_order ASC, b.id ASC",
    )
    .bind(chapter_id)
    .fetch_all(pool)
    .await?;
    Ok(rows.into_iter().map(beat_from_row).collect())
}

fn beat_from_row(row: BeatRow) -> StoryBeat {
    StoryBeat {
        id: row.id,
        chapter_id: row.chapter_id,
        title: row.title,
        synopsis: row.synopsis,
        mechanical: row.mechanical,
        content: row.content,
        variable_updates: parse_beat_variable_updates(&row.variable_updates),
        sort_order: row.sort_order,
        created_at: DateTime::parse_from_rfc3339(&row.created_at)
            .map(|dt| dt.with_timezone(&Utc))
            .unwrap_or_else(|_| Utc::now()),
        updated_at: DateTime::parse_from_rfc3339(&row.updated_at)
            .map(|dt| dt.with_timezone(&Utc))
            .unwrap_or_else(|_| Utc::now()),
        job_status: row.job_status.map(|s| parse_job_status(&s)),
    }
}

fn parse_beat_variable_updates(raw: &str) -> Vec<BeatVariableUpdate> {
    serde_json::from_str(raw).unwrap_or_default()
}

pub async fn get_beat(
    pool: &SqlitePool,
    story_id: i64,
    chapter_id: i64,
    beat_id: i64,
) -> AppResult<StoryBeat> {
    let _ = get_chapter(pool, story_id, chapter_id).await?;
    let row = sqlx::query_as::<_, BeatRow>(
        "SELECT b.id, b.chapter_id, b.title, b.synopsis, b.mechanical, b.content, b.variable_updates, b.sort_order, b.created_at, b.updated_at, j.status as job_status FROM story_beats b LEFT JOIN generation_jobs j ON j.beat_id = b.id AND j.status IN ('queued','running') WHERE b.id = ?1 AND b.chapter_id = ?2",
    )
    .bind(beat_id)
    .bind(chapter_id)
    .fetch_optional(pool)
    .await?
    .ok_or_else(|| AppError::not_found("Beat not found"))?;
    Ok(beat_from_row(row))
}

pub async fn create_beat(
    pool: &SqlitePool,
    story_id: i64,
    chapter_id: i64,
    payload: StoryBeatCreate,
) -> AppResult<StoryBeat> {
    let _ = get_chapter(pool, story_id, chapter_id).await?;
    let sort_order =
        match payload.sort_order {
            Some(order) => order,
            None => sqlx::query_scalar::<_, i64>(
                "SELECT COALESCE(MAX(sort_order), -1) + 1 FROM story_beats WHERE chapter_id = ?1",
            )
            .bind(chapter_id)
            .fetch_one(pool)
            .await?,
        };
    let now = Utc::now().to_rfc3339();
    let id = sqlx::query_scalar::<_, i64>(
        "INSERT INTO story_beats (chapter_id, title, synopsis, mechanical, content, sort_order, created_at, updated_at) VALUES (?1,?2,?3,?4,?5,?6,?7,?7) RETURNING id",
    )
    .bind(chapter_id)
    .bind(&payload.title)
    .bind(&payload.synopsis)
    .bind(&payload.mechanical)
    .bind(&payload.content)
    .bind(sort_order)
    .bind(&now)
    .fetch_one(pool)
    .await?;
    touch_story(pool, story_id).await?;
    get_beat(pool, story_id, chapter_id, id).await
}

pub async fn update_beat(
    pool: &SqlitePool,
    story_id: i64,
    chapter_id: i64,
    beat_id: i64,
    payload: StoryBeatUpdate,
) -> AppResult<StoryBeat> {
    let chapter = get_chapter(pool, story_id, chapter_id).await?;
    let existing = get_beat(pool, story_id, chapter_id, beat_id).await?;
    let title = payload.title.unwrap_or(existing.title);
    let synopsis = payload.synopsis.unwrap_or(existing.synopsis);
    let mechanical = payload.mechanical.unwrap_or(existing.mechanical);
    let content_changed = payload
        .content
        .as_ref()
        .is_some_and(|content| content != &existing.content);
    let content = payload.content.unwrap_or(existing.content);
    let sort_order = payload.sort_order.unwrap_or(existing.sort_order);
    let now = Utc::now().to_rfc3339();
    sqlx::query(
        "UPDATE story_beats SET title=?1, synopsis=?2, mechanical=?3, content=?4, sort_order=?5, updated_at=?6 WHERE id=?7",
    )
    .bind(&title)
    .bind(&synopsis)
    .bind(&mechanical)
    .bind(&content)
    .bind(sort_order)
    .bind(&now)
    .bind(beat_id)
    .execute(pool)
    .await?;
    if content_changed {
        invalidate_prose_summaries_from(pool, story_id, chapter.sort_order).await?;
    }
    touch_story(pool, story_id).await?;
    get_beat(pool, story_id, chapter_id, beat_id).await
}

pub async fn delete_beat(
    pool: &SqlitePool,
    story_id: i64,
    chapter_id: i64,
    beat_id: i64,
) -> AppResult<()> {
    let _ = get_chapter(pool, story_id, chapter_id).await?;
    let result = sqlx::query("DELETE FROM story_beats WHERE id = ?1 AND chapter_id = ?2")
        .bind(beat_id)
        .bind(chapter_id)
        .execute(pool)
        .await?;
    if result.rows_affected() == 0 {
        return Err(AppError::not_found("Beat not found"));
    }
    touch_story(pool, story_id).await?;
    Ok(())
}

pub async fn update_chapter_outline(
    pool: &SqlitePool,
    chapter_id: i64,
    title: &str,
    synopsis: &str,
) -> AppResult<()> {
    let now = Utc::now().to_rfc3339();
    sqlx::query("UPDATE story_chapters SET title=?1, synopsis=?2, updated_at=?3 WHERE id=?4")
        .bind(title)
        .bind(synopsis)
        .bind(&now)
        .bind(chapter_id)
        .execute(pool)
        .await?;
    Ok(())
}

pub async fn update_beat_outline(
    pool: &SqlitePool,
    beat_id: i64,
    title: &str,
    synopsis: &str,
) -> AppResult<()> {
    let now = Utc::now().to_rfc3339();
    sqlx::query("UPDATE story_beats SET title=?1, synopsis=?2, updated_at=?3 WHERE id=?4")
        .bind(title)
        .bind(synopsis)
        .bind(&now)
        .bind(beat_id)
        .execute(pool)
        .await?;
    Ok(())
}

pub async fn update_beat_mechanical(
    pool: &SqlitePool,
    beat_id: i64,
    mechanical: &str,
) -> AppResult<()> {
    let now = Utc::now().to_rfc3339();
    sqlx::query("UPDATE story_beats SET mechanical=?1, updated_at=?2 WHERE id=?3")
        .bind(mechanical)
        .bind(&now)
        .bind(beat_id)
        .execute(pool)
        .await?;
    Ok(())
}

pub async fn update_beat_content(pool: &SqlitePool, beat_id: i64, content: &str) -> AppResult<()> {
    let now = Utc::now().to_rfc3339();
    sqlx::query("UPDATE story_beats SET content=?1, updated_at=?2 WHERE id=?3")
        .bind(content)
        .bind(&now)
        .bind(beat_id)
        .execute(pool)
        .await?;
    Ok(())
}

pub fn chapter_has_substantial_prose(chapter: &StoryChapter) -> bool {
    chapter
        .beats
        .iter()
        .any(|beat| beat.content.chars().count() > 80)
}

pub async fn invalidate_prose_summaries_from(
    pool: &SqlitePool,
    story_id: i64,
    from_sort_order: i64,
) -> AppResult<()> {
    sqlx::query(
        "UPDATE story_chapters SET prose_summary_valid = 0 WHERE story_id = ?1 AND sort_order >= ?2",
    )
    .bind(story_id)
    .bind(from_sort_order)
    .execute(pool)
    .await?;
    Ok(())
}

pub async fn ensure_beat_generation_allowed(
    pool: &SqlitePool,
    story_id: i64,
    chapter_id: i64,
) -> AppResult<()> {
    let detail = get_story_detail(pool, story_id).await?;
    let chapter = detail
        .chapters
        .iter()
        .find(|c| c.id == chapter_id)
        .ok_or_else(|| AppError::not_found("Chapter not found"))?;
    for prior in detail
        .chapters
        .iter()
        .filter(|c| c.sort_order < chapter.sort_order)
    {
        if chapter_has_substantial_prose(prior) && !prior.prose_summary_valid {
            return Err(AppError::bad_request(format!(
                "Chapter {} prose summary is stale — summarize it before working on later chapters",
                prior.sort_order + 1
            )));
        }
    }
    Ok(())
}

pub async fn ensure_chapter_beats_allowed(
    pool: &SqlitePool,
    story_id: i64,
    chapter_id: i64,
) -> AppResult<()> {
    let detail = get_story_detail(pool, story_id).await?;
    let chapter = detail
        .chapters
        .iter()
        .find(|c| c.id == chapter_id)
        .ok_or_else(|| AppError::not_found("Chapter not found"))?;
    for prior in detail
        .chapters
        .iter()
        .filter(|c| c.sort_order < chapter.sort_order)
    {
        if chapter_has_substantial_prose(prior) && !prior.prose_summary_valid {
            return Err(AppError::bad_request(format!(
                "Chapter {} prose summary is stale — summarize it before proposing beats here",
                prior.sort_order + 1
            )));
        }
    }
    Ok(())
}

pub async fn set_chapter_prose_summary(
    pool: &SqlitePool,
    chapter_id: i64,
    summary: &str,
) -> AppResult<()> {
    let now = Utc::now().to_rfc3339();
    sqlx::query(
        "UPDATE story_chapters SET prose_summary=?1, prose_summary_valid=1, prose_summary_at=?2, updated_at=?3 WHERE id=?4",
    )
    .bind(summary)
    .bind(&now)
    .bind(&now)
    .bind(chapter_id)
    .execute(pool)
    .await?;
    Ok(())
}

pub async fn prepare_summarize_chapter(
    pool: &SqlitePool,
    story_id: i64,
    chapter_id: i64,
) -> AppResult<Job> {
    let chapter = get_chapter(pool, story_id, chapter_id).await?;
    if !chapter_has_substantial_prose(&chapter) {
        return Err(AppError::bad_request(
            "Chapter has no substantial prose to summarize",
        ));
    }
    if has_active_chapter_summarize_job(pool, chapter_id).await? {
        return Err(AppError::bad_request(
            "A chapter summarize job is already in progress",
        ));
    }
    enqueue_story_job(
        pool,
        JobType::StoryChapterSummarize,
        story_id,
        Some(chapter_id),
        None,
        String::new(),
    )
    .await
}

pub async fn has_active_chapter_summarize_job(
    pool: &SqlitePool,
    chapter_id: i64,
) -> AppResult<bool> {
    let count = sqlx::query_scalar::<_, i64>(
        "SELECT COUNT(*) FROM generation_jobs WHERE chapter_id = ?1 AND job_type = 'story_chapter_summarize' AND status IN ('queued','running')",
    )
    .bind(chapter_id)
    .fetch_one(pool)
    .await?;
    Ok(count > 0)
}

pub async fn has_active_beat_job(pool: &SqlitePool, beat_id: i64) -> AppResult<bool> {
    let count = sqlx::query_scalar::<_, i64>(
        "SELECT COUNT(*) FROM generation_jobs WHERE beat_id = ?1 AND status IN ('queued','running')",
    )
    .bind(beat_id)
    .fetch_one(pool)
    .await?;
    Ok(count > 0)
}

pub async fn enqueue_beat_variable_recheck_job(
    pool: &SqlitePool,
    story_id: i64,
    chapter_id: i64,
    beat_id: i64,
    guidance_notes: String,
) -> AppResult<Job> {
    enqueue_story_job(
        pool,
        JobType::StoryBeatVariableRecheck,
        story_id,
        Some(chapter_id),
        Some(beat_id),
        guidance_notes,
    )
    .await
}

pub async fn enqueue_beat_prose_recheck_job(
    pool: &SqlitePool,
    story_id: i64,
    chapter_id: i64,
    beat_id: i64,
    guidance_notes: String,
) -> AppResult<Job> {
    enqueue_story_job(
        pool,
        JobType::StoryBeatProseRecheck,
        story_id,
        Some(chapter_id),
        Some(beat_id),
        guidance_notes,
    )
    .await
}

pub async fn get_active_story_job(pool: &SqlitePool, story_id: i64) -> AppResult<Option<Job>> {
    let row = sqlx::query_as::<_, JobRow>(
        "SELECT id, job_type, chat_id, message_id, story_id, chapter_id, beat_id, guidance_notes, status, error, position, created_at, started_at, completed_at FROM generation_jobs WHERE story_id = ?1 AND status IN ('queued','running') ORDER BY created_at ASC LIMIT 1",
    )
    .bind(story_id)
    .fetch_optional(pool)
    .await?;
    Ok(row.map(Into::into))
}

pub async fn enqueue_story_job(
    pool: &SqlitePool,
    job_type: JobType,
    story_id: i64,
    chapter_id: Option<i64>,
    beat_id: Option<i64>,
    guidance_notes: String,
) -> AppResult<Job> {
    let now = Utc::now().to_rfc3339();
    let position: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM generation_jobs WHERE status = 'queued'")
            .fetch_one(pool)
            .await?;
    let id = sqlx::query_scalar::<_, i64>(
        "INSERT INTO generation_jobs (job_type, story_id, chapter_id, beat_id, guidance_notes, status, position, created_at) VALUES (?1,?2,?3,?4,?5,'queued',?6,?7) RETURNING id",
    )
    .bind(job_type_str(job_type))
    .bind(story_id)
    .bind(chapter_id)
    .bind(beat_id)
    .bind(&guidance_notes)
    .bind(position + 1)
    .bind(&now)
    .fetch_one(pool)
    .await?;
    get_job(pool, id).await
}

pub async fn prepare_propose_chapters(
    pool: &SqlitePool,
    story_id: i64,
    payload: &GenerateRequest,
) -> AppResult<Job> {
    let _ = get_story(pool, story_id).await?;
    enqueue_story_job(
        pool,
        JobType::StoryProposeChapters,
        story_id,
        None,
        None,
        payload.guidance_notes.clone(),
    )
    .await
}

pub async fn apply_chapter_proposal(
    pool: &SqlitePool,
    story_id: i64,
    chapters: &[(String, String)],
) -> AppResult<()> {
    let _ = get_story(pool, story_id).await?;
    sqlx::query("DELETE FROM story_chapters WHERE story_id = ?1")
        .bind(story_id)
        .execute(pool)
        .await?;
    for (sort_order, (title, synopsis)) in chapters.iter().enumerate() {
        create_chapter(
            pool,
            story_id,
            StoryChapterCreate {
                title: title.clone(),
                synopsis: synopsis.clone(),
                sort_order: Some(sort_order as i64),
            },
        )
        .await?;
    }
    touch_story(pool, story_id).await?;
    Ok(())
}

pub async fn prepare_propose_beats(
    pool: &SqlitePool,
    story_id: i64,
    chapter_id: i64,
    payload: &GenerateRequest,
) -> AppResult<Job> {
    ensure_chapter_beats_allowed(pool, story_id, chapter_id).await?;
    let _ = get_chapter(pool, story_id, chapter_id).await?;
    enqueue_story_job(
        pool,
        JobType::StoryProposeBeats,
        story_id,
        Some(chapter_id),
        None,
        payload.guidance_notes.clone(),
    )
    .await
}

pub async fn apply_beat_proposal(
    pool: &SqlitePool,
    story_id: i64,
    chapter_id: i64,
    beats: &[(String, String)],
) -> AppResult<()> {
    let _ = get_chapter(pool, story_id, chapter_id).await?;
    sqlx::query("DELETE FROM story_beats WHERE chapter_id = ?1")
        .bind(chapter_id)
        .execute(pool)
        .await?;
    for (sort_order, (title, synopsis)) in beats.iter().enumerate() {
        create_beat(
            pool,
            story_id,
            chapter_id,
            StoryBeatCreate {
                title: title.clone(),
                synopsis: synopsis.clone(),
                mechanical: String::new(),
                content: String::new(),
                sort_order: Some(sort_order as i64),
            },
        )
        .await?;
    }
    touch_story(pool, story_id).await?;
    Ok(())
}

pub async fn prepare_generate_chapter(
    pool: &SqlitePool,
    story_id: i64,
    payload: &GenerateRequest,
) -> AppResult<(StoryChapter, Job)> {
    let chapter = create_chapter(
        pool,
        story_id,
        StoryChapterCreate {
            title: String::new(),
            synopsis: String::new(),
            sort_order: None,
        },
    )
    .await?;
    let job = enqueue_story_job(
        pool,
        JobType::StoryChapterOutline,
        story_id,
        Some(chapter.id),
        None,
        payload.guidance_notes.clone(),
    )
    .await?;
    Ok((chapter, job))
}

pub async fn prepare_generate_beat(
    pool: &SqlitePool,
    story_id: i64,
    chapter_id: i64,
    payload: &GenerateRequest,
) -> AppResult<(StoryBeat, Job)> {
    ensure_chapter_beats_allowed(pool, story_id, chapter_id).await?;
    let beat = create_beat(
        pool,
        story_id,
        chapter_id,
        StoryBeatCreate {
            title: String::new(),
            synopsis: String::new(),
            mechanical: String::new(),
            content: String::new(),
            sort_order: None,
        },
    )
    .await?;
    let job = enqueue_story_job(
        pool,
        JobType::StoryBeatOutline,
        story_id,
        Some(chapter_id),
        Some(beat.id),
        payload.guidance_notes.clone(),
    )
    .await?;
    Ok((beat, job))
}

pub async fn prepare_generate_mechanical(
    pool: &SqlitePool,
    story_id: i64,
    chapter_id: i64,
    beat_id: i64,
    payload: &GenerateRequest,
) -> AppResult<Job> {
    let beat = get_beat(pool, story_id, chapter_id, beat_id).await?;
    if beat.synopsis.trim().is_empty() {
        return Err(AppError::bad_request(
            "Add a beat synopsis before generating a mechanical plan",
        ));
    }
    if has_active_beat_job(pool, beat_id).await? {
        return Err(AppError::bad_request(
            "Wait for the current beat job to finish before generating a mechanical plan",
        ));
    }
    enqueue_story_job(
        pool,
        JobType::StoryBeatMechanical,
        story_id,
        Some(chapter_id),
        Some(beat_id),
        payload.guidance_notes.clone(),
    )
    .await
}

pub async fn prepare_generate_prose(
    pool: &SqlitePool,
    story_id: i64,
    chapter_id: i64,
    beat_id: i64,
    payload: &GenerateRequest,
) -> AppResult<Job> {
    ensure_beat_generation_allowed(pool, story_id, chapter_id).await?;
    let beat = get_beat(pool, story_id, chapter_id, beat_id).await?;
    let chapter = get_chapter(pool, story_id, chapter_id).await?;
    if beat.mechanical.trim().is_empty() {
        return Err(AppError::bad_request(
            "Generate a mechanical beat plan before generating prose",
        ));
    }
    if has_active_beat_job(pool, beat_id).await? {
        return Err(AppError::bad_request(
            "Wait for the current beat job to finish before generating prose",
        ));
    }
    if !beat.variable_updates.is_empty() {
        revert_beat_variable_updates(
            pool,
            story_id,
            chapter.sort_order,
            beat.sort_order,
            &beat.variable_updates,
        )
        .await?;
        clear_beat_variable_updates(pool, beat_id).await?;
    }
    update_beat_content(pool, beat_id, "").await?;
    enqueue_story_job(
        pool,
        JobType::StoryBeatProse,
        story_id,
        Some(chapter_id),
        Some(beat_id),
        payload.guidance_notes.clone(),
    )
    .await
}

pub async fn list_story_variables(
    pool: &SqlitePool,
    story_id: i64,
) -> AppResult<Vec<StoryVariable>> {
    let rows = sqlx::query_as::<_, StoryVariableRow>(
        "SELECT id, story_id, key, value, source_chapter_order, source_beat_order, updated_at FROM story_variables WHERE story_id = ?1 ORDER BY source_chapter_order ASC, source_beat_order ASC, key ASC",
    )
    .bind(story_id)
    .fetch_all(pool)
    .await?;
    Ok(rows.into_iter().map(Into::into).collect())
}

pub async fn upsert_story_variable(
    pool: &SqlitePool,
    story_id: i64,
    key: String,
    value: String,
    source_chapter_order: i64,
    source_beat_order: i64,
) -> AppResult<StoryVariable> {
    let now = Utc::now().to_rfc3339();
    sqlx::query(
        "INSERT INTO story_variables (story_id, key, value, source_chapter_order, source_beat_order, updated_at) VALUES (?1,?2,?3,?4,?5,?6) ON CONFLICT(story_id, key, source_chapter_order, source_beat_order) DO UPDATE SET value=excluded.value, updated_at=excluded.updated_at",
    )
    .bind(story_id)
    .bind(&key)
    .bind(&value)
    .bind(source_chapter_order)
    .bind(source_beat_order)
    .bind(&now)
    .execute(pool)
    .await?;
    let row = sqlx::query_as::<_, StoryVariableRow>(
        "SELECT id, story_id, key, value, source_chapter_order, source_beat_order, updated_at FROM story_variables WHERE story_id = ?1 AND key = ?2 AND source_chapter_order = ?3 AND source_beat_order = ?4",
    )
    .bind(story_id)
    .bind(&key)
    .bind(source_chapter_order)
    .bind(source_beat_order)
    .fetch_one(pool)
    .await?;
    Ok(row.into())
}

pub async fn upsert_story_variable_manual(
    pool: &SqlitePool,
    story_id: i64,
    payload: StoryVariableUpdate,
) -> AppResult<StoryVariable> {
    let chapter_order = payload
        .source_chapter_order
        .unwrap_or(crate::story_variables::MANUAL_VARIABLE_SOURCE);
    let beat_order = payload.source_beat_order.unwrap_or(
        if chapter_order == crate::story_variables::MANUAL_VARIABLE_SOURCE {
            crate::story_variables::MANUAL_VARIABLE_SOURCE
        } else {
            0
        },
    );
    upsert_story_variable(
        pool,
        story_id,
        payload.key,
        payload.value,
        chapter_order,
        beat_order,
    )
    .await
}

pub async fn delete_story_variable(
    pool: &SqlitePool,
    story_id: i64,
    variable_id: i64,
) -> AppResult<()> {
    let result = sqlx::query("DELETE FROM story_variables WHERE story_id = ?1 AND id = ?2")
        .bind(story_id)
        .bind(variable_id)
        .execute(pool)
        .await?;
    if result.rows_affected() == 0 {
        return Err(AppError::not_found("Variable not found"));
    }
    Ok(())
}

pub async fn get_story_variable(
    pool: &SqlitePool,
    story_id: i64,
    variable_id: i64,
) -> AppResult<StoryVariable> {
    let row = sqlx::query_as::<_, StoryVariableRow>(
        "SELECT id, story_id, key, value, source_chapter_order, source_beat_order, updated_at FROM story_variables WHERE story_id = ?1 AND id = ?2",
    )
    .bind(story_id)
    .bind(variable_id)
    .fetch_optional(pool)
    .await?
    .ok_or_else(|| AppError::not_found("Variable not found"))?;
    Ok(row.into())
}

pub async fn delete_story_variable_scoped(
    pool: &SqlitePool,
    story_id: i64,
    key: &str,
    source_chapter_order: i64,
    source_beat_order: i64,
) -> AppResult<()> {
    let _ = sqlx::query(
        "DELETE FROM story_variables WHERE story_id = ?1 AND key = ?2 AND source_chapter_order = ?3 AND source_beat_order = ?4",
    )
    .bind(story_id)
    .bind(key)
    .bind(source_chapter_order)
    .bind(source_beat_order)
    .execute(pool)
    .await?;
    Ok(())
}

pub async fn apply_story_variable_updates(
    pool: &SqlitePool,
    story_id: i64,
    chapter_order: i64,
    beat_order: i64,
    updates: &[crate::variables::VariableUpdate],
) -> AppResult<()> {
    for update in updates {
        match update {
            crate::variables::VariableUpdate::Set { key, value } => {
                upsert_story_variable(
                    pool,
                    story_id,
                    key.clone(),
                    value.clone(),
                    chapter_order,
                    beat_order,
                )
                .await?;
            }
            crate::variables::VariableUpdate::Delete { key } => {
                let _ =
                    delete_story_variable_scoped(pool, story_id, key, chapter_order, beat_order)
                        .await;
            }
        }
    }
    Ok(())
}

pub async fn build_beat_variable_updates(
    pool: &SqlitePool,
    chapters: &[StoryChapter],
    story_id: i64,
    chapter_order: i64,
    beat_order: i64,
    updates: &[crate::variables::VariableUpdate],
) -> AppResult<Vec<BeatVariableUpdate>> {
    let panel = list_story_variables(pool, story_id).await?;
    let state = crate::variable_state::story_state_at(chapters, &panel, chapter_order, beat_order);
    let mut result = Vec::with_capacity(updates.len());
    for update in updates {
        match update {
            crate::variables::VariableUpdate::Set { key, value } => {
                let previous_value = state.get(key).cloned();
                result.push(BeatVariableUpdate {
                    key: key.clone(),
                    value: value.clone(),
                    previous_value,
                    deleted: false,
                });
            }
            crate::variables::VariableUpdate::Delete { key } => {
                let previous_value = state.get(key).cloned();
                result.push(BeatVariableUpdate {
                    key: key.clone(),
                    value: String::new(),
                    previous_value,
                    deleted: true,
                });
            }
        }
    }
    Ok(result)
}

pub async fn revert_beat_variable_updates(
    pool: &SqlitePool,
    story_id: i64,
    chapter_order: i64,
    beat_order: i64,
    updates: &[BeatVariableUpdate],
) -> AppResult<()> {
    for update in updates.iter().rev() {
        if update.deleted {
            if let Some(previous) = &update.previous_value {
                upsert_story_variable(
                    pool,
                    story_id,
                    update.key.clone(),
                    previous.clone(),
                    chapter_order,
                    beat_order,
                )
                .await?;
            }
        } else {
            let _ = delete_story_variable_scoped(
                pool,
                story_id,
                &update.key,
                chapter_order,
                beat_order,
            )
            .await;
        }
    }
    Ok(())
}

pub async fn finalize_beat_prose(
    pool: &SqlitePool,
    story_id: i64,
    chapter_order: i64,
    beat_id: i64,
    content: &str,
    variable_updates: &[BeatVariableUpdate],
) -> AppResult<()> {
    let now = Utc::now().to_rfc3339();
    let updates_json = serde_json::to_string(variable_updates).unwrap_or_else(|_| "[]".to_string());
    sqlx::query(
        "UPDATE story_beats SET content=?1, variable_updates=?2, updated_at=?3 WHERE id=?4",
    )
    .bind(content)
    .bind(updates_json)
    .bind(&now)
    .bind(beat_id)
    .execute(pool)
    .await?;
    invalidate_prose_summaries_from(pool, story_id, chapter_order).await?;
    Ok(())
}

async fn clear_beat_variable_updates(pool: &SqlitePool, beat_id: i64) -> AppResult<()> {
    let now = Utc::now().to_rfc3339();
    sqlx::query("UPDATE story_beats SET variable_updates='[]', updated_at=?1 WHERE id=?2")
        .bind(&now)
        .bind(beat_id)
        .execute(pool)
        .await?;
    Ok(())
}

#[derive(sqlx::FromRow)]
struct StoryRow {
    id: i64,
    title: String,
    premise: String,
    tone: String,
    genre: String,
    pov: String,
    length_preset: String,
    notes: String,
    created_at: String,
    updated_at: String,
}

#[derive(sqlx::FromRow)]
struct ChapterRow {
    id: i64,
    story_id: i64,
    title: String,
    synopsis: String,
    prose_summary: String,
    prose_summary_valid: i64,
    prose_summary_at: Option<String>,
    sort_order: i64,
    created_at: String,
    updated_at: String,
}

#[derive(sqlx::FromRow)]
struct BeatRow {
    id: i64,
    chapter_id: i64,
    title: String,
    synopsis: String,
    mechanical: String,
    content: String,
    variable_updates: String,
    sort_order: i64,
    created_at: String,
    updated_at: String,
    job_status: Option<String>,
}

#[derive(sqlx::FromRow)]
struct StoryVariableRow {
    id: i64,
    story_id: i64,
    key: String,
    value: String,
    source_chapter_order: i64,
    source_beat_order: i64,
    updated_at: String,
}

impl From<StoryVariableRow> for StoryVariable {
    fn from(row: StoryVariableRow) -> Self {
        Self {
            id: row.id,
            story_id: row.story_id,
            key: row.key,
            value: row.value,
            source_chapter_order: row.source_chapter_order,
            source_beat_order: row.source_beat_order,
            updated_at: DateTime::parse_from_rfc3339(&row.updated_at)
                .map(|dt| dt.with_timezone(&Utc))
                .unwrap_or_else(|_| Utc::now()),
        }
    }
}
