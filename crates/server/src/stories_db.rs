use chrono::{DateTime, Utc};
use dreamwell_types::{
    GenerateRequest, Job, JobType, LengthPreset, Story, StoryBeat, StoryBeatCreate,
    StoryBeatUpdate, StoryChapter, StoryChapterCreate, StoryChapterUpdate, StoryCreate,
    StoryDetail, StoryUpdate,
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
        "SELECT id, story_id, title, synopsis, sort_order, created_at, updated_at FROM story_chapters WHERE story_id = ?1 ORDER BY sort_order ASC, id ASC",
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
        "SELECT id, story_id, title, synopsis, sort_order, created_at, updated_at FROM story_chapters WHERE id = ?1 AND story_id = ?2",
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
        "SELECT b.id, b.chapter_id, b.title, b.synopsis, b.content, b.sort_order, b.created_at, b.updated_at, j.status as job_status FROM story_beats b LEFT JOIN generation_jobs j ON j.beat_id = b.id AND j.status IN ('queued','running') WHERE b.chapter_id = ?1 ORDER BY b.sort_order ASC, b.id ASC",
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
        content: row.content,
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

pub async fn get_beat(
    pool: &SqlitePool,
    story_id: i64,
    chapter_id: i64,
    beat_id: i64,
) -> AppResult<StoryBeat> {
    let _ = get_chapter(pool, story_id, chapter_id).await?;
    let row = sqlx::query_as::<_, BeatRow>(
        "SELECT b.id, b.chapter_id, b.title, b.synopsis, b.content, b.sort_order, b.created_at, b.updated_at, j.status as job_status FROM story_beats b LEFT JOIN generation_jobs j ON j.beat_id = b.id AND j.status IN ('queued','running') WHERE b.id = ?1 AND b.chapter_id = ?2",
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
        "INSERT INTO story_beats (chapter_id, title, synopsis, content, sort_order, created_at, updated_at) VALUES (?1,?2,?3,?4,?5,?6,?6) RETURNING id",
    )
    .bind(chapter_id)
    .bind(&payload.title)
    .bind(&payload.synopsis)
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
    let existing = get_beat(pool, story_id, chapter_id, beat_id).await?;
    let title = payload.title.unwrap_or(existing.title);
    let synopsis = payload.synopsis.unwrap_or(existing.synopsis);
    let content = payload.content.unwrap_or(existing.content);
    let sort_order = payload.sort_order.unwrap_or(existing.sort_order);
    let now = Utc::now().to_rfc3339();
    sqlx::query(
        "UPDATE story_beats SET title=?1, synopsis=?2, content=?3, sort_order=?4, updated_at=?5 WHERE id=?6",
    )
    .bind(&title)
    .bind(&synopsis)
    .bind(&content)
    .bind(sort_order)
    .bind(&now)
    .bind(beat_id)
    .execute(pool)
    .await?;
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

pub async fn prepare_generate_full_outline(
    pool: &SqlitePool,
    story_id: i64,
    payload: &GenerateRequest,
) -> AppResult<Job> {
    let detail = get_story_detail(pool, story_id).await?;
    let target = detail.story.length_preset.ref_chapters();
    let current = detail.chapters.len() as i64;
    for _ in current..target {
        create_chapter(
            pool,
            story_id,
            StoryChapterCreate {
                title: String::new(),
                synopsis: String::new(),
                sort_order: None,
            },
        )
        .await?;
    }
    let detail = get_story_detail(pool, story_id).await?;
    let needs_outline = detail
        .chapters
        .iter()
        .any(|c| c.title.is_empty() && c.synopsis.is_empty());
    if !needs_outline {
        return Err(AppError::bad_request(
            "All chapters already have outlines. Edit or delete chapters to regenerate.",
        ));
    }
    enqueue_story_job(
        pool,
        JobType::StoryFullOutline,
        story_id,
        None,
        None,
        payload.guidance_notes.clone(),
    )
    .await
}

pub async fn prepare_queue_remaining_chapters(
    pool: &SqlitePool,
    story_id: i64,
    payload: &GenerateRequest,
) -> AppResult<Vec<Job>> {
    let detail = get_story_detail(pool, story_id).await?;
    let target = detail.story.length_preset.ref_chapters();
    let current = detail.chapters.len() as i64;
    let mut jobs = Vec::new();
    for _ in current..target {
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
        jobs.push(job);
    }
    if jobs.is_empty() {
        let empty: Vec<_> = detail
            .chapters
            .iter()
            .filter(|c| c.title.is_empty() && c.synopsis.is_empty())
            .collect();
        for chapter in empty {
            let job = enqueue_story_job(
                pool,
                JobType::StoryChapterOutline,
                story_id,
                Some(chapter.id),
                None,
                payload.guidance_notes.clone(),
            )
            .await?;
            jobs.push(job);
        }
    }
    if jobs.is_empty() {
        return Err(AppError::bad_request("All chapters already have outlines."));
    }
    Ok(jobs)
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
    let beat = create_beat(
        pool,
        story_id,
        chapter_id,
        StoryBeatCreate {
            title: String::new(),
            synopsis: String::new(),
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

pub async fn prepare_generate_prose(
    pool: &SqlitePool,
    story_id: i64,
    chapter_id: i64,
    beat_id: i64,
    payload: &GenerateRequest,
) -> AppResult<Job> {
    let _ = get_beat(pool, story_id, chapter_id, beat_id).await?;
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
    content: String,
    sort_order: i64,
    created_at: String,
    updated_at: String,
    job_status: Option<String>,
}
