use chrono::{DateTime, Utc};
use dreamwell_types::{
    AppliedStateChange, Character, CharacterCreate, CharacterUpdate, Chat, ChatActor, ChatDetail,
    ChatStateEntry, ChatStateEntryUpdate, ChatUpdate, ChatVariable, ChatVariableUpdate,
    InferenceConnection, InferenceConnectionCreate, InferenceConnectionUpdate, Job, JobStatus,
    JobType, JsonFormatStrategy, Message, MessageRole, Settings, SettingsUpdate, StateKind,
    CHAT_ARCHIVE_RETENTION_DAYS, DEFAULT_SYSTEM_PROMPT_PREFIX, DEFAULT_USER_NAME,
};
use std::time::Duration;

#[path = "game_db.rs"]
mod game_db;
#[path = "stories_db.rs"]
mod stories_db;
pub use game_db::*;
use sqlx::sqlite::{SqliteConnectOptions, SqlitePoolOptions};
use sqlx::{ConnectOptions, Connection, SqlitePool};
pub use stories_db::*;

use crate::config::MAX_CONCURRENT_JOBS;
use crate::error::{AppError, AppResult};
use crate::inference::{chat_completion_json, InferenceConfig};
use tokio_util::sync::CancellationToken;

pub async fn connect(database_url: &str) -> AppResult<SqlitePool> {
    let url = database_url.strip_prefix("sqlite:").unwrap_or(database_url);
    let options = SqliteConnectOptions::new()
        .filename(url)
        .create_if_missing(true)
        .journal_mode(sqlx::sqlite::SqliteJournalMode::Wal)
        .synchronous(sqlx::sqlite::SqliteSynchronous::Normal)
        .busy_timeout(Duration::from_secs(30));

    // Run migrations and WAL maintenance on a single connection before opening the pool.
    // TRUNCATE checkpoint needs exclusive access; doing it on a live pool invalidates other
    // connection snapshots and surfaces as SQLITE_BUSY_SNAPSHOT (517) under concurrent load.
    let mut setup = options.connect().await?;
    sqlx::migrate!("./migrations").run(&mut setup).await?;
    ensure_settings_conn(&mut setup).await?;
    prepare_on_startup_conn(&mut setup).await?;
    setup.close().await?;

    let pool = SqlitePoolOptions::new()
        .max_connections(1)
        .acquire_timeout(Duration::from_secs(30))
        .connect_with(options)
        .await?;
    Ok(pool)
}

/// Release stale WAL locks and normalize journal state after an unclean shutdown.
async fn prepare_on_startup_conn(setup: &mut sqlx::sqlite::SqliteConnection) -> AppResult<()> {
    sqlx::query("PRAGMA wal_checkpoint(PASSIVE)")
        .execute(&mut *setup)
        .await?;
    purge_expired_archived_chats_conn(setup).await?;
    Ok(())
}

fn is_sqlite_locked(err: &sqlx::Error) -> bool {
    match err {
        sqlx::Error::Database(e) => {
            e.code()
                .is_some_and(|c| c == "5" || c == "517" || c == "261")
                || e.message().contains("database is locked")
        }
        _ => false,
    }
}

async fn ensure_settings_conn(setup: &mut sqlx::sqlite::SqliteConnection) -> AppResult<()> {
    sqlx::query(
        "INSERT OR IGNORE INTO app_settings (id, system_prompt_prefix, user_name) VALUES (1, ?1, ?2)",
    )
    .bind(DEFAULT_SYSTEM_PROMPT_PREFIX)
    .bind(DEFAULT_USER_NAME)
    .execute(&mut *setup)
    .await?;
    Ok(())
}

#[cfg(test)]
pub async fn ensure_settings(pool: &SqlitePool) -> AppResult<()> {
    let mut conn = pool.acquire().await?;
    ensure_settings_conn(&mut conn).await
}

async fn purge_expired_archived_chats_conn(
    conn: &mut sqlx::sqlite::SqliteConnection,
) -> AppResult<u64> {
    let cutoff = (Utc::now() - chrono::Duration::days(CHAT_ARCHIVE_RETENTION_DAYS)).to_rfc3339();
    let result =
        sqlx::query("DELETE FROM chats WHERE archived_at IS NOT NULL AND archived_at < ?1")
            .bind(&cutoff)
            .execute(&mut *conn)
            .await?;
    Ok(result.rows_affected())
}

fn parse_role(s: &str) -> MessageRole {
    match s {
        "user" => MessageRole::User,
        "assistant" => MessageRole::Assistant,
        _ => MessageRole::System,
    }
}

fn role_str(role: MessageRole) -> &'static str {
    match role {
        MessageRole::User => "user",
        MessageRole::Assistant => "assistant",
        MessageRole::System => "system",
    }
}

pub(crate) fn parse_job_status(s: &str) -> JobStatus {
    match s {
        "running" => JobStatus::Running,
        "completed" => JobStatus::Completed,
        "failed" => JobStatus::Failed,
        "cancelled" => JobStatus::Cancelled,
        _ => JobStatus::Queued,
    }
}

pub(crate) fn parse_job_type(s: &str) -> JobType {
    match s {
        "story_chapter_outline" => JobType::StoryChapterOutline,
        "story_full_outline" | "story_propose_chapters" => JobType::StoryProposeChapters,
        "story_beat_outline" => JobType::StoryBeatOutline,
        "story_propose_beats" => JobType::StoryProposeBeats,
        "story_beat_prose" => JobType::StoryBeatProse,
        "story_beat_prose_continue" => JobType::StoryBeatProseContinue,
        "story_beat_mechanical" => JobType::StoryBeatMechanical,
        "story_beat_prose_recheck" => JobType::StoryBeatProseRecheck,
        "story_chapter_summarize" => JobType::StoryChapterSummarize,
        "story_beat_variable_recheck" => JobType::StoryBeatVariableRecheck,
        "chat_summarize" => JobType::ChatSummarize,
        "chat_variable_recheck" => JobType::ChatVariableRecheck,
        "game_turn_structured_agent" => JobType::GameTurnStructuredAgent,
        "game_scene_summarize" => JobType::GameSceneSummarize,
        "game_prose_recheck" => JobType::GameProseRecheck,
        "game_state_recheck" => JobType::GameStateRecheck,
        _ => JobType::ChatMessage,
    }
}

pub(crate) fn job_type_str(job_type: JobType) -> &'static str {
    match job_type {
        JobType::ChatMessage => "chat_message",
        JobType::ChatSummarize => "chat_summarize",
        JobType::ChatVariableRecheck => "chat_variable_recheck",
        JobType::StoryChapterOutline => "story_chapter_outline",
        JobType::StoryProposeChapters => "story_propose_chapters",
        JobType::StoryBeatOutline => "story_beat_outline",
        JobType::StoryProposeBeats => "story_propose_beats",
        JobType::StoryBeatProse => "story_beat_prose",
        JobType::StoryBeatProseContinue => "story_beat_prose_continue",
        JobType::StoryBeatMechanical => "story_beat_mechanical",
        JobType::StoryBeatProseRecheck => "story_beat_prose_recheck",
        JobType::StoryChapterSummarize => "story_chapter_summarize",
        JobType::StoryBeatVariableRecheck => "story_beat_variable_recheck",
        JobType::GameTurnStructuredAgent => "game_turn_structured_agent",
        JobType::GameSceneSummarize => "game_scene_summarize",
        JobType::GameProseRecheck => "game_prose_recheck",
        JobType::GameStateRecheck => "game_state_recheck",
    }
}

pub(crate) fn job_status_str(status: JobStatus) -> &'static str {
    match status {
        JobStatus::Queued => "queued",
        JobStatus::Running => "running",
        JobStatus::Completed => "completed",
        JobStatus::Failed => "failed",
        JobStatus::Cancelled => "cancelled",
    }
}

pub async fn list_characters(pool: &SqlitePool) -> AppResult<Vec<Character>> {
    let rows = sqlx::query_as::<_, CharacterRow>(
        "SELECT id, name, description, personality, scenario, first_message, example_dialogue, system_prompt, avatar_url, created_at, updated_at FROM characters ORDER BY updated_at DESC",
    )
    .fetch_all(pool)
    .await?;
    Ok(rows.into_iter().map(Into::into).collect())
}

pub async fn get_character(pool: &SqlitePool, id: i64) -> AppResult<Character> {
    let row = sqlx::query_as::<_, CharacterRow>(
        "SELECT id, name, description, personality, scenario, first_message, example_dialogue, system_prompt, avatar_url, created_at, updated_at FROM characters WHERE id = ?1",
    )
    .bind(id)
    .fetch_optional(pool)
    .await?
    .ok_or_else(|| AppError::not_found("Character not found"))?;
    Ok(row.into())
}

pub async fn create_character(pool: &SqlitePool, payload: CharacterCreate) -> AppResult<Character> {
    let now = Utc::now();
    let now_s = now.to_rfc3339();
    let id = sqlx::query_scalar::<_, i64>(
        "INSERT INTO characters (name, description, personality, scenario, first_message, example_dialogue, system_prompt, avatar_url, created_at, updated_at) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10) RETURNING id",
    )
    .bind(&payload.name)
    .bind(&payload.description)
    .bind(&payload.personality)
    .bind(&payload.scenario)
    .bind(&payload.first_message)
    .bind(&payload.example_dialogue)
    .bind(&payload.system_prompt)
    .bind(&payload.avatar_url)
    .bind(&now_s)
    .bind(&now_s)
    .fetch_one(pool)
    .await?;
    get_character(pool, id).await
}

pub async fn update_character(
    pool: &SqlitePool,
    id: i64,
    payload: CharacterUpdate,
) -> AppResult<Character> {
    let existing = get_character(pool, id).await?;
    let updated = Character {
        name: payload.name.unwrap_or(existing.name),
        description: payload.description.unwrap_or(existing.description),
        personality: payload.personality.unwrap_or(existing.personality),
        scenario: payload.scenario.unwrap_or(existing.scenario),
        first_message: payload.first_message.unwrap_or(existing.first_message),
        example_dialogue: payload
            .example_dialogue
            .unwrap_or(existing.example_dialogue),
        system_prompt: payload.system_prompt.unwrap_or(existing.system_prompt),
        avatar_url: payload.avatar_url.or(existing.avatar_url),
        updated_at: Utc::now(),
        ..existing
    };
    sqlx::query(
        "UPDATE characters SET name=?1, description=?2, personality=?3, scenario=?4, first_message=?5, example_dialogue=?6, system_prompt=?7, avatar_url=?8, updated_at=?9 WHERE id=?10",
    )
    .bind(&updated.name)
    .bind(&updated.description)
    .bind(&updated.personality)
    .bind(&updated.scenario)
    .bind(&updated.first_message)
    .bind(&updated.example_dialogue)
    .bind(&updated.system_prompt)
    .bind(&updated.avatar_url)
    .bind(updated.updated_at.to_rfc3339())
    .bind(id)
    .execute(pool)
    .await?;
    get_character(pool, id).await
}

pub async fn delete_character(pool: &SqlitePool, id: i64) -> AppResult<()> {
    let exists: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM characters WHERE id = ?1)")
        .bind(id)
        .fetch_one(pool)
        .await?;
    if !exists {
        return Err(AppError::not_found("Character not found"));
    }
    sqlx::query("DELETE FROM chats WHERE character_id = ?1")
        .bind(id)
        .execute(pool)
        .await?;
    sqlx::query("DELETE FROM characters WHERE id = ?1")
        .bind(id)
        .execute(pool)
        .await?;
    Ok(())
}

pub async fn list_chats(pool: &SqlitePool) -> AppResult<Vec<Chat>> {
    purge_expired_archived_chats(pool).await?;
    let rows = sqlx::query_as::<_, ChatRow>(
        "SELECT c.id, c.title, c.character_id, ch.name AS character_name, c.summary, c.created_at, c.updated_at, c.archived_at
         FROM chats c
         JOIN characters ch ON ch.id = c.character_id
         WHERE c.archived_at IS NULL
         ORDER BY c.updated_at DESC, c.title ASC",
    )
    .fetch_all(pool)
    .await?;
    let mut chats = Vec::with_capacity(rows.len());
    for row in rows {
        chats.push(chat_from_row(pool, row).await?);
    }
    Ok(chats)
}

pub async fn list_archived_chats(pool: &SqlitePool) -> AppResult<Vec<Chat>> {
    purge_expired_archived_chats(pool).await?;
    let rows = sqlx::query_as::<_, ChatRow>(
        "SELECT c.id, c.title, c.character_id, ch.name AS character_name, c.summary, c.created_at, c.updated_at, c.archived_at
         FROM chats c
         JOIN characters ch ON ch.id = c.character_id
         WHERE c.archived_at IS NOT NULL
         ORDER BY c.archived_at DESC, c.title ASC",
    )
    .fetch_all(pool)
    .await?;
    let mut chats = Vec::with_capacity(rows.len());
    for row in rows {
        chats.push(chat_from_row(pool, row).await?);
    }
    Ok(chats)
}

pub async fn get_chat(pool: &SqlitePool, id: i64) -> AppResult<Chat> {
    let row = fetch_chat_row(pool, id, false)
        .await?
        .ok_or_else(|| AppError::not_found("Chat not found"))?;
    chat_from_row(pool, row).await
}

async fn fetch_chat_row(
    pool: &SqlitePool,
    id: i64,
    include_archived: bool,
) -> AppResult<Option<ChatRow>> {
    let row = if include_archived {
        sqlx::query_as::<_, ChatRow>(
            "SELECT c.id, c.title, c.character_id, ch.name AS character_name, c.summary, c.created_at, c.updated_at, c.archived_at
             FROM chats c
             JOIN characters ch ON ch.id = c.character_id
             WHERE c.id = ?1",
        )
        .bind(id)
        .fetch_optional(pool)
        .await?
    } else {
        sqlx::query_as::<_, ChatRow>(
            "SELECT c.id, c.title, c.character_id, ch.name AS character_name, c.summary, c.created_at, c.updated_at, c.archived_at
             FROM chats c
             JOIN characters ch ON ch.id = c.character_id
             WHERE c.id = ?1 AND c.archived_at IS NULL",
        )
        .bind(id)
        .fetch_optional(pool)
        .await?
    };
    Ok(row)
}

async fn chat_from_row(pool: &SqlitePool, row: ChatRow) -> AppResult<Chat> {
    let active_job = if row.archived_at.is_none() {
        get_active_job(pool, row.id).await?
    } else {
        None
    };
    let queued_jobs: i64 = if row.archived_at.is_none() {
        sqlx::query_scalar(
            "SELECT COUNT(*) FROM generation_jobs WHERE chat_id = ?1 AND job_type IN ('chat_message', 'chat_summarize', 'chat_variable_recheck') AND status = 'queued'",
        )
        .bind(row.id)
        .fetch_one(pool)
        .await?
    } else {
        0
    };
    Ok(Chat {
        id: row.id,
        title: row.title,
        character_id: row.character_id,
        character_name: row.character_name,
        summary: row.summary,
        created_at: parse_dt(&row.created_at)?,
        updated_at: parse_dt(&row.updated_at)?,
        archived_at: row.archived_at.as_deref().map(parse_dt).transpose()?,
        active_job,
        queued_jobs,
    })
}

pub async fn create_chat(pool: &SqlitePool, title: String, character_id: i64) -> AppResult<Chat> {
    let character = get_character(pool, character_id).await?;
    let now = Utc::now().to_rfc3339();
    let id = sqlx::query_scalar::<_, i64>(
        "INSERT INTO chats (title, character_id, summary, created_at, updated_at) VALUES (?1,?2,'',?3,?3) RETURNING id",
    )
    .bind(&title)
    .bind(character_id)
    .bind(&now)
    .fetch_one(pool)
    .await?;

    seed_character_greeting(pool, id, &character).await?;
    seed_chat_pc_actor(pool, id).await?;
    get_chat(pool, id).await
}

pub async fn seed_chat_pc_actor(pool: &SqlitePool, chat_id: i64) -> AppResult<()> {
    let settings = get_settings(pool).await?;
    let now = Utc::now().to_rfc3339();
    sqlx::query(
        "INSERT INTO chat_actors (chat_id, role, name, description, skills, created_at, updated_at) VALUES (?1,'pc',?2,?3,'{}',?4,?4)",
    )
    .bind(chat_id)
    .bind(if settings.user_name.trim().is_empty() {
        DEFAULT_USER_NAME
    } else {
        settings.user_name.trim()
    })
    .bind(settings.persona_description.trim())
    .bind(&now)
    .execute(pool)
    .await?;
    Ok(())
}

pub async fn get_chat_detail(pool: &SqlitePool, chat_id: i64) -> AppResult<ChatDetail> {
    let chat = get_chat(pool, chat_id).await?;
    let actors = list_chat_actors(pool, chat_id).await?;
    let state = list_chat_state_entries(pool, chat_id).await?;
    Ok(ChatDetail {
        chat,
        actors,
        state,
    })
}

pub async fn list_chat_actors(pool: &SqlitePool, chat_id: i64) -> AppResult<Vec<ChatActor>> {
    let rows = sqlx::query_as::<_, ChatActorRow>(
        "SELECT id, chat_id, role, name, description, skills, sort_order, created_at, updated_at FROM chat_actors WHERE chat_id = ?1 ORDER BY sort_order ASC, id ASC",
    )
    .bind(chat_id)
    .fetch_all(pool)
    .await?;
    rows.into_iter().map(chat_actor_from_row).collect()
}

pub async fn list_chat_state_entries(
    pool: &SqlitePool,
    chat_id: i64,
) -> AppResult<Vec<ChatStateEntry>> {
    let rows = sqlx::query_as::<_, ChatStateRow>(
        "SELECT id, chat_id, actor_id, kind, key, value, num_value, max_value, source_message_id, updated_at FROM chat_state_entries WHERE chat_id = ?1 ORDER BY kind ASC, key ASC",
    )
    .bind(chat_id)
    .fetch_all(pool)
    .await?;
    rows.into_iter().map(chat_state_from_row).collect()
}

pub async fn update_chat_state_entry(
    pool: &SqlitePool,
    chat_id: i64,
    entry_id: i64,
    payload: ChatStateEntryUpdate,
) -> AppResult<ChatStateEntry> {
    let row = sqlx::query_as::<_, ChatStateRow>(
        "SELECT id, chat_id, actor_id, kind, key, value, num_value, max_value, source_message_id, updated_at FROM chat_state_entries WHERE id = ?1 AND chat_id = ?2",
    )
    .bind(entry_id)
    .bind(chat_id)
    .fetch_optional(pool)
    .await?
    .ok_or_else(|| AppError::not_found("State entry not found"))?;
    let mut entry = chat_state_from_row(row)?;
    if let Some(value) = payload.value {
        entry.value = value;
    }
    if let Some(num) = payload.num_value {
        entry.num_value = Some(num);
    }
    if let Some(max) = payload.max_value {
        entry.max_value = Some(max);
    }
    entry.updated_at = Utc::now();
    sqlx::query(
        "UPDATE chat_state_entries SET value=?1, num_value=?2, max_value=?3, source_message_id=-1, updated_at=?4 WHERE id=?5",
    )
    .bind(&entry.value)
    .bind(entry.num_value)
    .bind(entry.max_value)
    .bind(entry.updated_at.to_rfc3339())
    .bind(entry_id)
    .execute(pool)
    .await?;
    Ok(entry)
}

fn parse_state_kind(s: &str) -> StateKind {
    match s {
        "resource" => StateKind::Resource,
        "condition" => StateKind::Condition,
        "fact" => StateKind::Fact,
        "clock" => StateKind::Clock,
        _ => StateKind::Fact,
    }
}

fn chat_actor_from_row(row: ChatActorRow) -> AppResult<ChatActor> {
    Ok(ChatActor {
        id: row.id,
        chat_id: row.chat_id,
        role: row.role,
        name: row.name,
        description: row.description,
        skills: serde_json::from_str(&row.skills).unwrap_or_default(),
        sort_order: row.sort_order,
        created_at: parse_dt(&row.created_at)?,
        updated_at: parse_dt(&row.updated_at)?,
    })
}

fn chat_state_from_row(row: ChatStateRow) -> AppResult<ChatStateEntry> {
    Ok(ChatStateEntry {
        id: row.id,
        chat_id: row.chat_id,
        actor_id: row.actor_id,
        kind: parse_state_kind(&row.kind),
        key: row.key,
        value: row.value,
        num_value: row.num_value,
        max_value: row.max_value,
        source_message_id: row.source_message_id,
        updated_at: parse_dt(&row.updated_at)?,
    })
}

fn parse_applied_state_changes(raw: &str) -> Vec<AppliedStateChange> {
    serde_json::from_str(raw).unwrap_or_default()
}

fn parse_string_array(raw: &str) -> Vec<String> {
    serde_json::from_str(raw).unwrap_or_default()
}

async fn seed_character_greeting(
    pool: &SqlitePool,
    chat_id: i64,
    character: &Character,
) -> AppResult<()> {
    if !character.first_message.trim().is_empty() {
        insert_message(
            pool,
            chat_id,
            MessageRole::Assistant,
            character.first_message.trim().to_string(),
            false,
        )
        .await?;
    }
    Ok(())
}

pub async fn update_chat(pool: &SqlitePool, id: i64, payload: ChatUpdate) -> AppResult<Chat> {
    let existing = get_chat(pool, id).await?;
    let title = payload.title.unwrap_or(existing.title);
    let character_id = payload.character_id.unwrap_or(existing.character_id);
    let summary_updated = payload.summary.is_some();
    let previous_summary = existing.summary.clone();
    let summary = payload.summary.unwrap_or(existing.summary);
    let _ = get_character(pool, character_id).await?;
    let now = Utc::now().to_rfc3339();
    sqlx::query(
        "UPDATE chats SET title=?1, character_id=?2, summary=?3, updated_at=?4 WHERE id=?5",
    )
    .bind(&title)
    .bind(character_id)
    .bind(&summary)
    .bind(&now)
    .bind(id)
    .execute(pool)
    .await?;

    if summary_updated && summary != previous_summary {
        crate::summarize::refresh_chat_summary_markers(pool, id, &summary).await?;
    }

    if character_id != existing.character_id {
        let messages = list_messages(pool, id).await?;
        if messages.is_empty() {
            let character = get_character(pool, character_id).await?;
            seed_character_greeting(pool, id, &character).await?;
        }
    }

    get_chat(pool, id).await
}

pub async fn archive_chat(pool: &SqlitePool, id: i64) -> AppResult<()> {
    let exists = fetch_chat_row(pool, id, false).await?;
    if exists.is_none() {
        return Err(AppError::not_found("Chat not found"));
    }
    let now = Utc::now().to_rfc3339();
    sqlx::query("UPDATE chats SET archived_at = ?1 WHERE id = ?2 AND archived_at IS NULL")
        .bind(&now)
        .bind(id)
        .execute(pool)
        .await?;
    Ok(())
}

pub async fn restore_chat(pool: &SqlitePool, id: i64) -> AppResult<Chat> {
    let exists = fetch_chat_row(pool, id, true).await?;
    if exists
        .as_ref()
        .and_then(|row| row.archived_at.as_deref())
        .is_none()
    {
        return Err(AppError::not_found("Archived chat not found"));
    }
    let now = Utc::now().to_rfc3339();
    sqlx::query("UPDATE chats SET archived_at = NULL, updated_at = ?1 WHERE id = ?2")
        .bind(&now)
        .bind(id)
        .execute(pool)
        .await?;
    get_chat(pool, id).await
}

pub async fn permanently_delete_chat(pool: &SqlitePool, id: i64) -> AppResult<()> {
    let result = sqlx::query("DELETE FROM chats WHERE id = ?1 AND archived_at IS NOT NULL")
        .bind(id)
        .execute(pool)
        .await?;
    if result.rows_affected() == 0 {
        return Err(AppError::not_found("Archived chat not found"));
    }
    Ok(())
}

pub async fn purge_expired_archived_chats(pool: &SqlitePool) -> AppResult<u64> {
    let cutoff = (Utc::now() - chrono::Duration::days(CHAT_ARCHIVE_RETENTION_DAYS)).to_rfc3339();
    let result =
        sqlx::query("DELETE FROM chats WHERE archived_at IS NOT NULL AND archived_at < ?1")
            .bind(&cutoff)
            .execute(pool)
            .await?;
    Ok(result.rows_affected())
}

pub async fn list_active_jobs_for_chat(pool: &SqlitePool, chat_id: i64) -> AppResult<Vec<Job>> {
    let rows = sqlx::query_as::<_, JobRow>(
        "SELECT id, job_type, chat_id, message_id, story_id, chapter_id, beat_id, game_id, turn_id, guidance_notes, status, error, position, created_at, started_at, completed_at FROM generation_jobs WHERE chat_id = ?1 AND job_type IN ('chat_message', 'chat_summarize', 'chat_variable_recheck') AND status IN ('queued','running') ORDER BY created_at ASC",
    )
    .bind(chat_id)
    .fetch_all(pool)
    .await?;
    Ok(rows.into_iter().map(Into::into).collect())
}

pub async fn list_messages(pool: &SqlitePool, chat_id: i64) -> AppResult<Vec<Message>> {
    let _ = get_chat(pool, chat_id).await?;
    let rows = sqlx::query_as::<_, MessageRow>(
        "SELECT m.id, m.chat_id, m.role, m.content, m.thought_content, m.thought_duration_ms, m.thought_in_progress, m.variable_updates, m.reply_beats, m.state_changes, m.generation_phase, m.is_summary, m.in_summary, m.created_at, j.status as job_status, (SELECT gj.error FROM generation_jobs gj WHERE gj.message_id = m.id AND gj.status = 'failed' ORDER BY gj.completed_at DESC LIMIT 1) as generation_error FROM messages m LEFT JOIN generation_jobs j ON j.id = (SELECT id FROM generation_jobs WHERE message_id = m.id AND status IN ('queued','running') ORDER BY created_at DESC LIMIT 1) WHERE m.chat_id = ?1 ORDER BY m.created_at ASC",
    )
    .bind(chat_id)
    .fetch_all(pool)
    .await?;
    Ok(rows.into_iter().map(message_from_row).collect())
}

pub async fn get_message(pool: &SqlitePool, chat_id: i64, message_id: i64) -> AppResult<Message> {
    list_messages(pool, chat_id)
        .await?
        .into_iter()
        .find(|m| m.id == message_id)
        .ok_or_else(|| AppError::not_found("Message not found"))
}

pub async fn is_last_message(pool: &SqlitePool, chat_id: i64, message_id: i64) -> AppResult<bool> {
    let messages = list_messages(pool, chat_id).await?;
    Ok(messages.last().map(|m| m.id) == Some(message_id))
}

pub async fn list_active_jobs_for_message(
    pool: &SqlitePool,
    message_id: i64,
) -> AppResult<Vec<Job>> {
    let rows = sqlx::query_as::<_, JobRow>(
        "SELECT id, job_type, chat_id, message_id, story_id, chapter_id, beat_id, game_id, turn_id, guidance_notes, status, error, position, created_at, started_at, completed_at FROM generation_jobs WHERE message_id = ?1 AND status IN ('queued','running') ORDER BY created_at ASC",
    )
    .bind(message_id)
    .fetch_all(pool)
    .await?;
    Ok(rows.into_iter().map(Into::into).collect())
}

pub async fn delete_messages_after(
    pool: &SqlitePool,
    chat_id: i64,
    message_id: i64,
) -> AppResult<Vec<i64>> {
    let messages = list_messages(pool, chat_id).await?;
    let Some(idx) = messages.iter().position(|m| m.id == message_id) else {
        return Err(AppError::not_found("Message not found"));
    };
    let to_delete: Vec<i64> = messages[(idx + 1)..].iter().map(|m| m.id).collect();
    if !to_delete.is_empty() {
        delete_messages(pool, &to_delete).await?;
    }
    Ok(to_delete)
}

pub async fn insert_message(
    pool: &SqlitePool,
    chat_id: i64,
    role: MessageRole,
    content: String,
    is_summary: bool,
) -> AppResult<Message> {
    let now = Utc::now().to_rfc3339();
    let id = sqlx::query_scalar::<_, i64>(
        "INSERT INTO messages (chat_id, role, content, is_summary, created_at) VALUES (?1,?2,?3,?4,?5) RETURNING id",
    )
    .bind(chat_id)
    .bind(role_str(role))
    .bind(&content)
    .bind(is_summary as i64)
    .bind(&now)
    .fetch_one(pool)
    .await?;
    Ok(Message {
        id,
        chat_id,
        role,
        content,
        thought_content: String::new(),
        thought_duration_ms: None,
        thought_in_progress: false,
        variable_updates: Vec::new(),
        reply_beats: Vec::new(),
        state_changes: Vec::new(),
        generation_phase: String::new(),
        is_summary,
        in_summary: false,
        created_at: parse_dt(&now)?,
        job_status: None,
        generation_error: None,
    })
}

/// Keep partial reply text when generation fails; only write a placeholder when empty.
pub async fn fail_chat_message_generation(
    pool: &SqlitePool,
    message_id: i64,
    error: &str,
) -> AppResult<()> {
    let current = get_message_generation_snapshot(pool, message_id).await?;
    if current.content.trim().is_empty() {
        update_message_content(pool, message_id, &format!("[Generation failed: {error}]")).await?;
    }
    set_thought_in_progress(pool, message_id, false).await?;
    Ok(())
}

pub async fn mark_messages_in_summary(pool: &SqlitePool, ids: &[i64]) -> AppResult<()> {
    for id in ids {
        sqlx::query("UPDATE messages SET in_summary = 1 WHERE id = ?1")
            .bind(id)
            .execute(pool)
            .await?;
    }
    Ok(())
}

pub async fn clear_messages_in_summary(pool: &SqlitePool, chat_id: i64) -> AppResult<()> {
    sqlx::query("UPDATE messages SET in_summary = 0 WHERE chat_id = ?1")
        .bind(chat_id)
        .execute(pool)
        .await?;
    Ok(())
}

pub async fn update_message_content(
    pool: &SqlitePool,
    message_id: i64,
    content: &str,
) -> AppResult<()> {
    sqlx::query("UPDATE messages SET content = ?1 WHERE id = ?2")
        .bind(content)
        .bind(message_id)
        .execute(pool)
        .await?;
    Ok(())
}

pub async fn update_message_content_and_variable_updates(
    pool: &SqlitePool,
    message_id: i64,
    content: &str,
    variable_updates: &[dreamwell_types::MessageVariableUpdate],
) -> AppResult<()> {
    let variable_updates_json = serde_json::to_string(variable_updates)
        .map_err(|e| AppError::internal(format!("serialize variable updates: {e}")))?;
    sqlx::query("UPDATE messages SET content = ?1, variable_updates = ?2 WHERE id = ?3")
        .bind(content)
        .bind(variable_updates_json)
        .bind(message_id)
        .execute(pool)
        .await?;
    Ok(())
}

#[derive(Debug, Clone)]
pub struct MessageGenerationSnapshot {
    pub content: String,
    pub thought_content: String,
    pub thought_duration_ms: Option<i64>,
    pub thought_in_progress: bool,
}

pub async fn get_message_generation_snapshot(
    pool: &SqlitePool,
    message_id: i64,
) -> AppResult<MessageGenerationSnapshot> {
    let row = sqlx::query_as::<_, (String, String, Option<i64>, i64)>(
        "SELECT content, thought_content, thought_duration_ms, thought_in_progress FROM messages WHERE id = ?1",
    )
    .bind(message_id)
    .fetch_optional(pool)
    .await?
    .ok_or_else(|| AppError::not_found("Message not found"))?;
    Ok(MessageGenerationSnapshot {
        content: row.0,
        thought_content: row.1,
        thought_duration_ms: row.2,
        thought_in_progress: row.3 != 0,
    })
}

/// Avoid wiping previously saved generation text when a retried or empty stream
/// would write blank fields over a completed message.
fn merge_generation_fields(
    current: &MessageGenerationSnapshot,
    content: &str,
    thought_content: &str,
    thought_duration_ms: Option<i64>,
    thought_in_progress: bool,
) -> (String, String, Option<i64>, bool) {
    let kept_content = content.is_empty() && !current.content.is_empty();
    let kept_thought = thought_content.is_empty() && !current.thought_content.is_empty();

    let final_content = if kept_content {
        current.content.clone()
    } else {
        content.to_string()
    };
    let final_thought = if kept_thought {
        current.thought_content.clone()
    } else {
        thought_content.to_string()
    };
    let final_duration = if kept_thought {
        current.thought_duration_ms.or(thought_duration_ms)
    } else if final_thought.is_empty() {
        None
    } else {
        thought_duration_ms
    };
    // Preserve in-progress state when a transient empty thought update would
    // otherwise keep prior reasoning text during active streaming.
    let final_in_progress = if kept_content {
        false
    } else {
        thought_in_progress
    };

    (
        final_content,
        final_thought,
        final_duration,
        final_in_progress,
    )
}

pub async fn update_message_generation(
    pool: &SqlitePool,
    message_id: i64,
    content: &str,
    thought_content: &str,
    thought_duration_ms: Option<i64>,
    thought_in_progress: bool,
) -> AppResult<()> {
    let current = get_message_generation_snapshot(pool, message_id).await?;
    let (content, thought_content, thought_duration_ms, thought_in_progress) =
        merge_generation_fields(
            &current,
            content,
            thought_content,
            thought_duration_ms,
            thought_in_progress,
        );
    sqlx::query(
        "UPDATE messages SET content = ?1, thought_content = ?2, thought_duration_ms = ?3, thought_in_progress = ?4 WHERE id = ?5",
    )
    .bind(&content)
    .bind(&thought_content)
    .bind(thought_duration_ms)
    .bind(thought_in_progress as i64)
    .bind(message_id)
    .execute(pool)
    .await?;
    Ok(())
}

pub async fn clear_message_thoughts(pool: &SqlitePool, message_id: i64) -> AppResult<()> {
    sqlx::query(
        "UPDATE messages SET thought_content = '', thought_duration_ms = NULL, thought_in_progress = 0 WHERE id = ?1",
    )
    .bind(message_id)
    .execute(pool)
    .await?;
    Ok(())
}

pub async fn clear_message_variable_updates(pool: &SqlitePool, message_id: i64) -> AppResult<()> {
    sqlx::query("UPDATE messages SET variable_updates = '[]' WHERE id = ?1")
        .bind(message_id)
        .execute(pool)
        .await?;
    Ok(())
}

pub async fn set_thought_in_progress(
    pool: &SqlitePool,
    message_id: i64,
    in_progress: bool,
) -> AppResult<()> {
    sqlx::query("UPDATE messages SET thought_in_progress = ?1 WHERE id = ?2")
        .bind(in_progress as i64)
        .bind(message_id)
        .execute(pool)
        .await?;
    Ok(())
}

pub async fn finalize_message_generation(
    pool: &SqlitePool,
    message_id: i64,
    content: &str,
    thought_content: &str,
    thought_duration_ms: Option<i64>,
    thought_in_progress: bool,
    variable_updates: &[dreamwell_types::MessageVariableUpdate],
) -> AppResult<()> {
    let current = get_message_generation_snapshot(pool, message_id).await?;
    let (content, thought_content, thought_duration_ms, thought_in_progress) =
        merge_generation_fields(
            &current,
            content,
            thought_content,
            thought_duration_ms,
            thought_in_progress,
        );
    let variable_updates_json = serde_json::to_string(variable_updates)
        .map_err(|e| AppError::internal(format!("serialize variable updates: {e}")))?;
    sqlx::query(
        "UPDATE messages SET content = ?1, thought_content = ?2, thought_duration_ms = ?3, thought_in_progress = ?4, variable_updates = ?5 WHERE id = ?6",
    )
    .bind(&content)
    .bind(&thought_content)
    .bind(thought_duration_ms)
    .bind(thought_in_progress as i64)
    .bind(variable_updates_json)
    .bind(message_id)
    .execute(pool)
    .await?;
    Ok(())
}

pub async fn touch_chat(pool: &SqlitePool, chat_id: i64) -> AppResult<()> {
    let now = Utc::now().to_rfc3339();
    sqlx::query("UPDATE chats SET updated_at = ?1 WHERE id = ?2")
        .bind(&now)
        .bind(chat_id)
        .execute(pool)
        .await?;
    Ok(())
}

pub async fn list_variables(pool: &SqlitePool, chat_id: i64) -> AppResult<Vec<ChatVariable>> {
    let entries = list_chat_state_entries(pool, chat_id).await?;
    Ok(entries
        .into_iter()
        .filter(|e| e.kind == StateKind::Fact && e.actor_id.is_none())
        .map(|e| ChatVariable {
            id: e.id,
            chat_id: e.chat_id,
            key: e.key,
            value: e.value,
            source_message_id: e.source_message_id,
            updated_at: e.updated_at,
        })
        .collect())
}

pub async fn upsert_variable(
    pool: &SqlitePool,
    chat_id: i64,
    key: String,
    value: String,
    source_message_id: i64,
) -> AppResult<ChatVariable> {
    let now = Utc::now().to_rfc3339();
    sqlx::query(
        "INSERT INTO chat_state_entries (chat_id, actor_id, kind, key, value, source_message_id, updated_at) VALUES (?1,NULL,'fact',?2,?3,?4,?5) ON CONFLICT(chat_id, actor_id, kind, key) DO UPDATE SET value=excluded.value, source_message_id=excluded.source_message_id, updated_at=excluded.updated_at",
    )
    .bind(chat_id)
    .bind(&key)
    .bind(&value)
    .bind(source_message_id)
    .bind(&now)
    .execute(pool)
    .await?;
    let entries = list_chat_state_entries(pool, chat_id).await?;
    entries
        .into_iter()
        .find(|e| e.key == key && e.kind == StateKind::Fact && e.actor_id.is_none())
        .map(|e| ChatVariable {
            id: e.id,
            chat_id: e.chat_id,
            key: e.key,
            value: e.value,
            source_message_id: e.source_message_id,
            updated_at: e.updated_at,
        })
        .ok_or_else(|| AppError::internal("variable upsert failed"))
}

pub async fn upsert_variable_manual(
    pool: &SqlitePool,
    chat_id: i64,
    payload: ChatVariableUpdate,
) -> AppResult<ChatVariable> {
    let source_message_id = payload
        .source_message_id
        .unwrap_or(crate::variable_state::MANUAL_MESSAGE_SOURCE);
    upsert_variable(pool, chat_id, payload.key, payload.value, source_message_id).await
}

pub async fn delete_variable(pool: &SqlitePool, chat_id: i64, variable_id: i64) -> AppResult<()> {
    let result = sqlx::query(
        "DELETE FROM chat_state_entries WHERE chat_id = ?1 AND id = ?2 AND kind = 'fact'",
    )
    .bind(chat_id)
    .bind(variable_id)
    .execute(pool)
    .await?;
    if result.rows_affected() == 0 {
        return Err(AppError::not_found("Variable not found"));
    }
    Ok(())
}

pub async fn get_chat_variable(
    pool: &SqlitePool,
    chat_id: i64,
    variable_id: i64,
) -> AppResult<ChatVariable> {
    let entries = list_chat_state_entries(pool, chat_id).await?;
    entries
        .into_iter()
        .find(|e| e.id == variable_id)
        .map(|e| ChatVariable {
            id: e.id,
            chat_id: e.chat_id,
            key: e.key,
            value: e.value,
            source_message_id: e.source_message_id,
            updated_at: e.updated_at,
        })
        .ok_or_else(|| AppError::not_found("Variable not found"))
}

pub async fn delete_variable_scoped(
    pool: &SqlitePool,
    chat_id: i64,
    key: &str,
    source_message_id: i64,
) -> AppResult<()> {
    let _ = sqlx::query(
        "DELETE FROM chat_state_entries WHERE chat_id = ?1 AND key = ?2 AND source_message_id = ?3 AND kind = 'fact' AND actor_id IS NULL",
    )
    .bind(chat_id)
    .bind(key)
    .bind(source_message_id)
    .execute(pool)
    .await?;
    Ok(())
}

pub async fn clear_message_typed_state(pool: &SqlitePool, message_id: i64) -> AppResult<()> {
    sqlx::query(
        "UPDATE messages SET reply_beats='[]', state_changes='[]', generation_phase='' WHERE id = ?1",
    )
    .bind(message_id)
    .execute(pool)
    .await?;
    Ok(())
}

pub async fn update_message_generation_phase(
    pool: &SqlitePool,
    message_id: i64,
    phase: &str,
) -> AppResult<()> {
    sqlx::query("UPDATE messages SET generation_phase = ?1 WHERE id = ?2")
        .bind(phase)
        .bind(message_id)
        .execute(pool)
        .await?;
    Ok(())
}

pub async fn save_message_plan(
    pool: &SqlitePool,
    message_id: i64,
    reply_beats: &[String],
    state_changes: &[AppliedStateChange],
) -> AppResult<()> {
    let beats_json = serde_json::to_string(reply_beats)
        .map_err(|e| AppError::internal(format!("serialize reply_beats: {e}")))?;
    let changes_json = serde_json::to_string(state_changes)
        .map_err(|e| AppError::internal(format!("serialize state_changes: {e}")))?;
    sqlx::query(
        "UPDATE messages SET reply_beats=?1, state_changes=?2, generation_phase='prose' WHERE id=?3",
    )
    .bind(beats_json)
    .bind(changes_json)
    .bind(message_id)
    .execute(pool)
    .await?;
    Ok(())
}

#[allow(clippy::too_many_arguments)]
pub async fn finalize_message_typed_generation(
    pool: &SqlitePool,
    message_id: i64,
    content: &str,
    thought_content: &str,
    thought_duration_ms: Option<i64>,
    thought_in_progress: bool,
    reply_beats: &[String],
    state_changes: &[AppliedStateChange],
) -> AppResult<()> {
    let current = get_message_generation_snapshot(pool, message_id).await?;
    let (content, thought_content, thought_duration_ms, thought_in_progress) =
        merge_generation_fields(
            &current,
            content,
            thought_content,
            thought_duration_ms,
            thought_in_progress,
        );
    let beats_json = serde_json::to_string(reply_beats)
        .map_err(|e| AppError::internal(format!("serialize reply_beats: {e}")))?;
    let changes_json = serde_json::to_string(state_changes)
        .map_err(|e| AppError::internal(format!("serialize state_changes: {e}")))?;
    sqlx::query(
        "UPDATE messages SET content = ?1, thought_content = ?2, thought_duration_ms = ?3, thought_in_progress = ?4, reply_beats = ?5, state_changes = ?6, generation_phase = 'complete', variable_updates = '[]' WHERE id = ?7",
    )
    .bind(&content)
    .bind(&thought_content)
    .bind(thought_duration_ms)
    .bind(thought_in_progress as i64)
    .bind(beats_json)
    .bind(changes_json)
    .bind(message_id)
    .execute(pool)
    .await?;
    Ok(())
}

pub async fn get_settings(pool: &SqlitePool) -> AppResult<Settings> {
    let row = sqlx::query_as::<_, SettingsRow>(
        "SELECT inference_url, active_inference_connection_id, model, temperature, top_p, max_tokens, system_prompt_prefix, system_prompt_suffix, user_name, persona_description, summarize_enabled, summarize_adaptive, summarize_after_messages, summarize_keep_recent, variables_enabled, thought_blocks_enabled, max_context_messages, context_tokens, auto_context_on_model_change FROM app_settings WHERE id = 1",
    )
    .fetch_one(pool)
    .await?;
    let connections = list_inference_connections(pool).await?;
    let mut settings = row.into_settings(connections.clone());
    if let Some(active_id) = settings.active_connection_id {
        if let Some(active) = connections.iter().find(|c| c.id == active_id) {
            apply_connection_profile(&mut settings, active);
        }
    }
    Ok(settings)
}

pub async fn get_inference_config(pool: &SqlitePool) -> AppResult<InferenceConfig> {
    let row = sqlx::query_as::<_, ActiveConnectionRow>(
        "SELECT s.inference_url AS fallback_url, c.id AS connection_id, c.inference_url AS connection_url, c.api_key, COALESCE(c.json_format_strategy, 'auto') AS json_format_strategy, COALESCE(c.tool_call_parser, 'auto') AS tool_call_parser
         FROM app_settings s
         LEFT JOIN inference_connections c ON c.id = s.active_inference_connection_id
         WHERE s.id = 1",
    )
    .fetch_one(pool)
    .await?;

    let base_url = row
        .connection_url
        .filter(|url| !url.is_empty())
        .unwrap_or(row.fallback_url);
    Ok(InferenceConfig::with_connection(
        base_url,
        row.api_key,
        row.connection_id,
        parse_json_format_strategy(&row.json_format_strategy),
        row.tool_call_parser,
    ))
}

/// Structured JSON completion using the active connection's format preference.
#[allow(clippy::too_many_arguments)]
pub async fn chat_completion_json_for_connection<T>(
    pool: &SqlitePool,
    config: &InferenceConfig,
    model: &str,
    messages: &[serde_json::Value],
    temperature: f64,
    top_p: f64,
    max_tokens: i64,
    response_format: Option<&serde_json::Value>,
    max_attempts: u32,
    token: &CancellationToken,
) -> AppResult<T>
where
    T: serde::de::DeserializeOwned,
{
    let mut learned = None;
    let result = chat_completion_json(
        config,
        model,
        messages,
        temperature,
        top_p,
        max_tokens,
        response_format,
        max_attempts,
        token,
        &mut learned,
    )
    .await?;
    if let (Some(connection_id), Some(strategy)) = (config.connection_id, learned) {
        persist_learned_json_format_strategy(pool, connection_id, strategy).await?;
    }
    Ok(result)
}

pub async fn persist_learned_json_format_strategy(
    pool: &SqlitePool,
    connection_id: i64,
    strategy: JsonFormatStrategy,
) -> AppResult<()> {
    if strategy == JsonFormatStrategy::Auto {
        return Ok(());
    }
    sqlx::query(
        "UPDATE inference_connections SET json_format_strategy = ?1 WHERE id = ?2 AND json_format_strategy = 'auto'",
    )
    .bind(json_format_strategy_to_db(strategy))
    .bind(connection_id)
    .execute(pool)
    .await?;
    Ok(())
}

fn json_format_strategy_to_db(strategy: JsonFormatStrategy) -> &'static str {
    match strategy {
        JsonFormatStrategy::Auto => "auto",
        JsonFormatStrategy::ResponseJsonSchema => "response_json_schema",
        JsonFormatStrategy::GuidedJson => "guided_json",
        JsonFormatStrategy::JsonObject => "json_object",
    }
}

fn parse_json_format_strategy(raw: &str) -> JsonFormatStrategy {
    match raw {
        "response_json_schema" => JsonFormatStrategy::ResponseJsonSchema,
        "guided_json" => JsonFormatStrategy::GuidedJson,
        "json_object" => JsonFormatStrategy::JsonObject,
        _ => JsonFormatStrategy::Auto,
    }
}

const INFERENCE_CONNECTION_SELECT: &str = "SELECT id, name, inference_url, api_key, model, json_format_strategy, tool_call_parser, temperature, top_p, max_tokens, context_tokens, max_context_messages, auto_context_on_model_change FROM inference_connections";

fn apply_connection_profile(settings: &mut Settings, conn: &InferenceConnection) {
    settings.inference_url = conn.inference_url.clone();
    settings.model = conn.model.clone();
    settings.temperature = conn.temperature;
    settings.top_p = conn.top_p;
    settings.max_tokens = conn.max_tokens;
    settings.context_tokens = conn.context_tokens;
    settings.max_context_messages = conn.max_context_messages;
    settings.auto_context_on_model_change = conn.auto_context_on_model_change;
}

async fn snapshot_active_connection_settings(
    pool: &SqlitePool,
    connection: &InferenceConnection,
) -> AppResult<()> {
    sqlx::query(
        "UPDATE app_settings SET inference_url = ?1, model = ?2, temperature = ?3, top_p = ?4, max_tokens = ?5, context_tokens = ?6, max_context_messages = ?7, auto_context_on_model_change = ?8 WHERE id = 1",
    )
    .bind(&connection.inference_url)
    .bind(&connection.model)
    .bind(connection.temperature)
    .bind(connection.top_p)
    .bind(connection.max_tokens)
    .bind(connection.context_tokens)
    .bind(connection.max_context_messages)
    .bind(connection.auto_context_on_model_change as i64)
    .execute(pool)
    .await?;
    Ok(())
}

pub async fn list_inference_connections(pool: &SqlitePool) -> AppResult<Vec<InferenceConnection>> {
    let query = format!("{INFERENCE_CONNECTION_SELECT} ORDER BY id");
    let rows = sqlx::query_as::<_, InferenceConnectionRow>(&query)
        .fetch_all(pool)
        .await?;
    Ok(rows
        .into_iter()
        .map(InferenceConnectionRow::into_connection)
        .collect())
}

pub async fn create_inference_connection(
    pool: &SqlitePool,
    payload: InferenceConnectionCreate,
) -> AppResult<InferenceConnection> {
    let api_key = payload.api_key.unwrap_or_default();
    let id = sqlx::query_scalar::<_, i64>(
        "INSERT INTO inference_connections (name, inference_url, api_key) VALUES (?1, ?2, ?3) RETURNING id",
    )
    .bind(payload.name.trim())
    .bind(payload.inference_url.trim())
    .bind(api_key)
    .fetch_one(pool)
    .await?;
    get_inference_connection(pool, id).await
}

pub async fn get_inference_connection(
    pool: &SqlitePool,
    id: i64,
) -> AppResult<InferenceConnection> {
    let query = format!("{INFERENCE_CONNECTION_SELECT} WHERE id = ?1");
    let row = sqlx::query_as::<_, InferenceConnectionRow>(&query)
        .bind(id)
        .fetch_optional(pool)
        .await?
        .ok_or_else(|| AppError::not_found("Inference connection not found"))?;
    Ok(row.into_connection())
}

pub async fn update_inference_connection(
    pool: &SqlitePool,
    id: i64,
    payload: InferenceConnectionUpdate,
) -> AppResult<InferenceConnection> {
    let query = format!("{INFERENCE_CONNECTION_SELECT} WHERE id = ?1");
    let current = sqlx::query_as::<_, InferenceConnectionRow>(&query)
        .bind(id)
        .fetch_optional(pool)
        .await?
        .ok_or_else(|| AppError::not_found("Inference connection not found"))?;

    let name = payload.name.unwrap_or(current.name);
    let inference_url = payload.inference_url.unwrap_or(current.inference_url);
    let api_key = match payload.api_key {
        Some(key) => key,
        None => current.api_key,
    };
    let json_format_strategy = payload
        .json_format_strategy
        .map(json_format_strategy_to_db)
        .unwrap_or(current.json_format_strategy.as_str());
    let tool_call_parser = payload.tool_call_parser.unwrap_or(current.tool_call_parser);
    let model = payload.model.unwrap_or(current.model);
    let temperature = payload.temperature.unwrap_or(current.temperature);
    let top_p = payload.top_p.unwrap_or(current.top_p);
    let max_tokens = payload.max_tokens.unwrap_or(current.max_tokens);
    let context_tokens = payload.context_tokens.unwrap_or(current.context_tokens);
    let max_context_messages = payload
        .max_context_messages
        .unwrap_or(current.max_context_messages);
    let auto_context_on_model_change = payload
        .auto_context_on_model_change
        .unwrap_or(current.auto_context_on_model_change != 0);

    sqlx::query(
        "UPDATE inference_connections SET name = ?1, inference_url = ?2, api_key = ?3, model = ?4, json_format_strategy = ?5, tool_call_parser = ?6, temperature = ?7, top_p = ?8, max_tokens = ?9, context_tokens = ?10, max_context_messages = ?11, auto_context_on_model_change = ?12 WHERE id = ?13",
    )
    .bind(name.trim())
    .bind(inference_url.trim())
    .bind(api_key)
    .bind(model)
    .bind(json_format_strategy)
    .bind(tool_call_parser)
    .bind(temperature)
    .bind(top_p)
    .bind(max_tokens)
    .bind(context_tokens)
    .bind(max_context_messages)
    .bind(auto_context_on_model_change as i64)
    .bind(id)
    .execute(pool)
    .await?;

    sync_active_connection_snapshot(pool).await?;
    get_inference_connection(pool, id).await
}

pub async fn delete_inference_connection(pool: &SqlitePool, id: i64) -> AppResult<()> {
    let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM inference_connections")
        .fetch_one(pool)
        .await?;
    if count <= 1 {
        return Err(AppError::bad_request(
            "Cannot delete the only inference connection",
        ));
    }

    let active_id: Option<i64> =
        sqlx::query_scalar("SELECT active_inference_connection_id FROM app_settings WHERE id = 1")
            .fetch_one(pool)
            .await?;

    sqlx::query("DELETE FROM inference_connections WHERE id = ?1")
        .bind(id)
        .execute(pool)
        .await?;

    if active_id == Some(id) {
        let replacement: i64 =
            sqlx::query_scalar("SELECT id FROM inference_connections ORDER BY id LIMIT 1")
                .fetch_one(pool)
                .await?;
        set_active_inference_connection(pool, replacement).await?;
    }

    Ok(())
}

async fn set_active_inference_connection(pool: &SqlitePool, id: i64) -> AppResult<()> {
    let connection = get_inference_connection(pool, id).await?;
    sqlx::query(
        "UPDATE app_settings SET active_inference_connection_id = ?1, inference_url = ?2, model = ?3, temperature = ?4, top_p = ?5, max_tokens = ?6, context_tokens = ?7, max_context_messages = ?8, auto_context_on_model_change = ?9 WHERE id = 1",
    )
    .bind(id)
    .bind(&connection.inference_url)
    .bind(&connection.model)
    .bind(connection.temperature)
    .bind(connection.top_p)
    .bind(connection.max_tokens)
    .bind(connection.context_tokens)
    .bind(connection.max_context_messages)
    .bind(connection.auto_context_on_model_change as i64)
    .execute(pool)
    .await?;
    Ok(())
}

async fn sync_active_connection_snapshot(pool: &SqlitePool) -> AppResult<()> {
    let active_id: Option<i64> =
        sqlx::query_scalar("SELECT active_inference_connection_id FROM app_settings WHERE id = 1")
            .fetch_one(pool)
            .await?;
    if let Some(id) = active_id {
        let connection = get_inference_connection(pool, id).await?;
        snapshot_active_connection_settings(pool, &connection).await?;
    }
    Ok(())
}

async fn update_active_connection_profile(
    pool: &SqlitePool,
    current: &mut Settings,
    update: InferenceConnectionUpdate,
) -> AppResult<()> {
    let Some(active_id) = current.active_connection_id else {
        return Ok(());
    };
    let updated = update_inference_connection(pool, active_id, update).await?;
    if let Some(conn) = current.connections.iter_mut().find(|c| c.id == active_id) {
        *conn = updated;
    }
    Ok(())
}

pub async fn update_settings(pool: &SqlitePool, payload: SettingsUpdate) -> AppResult<Settings> {
    let mut current = get_settings(pool).await?;
    let inference_url_updated = payload.inference_url.is_some();
    let model_updated = payload.model.is_some();
    let temperature_updated = payload.temperature.is_some();
    let top_p_updated = payload.top_p.is_some();
    let max_tokens_updated = payload.max_tokens.is_some();
    let max_context_messages_updated = payload.max_context_messages.is_some();
    let context_tokens_updated = payload.context_tokens.is_some();
    let auto_context_updated = payload.auto_context_on_model_change.is_some();

    // Apply connection switch before per-connection fields so autosave cannot write
    // profile fields from the previous connection onto the newly selected one.
    if let Some(v) = payload.active_connection_id {
        set_active_inference_connection(pool, v).await?;
        current.active_connection_id = Some(v);
        let fresh = get_inference_connection(pool, v).await?;
        if let Some(conn) = current.connections.iter_mut().find(|c| c.id == v) {
            *conn = fresh.clone();
        }
        if !inference_url_updated {
            current.inference_url = fresh.inference_url;
        }
        if !model_updated {
            current.model = fresh.model;
        }
        if !temperature_updated {
            current.temperature = fresh.temperature;
        }
        if !top_p_updated {
            current.top_p = fresh.top_p;
        }
        if !max_tokens_updated {
            current.max_tokens = fresh.max_tokens;
        }
        if !context_tokens_updated {
            current.context_tokens = fresh.context_tokens;
        }
        if !max_context_messages_updated {
            current.max_context_messages = fresh.max_context_messages;
        }
        if !auto_context_updated {
            current.auto_context_on_model_change = fresh.auto_context_on_model_change;
        }
    }

    if let Some(v) = payload.inference_url {
        current.inference_url = v.clone();
        if let Some(active_id) = current.active_connection_id {
            let updated = update_inference_connection(
                pool,
                active_id,
                InferenceConnectionUpdate {
                    inference_url: Some(v),
                    ..Default::default()
                },
            )
            .await?;
            if let Some(conn) = current.connections.iter_mut().find(|c| c.id == active_id) {
                *conn = updated;
            }
        }
    }
    if let Some(v) = payload.model {
        current.model = v.clone();
        if let Some(active_id) = current.active_connection_id {
            let updated = update_inference_connection(
                pool,
                active_id,
                InferenceConnectionUpdate {
                    model: Some(v),
                    ..Default::default()
                },
            )
            .await?;
            if let Some(conn) = current.connections.iter_mut().find(|c| c.id == active_id) {
                *conn = updated;
            }
        }
    }
    if let Some(v) = payload.temperature {
        current.temperature = v;
        update_active_connection_profile(
            pool,
            &mut current,
            InferenceConnectionUpdate {
                temperature: Some(v),
                ..Default::default()
            },
        )
        .await?;
    }
    if let Some(v) = payload.top_p {
        current.top_p = v;
        update_active_connection_profile(
            pool,
            &mut current,
            InferenceConnectionUpdate {
                top_p: Some(v),
                ..Default::default()
            },
        )
        .await?;
    }
    if let Some(v) = payload.max_tokens {
        current.max_tokens = v;
        update_active_connection_profile(
            pool,
            &mut current,
            InferenceConnectionUpdate {
                max_tokens: Some(v),
                ..Default::default()
            },
        )
        .await?;
    }
    if let Some(v) = payload.system_prompt_prefix {
        current.system_prompt_prefix = v;
    }
    if let Some(v) = payload.system_prompt_suffix {
        current.system_prompt_suffix = v;
    }
    if let Some(v) = payload.user_name {
        current.user_name = v;
    }
    if let Some(v) = payload.persona_description {
        current.persona_description = v;
    }
    if let Some(v) = payload.summarize_enabled {
        current.summarize_enabled = v;
    }
    if let Some(v) = payload.summarize_adaptive {
        current.summarize_adaptive = v;
    }
    if let Some(v) = payload.summarize_after_messages {
        current.summarize_after_messages = v;
    }
    if let Some(v) = payload.summarize_keep_recent {
        current.summarize_keep_recent = v;
    }
    if let Some(v) = payload.variables_enabled {
        current.variables_enabled = v;
    }
    if let Some(v) = payload.thought_blocks_enabled {
        current.thought_blocks_enabled = v;
    }
    if let Some(v) = payload.max_context_messages {
        current.max_context_messages = v;
        update_active_connection_profile(
            pool,
            &mut current,
            InferenceConnectionUpdate {
                max_context_messages: Some(v),
                ..Default::default()
            },
        )
        .await?;
    }
    if let Some(v) = payload.context_tokens {
        current.context_tokens = v.max(0);
        update_active_connection_profile(
            pool,
            &mut current,
            InferenceConnectionUpdate {
                context_tokens: Some(v.max(0)),
                ..Default::default()
            },
        )
        .await?;
    }
    if let Some(v) = payload.auto_context_on_model_change {
        current.auto_context_on_model_change = v;
        update_active_connection_profile(
            pool,
            &mut current,
            InferenceConnectionUpdate {
                auto_context_on_model_change: Some(v),
                ..Default::default()
            },
        )
        .await?;
    }
    if let Some(v) = payload.max_concurrent_jobs {
        MAX_CONCURRENT_JOBS.store(v.max(1), std::sync::atomic::Ordering::SeqCst);
        current.max_concurrent_jobs = v.max(1);
    }

    sqlx::query(
        "UPDATE app_settings SET inference_url=?1, model=?2, temperature=?3, top_p=?4, max_tokens=?5, system_prompt_prefix=?6, system_prompt_suffix=?7, user_name=?8, persona_description=?9, summarize_enabled=?10, summarize_adaptive=?11, summarize_after_messages=?12, summarize_keep_recent=?13, variables_enabled=?14, thought_blocks_enabled=?15, max_context_messages=?16, context_tokens=?17, auto_context_on_model_change=?18 WHERE id=1",
    )
    .bind(&current.inference_url)
    .bind(&current.model)
    .bind(current.temperature)
    .bind(current.top_p)
    .bind(current.max_tokens)
    .bind(&current.system_prompt_prefix)
    .bind(&current.system_prompt_suffix)
    .bind(&current.user_name)
    .bind(&current.persona_description)
    .bind(current.summarize_enabled as i64)
    .bind(current.summarize_adaptive as i64)
    .bind(current.summarize_after_messages)
    .bind(current.summarize_keep_recent)
    .bind(current.variables_enabled as i64)
    .bind(current.thought_blocks_enabled as i64)
    .bind(current.max_context_messages)
    .bind(current.context_tokens)
    .bind(current.auto_context_on_model_change as i64)
    .execute(pool)
    .await?;
    get_settings(pool).await
}

pub async fn has_active_summarize_job(pool: &SqlitePool, chat_id: i64) -> AppResult<bool> {
    let count: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM generation_jobs WHERE chat_id = ?1 AND job_type = 'chat_summarize' AND status IN ('queued','running')",
    )
    .bind(chat_id)
    .fetch_one(pool)
    .await?;
    Ok(count > 0)
}

pub async fn has_active_variable_recheck_job(
    pool: &SqlitePool,
    message_id: i64,
) -> AppResult<bool> {
    let count: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM generation_jobs WHERE message_id = ?1 AND job_type = 'chat_variable_recheck' AND status IN ('queued','running')",
    )
    .bind(message_id)
    .fetch_one(pool)
    .await?;
    Ok(count > 0)
}

pub async fn enqueue_variable_recheck_job(
    pool: &SqlitePool,
    chat_id: i64,
    message_id: i64,
) -> AppResult<Job> {
    let now = Utc::now().to_rfc3339();
    let position: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM generation_jobs WHERE status = 'queued'")
            .fetch_one(pool)
            .await?;
    let id = sqlx::query_scalar::<_, i64>(
        "INSERT INTO generation_jobs (job_type, chat_id, message_id, status, position, created_at) VALUES ('chat_variable_recheck',?1,?2,'queued',?3,?4) RETURNING id",
    )
    .bind(chat_id)
    .bind(message_id)
    .bind(position + 1)
    .bind(&now)
    .fetch_one(pool)
    .await?;
    get_job(pool, id).await
}

pub async fn enqueue_summarize_job(
    pool: &SqlitePool,
    chat_id: i64,
    message_id: i64,
    guidance_notes: &str,
) -> AppResult<Job> {
    let now = Utc::now().to_rfc3339();
    let position: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM generation_jobs WHERE status = 'queued'")
            .fetch_one(pool)
            .await?;
    let id = sqlx::query_scalar::<_, i64>(
        "INSERT INTO generation_jobs (job_type, chat_id, message_id, guidance_notes, status, position, created_at) VALUES ('chat_summarize',?1,?2,?3,'queued',?4,?5) RETURNING id",
    )
    .bind(chat_id)
    .bind(message_id)
    .bind(guidance_notes)
    .bind(position + 1)
    .bind(&now)
    .fetch_one(pool)
    .await?;
    get_job(pool, id).await
}

pub async fn set_message_created_at(
    pool: &SqlitePool,
    message_id: i64,
    created_at: &str,
) -> AppResult<()> {
    sqlx::query("UPDATE messages SET created_at = ?1 WHERE id = ?2")
        .bind(created_at)
        .bind(message_id)
        .execute(pool)
        .await?;
    Ok(())
}

pub async fn enqueue_job(pool: &SqlitePool, chat_id: i64, message_id: i64) -> AppResult<Job> {
    let now = Utc::now().to_rfc3339();
    let position: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM generation_jobs WHERE status = 'queued'")
            .fetch_one(pool)
            .await?;
    let id = sqlx::query_scalar::<_, i64>(
        "INSERT INTO generation_jobs (job_type, chat_id, message_id, status, position, created_at) VALUES ('chat_message',?1,?2,'queued',?3,?4) RETURNING id",
    )
    .bind(chat_id)
    .bind(message_id)
    .bind(position + 1)
    .bind(&now)
    .fetch_one(pool)
    .await?;
    get_job(pool, id).await
}

pub async fn get_job(pool: &SqlitePool, id: i64) -> AppResult<Job> {
    let row = sqlx::query_as::<_, JobRow>(
        "SELECT id, job_type, chat_id, message_id, story_id, chapter_id, beat_id, game_id, turn_id, guidance_notes, status, error, position, created_at, started_at, completed_at FROM generation_jobs WHERE id = ?1",
    )
    .bind(id)
    .fetch_optional(pool)
    .await?
    .ok_or_else(|| AppError::not_found("Job not found"))?;
    Ok(row.into())
}

pub async fn get_active_job(pool: &SqlitePool, chat_id: i64) -> AppResult<Option<Job>> {
    let row = sqlx::query_as::<_, JobRow>(
        "SELECT id, job_type, chat_id, message_id, story_id, chapter_id, beat_id, game_id, turn_id, guidance_notes, status, error, position, created_at, started_at, completed_at FROM generation_jobs WHERE chat_id = ?1 AND job_type IN ('chat_message', 'chat_summarize', 'chat_variable_recheck') AND status IN ('queued','running') ORDER BY created_at ASC LIMIT 1",
    )
    .bind(chat_id)
    .fetch_optional(pool)
    .await?;
    Ok(row.map(Into::into))
}

pub async fn list_queue(pool: &SqlitePool) -> AppResult<(Vec<Job>, Vec<Job>)> {
    let running = sqlx::query_as::<_, JobRow>(
        "SELECT id, job_type, chat_id, message_id, story_id, chapter_id, beat_id, game_id, turn_id, guidance_notes, status, error, position, created_at, started_at, completed_at FROM generation_jobs WHERE status = 'running' ORDER BY started_at ASC",
    )
    .fetch_all(pool)
    .await?
    .into_iter()
    .map(Into::into)
    .collect();
    let queued = sqlx::query_as::<_, JobRow>(
        "SELECT id, job_type, chat_id, message_id, story_id, chapter_id, beat_id, game_id, turn_id, guidance_notes, status, error, position, created_at, started_at, completed_at FROM generation_jobs WHERE status = 'queued' ORDER BY created_at ASC",
    )
    .fetch_all(pool)
    .await?
    .into_iter()
    .map(Into::into)
    .collect();
    Ok((running, queued))
}

pub async fn claim_jobs(pool: &SqlitePool, limit: i64) -> AppResult<Vec<i64>> {
    const MAX_ATTEMPTS: u32 = 12;
    for attempt in 1..=MAX_ATTEMPTS {
        match claim_jobs_once(pool, limit).await {
            Ok(ids) => return Ok(ids),
            Err(err) if is_sqlite_locked(&err) && attempt < MAX_ATTEMPTS => {
                tokio::time::sleep(Duration::from_millis(25 * u64::from(attempt))).await;
            }
            Err(err) => return Err(err.into()),
        }
    }
    Err(AppError::internal(
        "claim_jobs timed out waiting for database lock",
    ))
}

async fn claim_jobs_once(pool: &SqlitePool, limit: i64) -> Result<Vec<i64>, sqlx::Error> {
    let mut conn = pool.acquire().await?;
    sqlx::query("BEGIN IMMEDIATE").execute(&mut *conn).await?;
    let result = claim_jobs_in_tx(&mut conn, limit).await;
    if result.is_ok() {
        sqlx::query("COMMIT").execute(&mut *conn).await?;
    } else {
        let _ = sqlx::query("ROLLBACK").execute(&mut *conn).await;
    }
    result
}

async fn claim_jobs_in_tx(
    conn: &mut sqlx::sqlite::SqliteConnection,
    limit: i64,
) -> Result<Vec<i64>, sqlx::Error> {
    let running: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM generation_jobs WHERE status = 'running'")
            .fetch_one(&mut *conn)
            .await?;
    let max = MAX_CONCURRENT_JOBS.load(std::sync::atomic::Ordering::SeqCst);
    let slots = (max - running).max(0).min(limit);
    if slots == 0 {
        return Ok(vec![]);
    }
    let ids = sqlx::query_scalar::<_, i64>(
        "SELECT id FROM generation_jobs j
         WHERE j.status = 'queued'
         AND NOT (
           j.job_type IN ('story_chapter_outline', 'story_full_outline', 'story_propose_chapters', 'story_propose_beats')
           AND j.story_id IS NOT NULL
           AND EXISTS (
             SELECT 1 FROM generation_jobs r
             WHERE r.story_id = j.story_id
             AND r.status = 'running'
             AND r.job_type IN ('story_chapter_outline', 'story_full_outline', 'story_propose_chapters', 'story_propose_beats')
           )
         )
         ORDER BY j.created_at ASC LIMIT ?1",
    )
    .bind(slots)
    .fetch_all(&mut *conn)
    .await?;
    let now = Utc::now().to_rfc3339();
    for id in &ids {
        sqlx::query("UPDATE generation_jobs SET status='running', started_at=?1 WHERE id=?2")
            .bind(&now)
            .bind(id)
            .execute(&mut *conn)
            .await?;
    }
    Ok(ids)
}

pub async fn count_queued_jobs(pool: &SqlitePool) -> AppResult<i64> {
    Ok(
        sqlx::query_scalar("SELECT COUNT(*) FROM generation_jobs WHERE status = 'queued'")
            .fetch_one(pool)
            .await?,
    )
}

pub async fn requeue_stale_jobs(pool: &SqlitePool) -> AppResult<i64> {
    let result = sqlx::query(
        "UPDATE generation_jobs SET status = 'queued', started_at = NULL WHERE status = 'running'",
    )
    .execute(pool)
    .await?;
    Ok(result.rows_affected() as i64)
}

pub async fn requeue_stuck_jobs(pool: &SqlitePool, max_age_secs: i64) -> AppResult<i64> {
    let cutoff = (Utc::now() - chrono::Duration::seconds(max_age_secs)).to_rfc3339();
    let result = sqlx::query(
        "UPDATE generation_jobs SET status = 'queued', started_at = NULL \
         WHERE status = 'running' AND (started_at IS NULL OR started_at < ?1)",
    )
    .bind(&cutoff)
    .execute(pool)
    .await?;
    Ok(result.rows_affected() as i64)
}

pub async fn list_running_job_ids(pool: &SqlitePool) -> AppResult<Vec<i64>> {
    Ok(
        sqlx::query_scalar("SELECT id FROM generation_jobs WHERE status = 'running'")
            .fetch_all(pool)
            .await?,
    )
}

pub async fn requeue_jobs_by_id(pool: &SqlitePool, ids: &[i64]) -> AppResult<i64> {
    if ids.is_empty() {
        return Ok(0);
    }
    let mut requeued = 0_i64;
    for id in ids {
        let result = sqlx::query(
            "UPDATE generation_jobs SET status = 'queued', started_at = NULL \
             WHERE id = ?1 AND status = 'running'",
        )
        .bind(id)
        .execute(pool)
        .await?;
        requeued += result.rows_affected() as i64;
    }
    Ok(requeued)
}

pub async fn complete_job(
    pool: &SqlitePool,
    job_id: i64,
    status: JobStatus,
    error: Option<String>,
) -> AppResult<()> {
    let now = Utc::now().to_rfc3339();
    sqlx::query("UPDATE generation_jobs SET status=?1, error=?2, completed_at=?3 WHERE id=?4")
        .bind(job_status_str(status))
        .bind(error)
        .bind(&now)
        .bind(job_id)
        .execute(pool)
        .await?;
    Ok(())
}

pub async fn update_chat_summary(pool: &SqlitePool, chat_id: i64, summary: &str) -> AppResult<()> {
    sqlx::query("UPDATE chats SET summary = ?1 WHERE id = ?2")
        .bind(summary)
        .bind(chat_id)
        .execute(pool)
        .await?;
    Ok(())
}

pub async fn delete_messages(pool: &SqlitePool, ids: &[i64]) -> AppResult<()> {
    for id in ids {
        sqlx::query("DELETE FROM messages WHERE id = ?1")
            .bind(id)
            .execute(pool)
            .await?;
    }
    Ok(())
}

pub(crate) fn parse_dt(s: &str) -> AppResult<DateTime<Utc>> {
    DateTime::parse_from_rfc3339(s)
        .map(|dt| dt.with_timezone(&Utc))
        .or_else(|_| {
            chrono::NaiveDateTime::parse_from_str(s, "%Y-%m-%d %H:%M:%S").map(|ndt| ndt.and_utc())
        })
        .map_err(|e| AppError::internal(format!("invalid datetime: {e}")))
}

fn message_from_row(row: MessageRow) -> Message {
    let variable_updates = serde_json::from_str(&row.variable_updates).unwrap_or_default();
    Message {
        id: row.id,
        chat_id: row.chat_id,
        role: parse_role(&row.role),
        content: row.content,
        thought_content: row.thought_content,
        thought_duration_ms: row.thought_duration_ms,
        thought_in_progress: row.thought_in_progress != 0,
        variable_updates,
        reply_beats: parse_string_array(&row.reply_beats),
        state_changes: parse_applied_state_changes(&row.state_changes),
        generation_phase: row.generation_phase,
        is_summary: row.is_summary != 0,
        in_summary: row.in_summary != 0,
        created_at: DateTime::parse_from_rfc3339(&row.created_at)
            .map(|dt| dt.with_timezone(&Utc))
            .unwrap_or_else(|_| Utc::now()),
        job_status: row.job_status.map(|s| parse_job_status(&s)),
        generation_error: row.generation_error,
    }
}

#[derive(sqlx::FromRow)]
struct CharacterRow {
    id: i64,
    name: String,
    description: String,
    personality: String,
    scenario: String,
    first_message: String,
    example_dialogue: String,
    system_prompt: String,
    avatar_url: Option<String>,
    created_at: String,
    updated_at: String,
}

impl From<CharacterRow> for Character {
    fn from(row: CharacterRow) -> Self {
        Self {
            id: row.id,
            name: row.name,
            description: row.description,
            personality: row.personality,
            scenario: row.scenario,
            first_message: row.first_message,
            example_dialogue: row.example_dialogue,
            system_prompt: row.system_prompt,
            avatar_url: row.avatar_url,
            created_at: DateTime::parse_from_rfc3339(&row.created_at)
                .map(|dt| dt.with_timezone(&Utc))
                .unwrap_or_else(|_| Utc::now()),
            updated_at: DateTime::parse_from_rfc3339(&row.updated_at)
                .map(|dt| dt.with_timezone(&Utc))
                .unwrap_or_else(|_| Utc::now()),
        }
    }
}

#[derive(sqlx::FromRow)]
pub struct ChatRow {
    pub id: i64,
    pub title: String,
    pub character_id: i64,
    pub character_name: String,
    pub summary: String,
    pub created_at: String,
    pub updated_at: String,
    pub archived_at: Option<String>,
}

#[derive(sqlx::FromRow)]
struct MessageRow {
    id: i64,
    chat_id: i64,
    role: String,
    content: String,
    thought_content: String,
    thought_duration_ms: Option<i64>,
    thought_in_progress: i64,
    variable_updates: String,
    reply_beats: String,
    state_changes: String,
    generation_phase: String,
    is_summary: i64,
    in_summary: i64,
    created_at: String,
    job_status: Option<String>,
    generation_error: Option<String>,
}

#[derive(sqlx::FromRow)]
struct ChatActorRow {
    id: i64,
    chat_id: i64,
    role: String,
    name: String,
    description: String,
    skills: String,
    sort_order: i64,
    created_at: String,
    updated_at: String,
}

#[derive(sqlx::FromRow)]
struct ChatStateRow {
    id: i64,
    chat_id: i64,
    actor_id: Option<i64>,
    kind: String,
    key: String,
    value: String,
    num_value: Option<i64>,
    max_value: Option<i64>,
    source_message_id: i64,
    updated_at: String,
}

#[derive(sqlx::FromRow)]
struct VariableRow {
    id: i64,
    chat_id: i64,
    key: String,
    value: String,
    source_message_id: i64,
    updated_at: String,
}

impl From<VariableRow> for ChatVariable {
    fn from(row: VariableRow) -> Self {
        Self {
            id: row.id,
            chat_id: row.chat_id,
            key: row.key,
            value: row.value,
            source_message_id: row.source_message_id,
            updated_at: DateTime::parse_from_rfc3339(&row.updated_at)
                .map(|dt| dt.with_timezone(&Utc))
                .unwrap_or_else(|_| Utc::now()),
        }
    }
}

#[derive(sqlx::FromRow)]
pub(crate) struct JobRow {
    id: i64,
    job_type: String,
    chat_id: Option<i64>,
    message_id: Option<i64>,
    story_id: Option<i64>,
    chapter_id: Option<i64>,
    beat_id: Option<i64>,
    game_id: Option<i64>,
    turn_id: Option<i64>,
    guidance_notes: String,
    status: String,
    error: Option<String>,
    position: i64,
    created_at: String,
    started_at: Option<String>,
    completed_at: Option<String>,
}

impl From<JobRow> for Job {
    fn from(row: JobRow) -> Self {
        Self {
            id: row.id,
            job_type: parse_job_type(&row.job_type),
            chat_id: row.chat_id,
            message_id: row.message_id,
            story_id: row.story_id,
            chapter_id: row.chapter_id,
            beat_id: row.beat_id,
            game_id: row.game_id,
            turn_id: row.turn_id,
            guidance_notes: row.guidance_notes,
            status: parse_job_status(&row.status),
            error: row.error,
            position: row.position,
            created_at: DateTime::parse_from_rfc3339(&row.created_at)
                .map(|dt| dt.with_timezone(&Utc))
                .unwrap_or_else(|_| Utc::now()),
            started_at: row.started_at.and_then(|s| {
                DateTime::parse_from_rfc3339(&s)
                    .ok()
                    .map(|dt| dt.with_timezone(&Utc))
            }),
            completed_at: row.completed_at.and_then(|s| {
                DateTime::parse_from_rfc3339(&s)
                    .ok()
                    .map(|dt| dt.with_timezone(&Utc))
            }),
        }
    }
}

#[derive(sqlx::FromRow)]
struct SettingsRow {
    inference_url: String,
    active_inference_connection_id: Option<i64>,
    model: String,
    temperature: f64,
    top_p: f64,
    max_tokens: i64,
    system_prompt_prefix: String,
    system_prompt_suffix: String,
    user_name: String,
    persona_description: String,
    summarize_enabled: i64,
    summarize_adaptive: i64,
    summarize_after_messages: i64,
    summarize_keep_recent: i64,
    variables_enabled: i64,
    thought_blocks_enabled: i64,
    max_context_messages: i64,
    context_tokens: i64,
    auto_context_on_model_change: i64,
}

impl SettingsRow {
    fn into_settings(self, connections: Vec<InferenceConnection>) -> Settings {
        Settings {
            inference_url: self.inference_url,
            active_connection_id: self.active_inference_connection_id,
            connections,
            model: self.model,
            temperature: self.temperature,
            top_p: self.top_p,
            max_tokens: self.max_tokens,
            system_prompt_prefix: self.system_prompt_prefix,
            system_prompt_suffix: self.system_prompt_suffix,
            user_name: self.user_name,
            persona_description: self.persona_description,
            summarize_enabled: self.summarize_enabled != 0,
            summarize_adaptive: self.summarize_adaptive != 0,
            summarize_after_messages: self.summarize_after_messages,
            summarize_keep_recent: self.summarize_keep_recent,
            variables_enabled: self.variables_enabled != 0,
            thought_blocks_enabled: self.thought_blocks_enabled != 0,
            max_context_messages: self.max_context_messages,
            context_tokens: self.context_tokens,
            auto_context_on_model_change: self.auto_context_on_model_change != 0,
            max_concurrent_jobs: MAX_CONCURRENT_JOBS.load(std::sync::atomic::Ordering::SeqCst),
        }
    }
}

#[derive(sqlx::FromRow)]
struct InferenceConnectionRow {
    id: i64,
    name: String,
    inference_url: String,
    api_key: String,
    model: String,
    json_format_strategy: String,
    tool_call_parser: String,
    temperature: f64,
    top_p: f64,
    max_tokens: i64,
    context_tokens: i64,
    max_context_messages: i64,
    auto_context_on_model_change: i64,
}

impl InferenceConnectionRow {
    fn into_connection(self) -> InferenceConnection {
        InferenceConnection {
            id: self.id,
            name: self.name,
            inference_url: self.inference_url,
            api_key_set: !self.api_key.is_empty(),
            model: self.model,
            json_format_strategy: parse_json_format_strategy(&self.json_format_strategy),
            tool_call_parser: self.tool_call_parser,
            temperature: self.temperature,
            top_p: self.top_p,
            max_tokens: self.max_tokens,
            context_tokens: self.context_tokens,
            max_context_messages: self.max_context_messages,
            auto_context_on_model_change: self.auto_context_on_model_change != 0,
        }
    }
}

#[derive(sqlx::FromRow)]
struct ActiveConnectionRow {
    fallback_url: String,
    connection_id: Option<i64>,
    connection_url: Option<String>,
    api_key: Option<String>,
    json_format_strategy: String,
    tool_call_parser: String,
}

#[cfg(test)]
mod chat_update_tests {
    use super::*;
    use dreamwell_types::ChatUpdate;
    use sqlx::SqlitePool;

    async fn test_pool() -> SqlitePool {
        let pool = SqlitePool::connect("sqlite::memory:").await.expect("pool");
        sqlx::migrate!("./migrations")
            .run(&pool)
            .await
            .expect("migrate");
        ensure_settings(&pool).await.expect("settings");
        pool
    }

    #[tokio::test]
    async fn update_chat_title_remains_in_active_list() {
        let pool = test_pool().await;
        let character_id = sqlx::query_scalar::<_, i64>(
            "INSERT INTO characters (name, description, personality, scenario, first_message, example_dialogue, system_prompt, created_at, updated_at) VALUES ('c','','','','','','',datetime('now'),datetime('now')) RETURNING id",
        )
        .fetch_one(&pool)
        .await
        .expect("character");
        let chat = create_chat(&pool, "Before rename".into(), character_id)
            .await
            .expect("chat");

        let updated = update_chat(
            &pool,
            chat.id,
            ChatUpdate {
                title: Some("After rename".into()),
                character_id: None,
                summary: None,
            },
        )
        .await
        .expect("update");

        assert_eq!(updated.title, "After rename");

        let list = list_chats(&pool).await.expect("list");
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].id, chat.id);
        assert_eq!(list[0].title, "After rename");
    }
}

#[cfg(test)]
mod generation_failure_tests {
    use super::*;
    use sqlx::SqlitePool;

    async fn test_pool() -> SqlitePool {
        let pool = SqlitePool::connect("sqlite::memory:").await.expect("pool");
        sqlx::migrate!("./migrations")
            .run(&pool)
            .await
            .expect("migrate");
        ensure_settings(&pool).await.expect("settings");
        pool
    }

    #[tokio::test]
    async fn fail_chat_message_preserves_partial_content() {
        let pool = test_pool().await;
        let character_id = sqlx::query_scalar::<_, i64>(
            "INSERT INTO characters (name, description, personality, scenario, first_message, example_dialogue, system_prompt, created_at, updated_at) VALUES ('c','','','','','','',datetime('now'),datetime('now')) RETURNING id",
        )
        .fetch_one(&pool)
        .await
        .expect("character");
        let chat_id = sqlx::query_scalar::<_, i64>(
            "INSERT INTO chats (title, character_id, summary, created_at, updated_at) VALUES ('t', ?1, '', datetime('now'), datetime('now')) RETURNING id",
        )
        .bind(character_id)
        .fetch_one(&pool)
        .await
        .expect("chat");
        let message_id = sqlx::query_scalar::<_, i64>(
            "INSERT INTO messages (chat_id, role, content, is_summary, created_at) VALUES (?1, 'assistant', 'Partial reply', 0, datetime('now')) RETURNING id",
        )
        .bind(chat_id)
        .fetch_one(&pool)
        .await
        .expect("message");

        fail_chat_message_generation(&pool, message_id, "connection reset")
            .await
            .expect("fail");

        let message = get_message(&pool, chat_id, message_id)
            .await
            .expect("message");
        assert_eq!(message.content, "Partial reply");
    }

    #[tokio::test]
    async fn fail_chat_message_writes_placeholder_when_empty() {
        let pool = test_pool().await;
        let character_id = sqlx::query_scalar::<_, i64>(
            "INSERT INTO characters (name, description, personality, scenario, first_message, example_dialogue, system_prompt, created_at, updated_at) VALUES ('c','','','','','','',datetime('now'),datetime('now')) RETURNING id",
        )
        .fetch_one(&pool)
        .await
        .expect("character");
        let chat_id = sqlx::query_scalar::<_, i64>(
            "INSERT INTO chats (title, character_id, summary, created_at, updated_at) VALUES ('t', ?1, '', datetime('now'), datetime('now')) RETURNING id",
        )
        .bind(character_id)
        .fetch_one(&pool)
        .await
        .expect("chat");
        let message_id = sqlx::query_scalar::<_, i64>(
            "INSERT INTO messages (chat_id, role, content, is_summary, created_at) VALUES (?1, 'assistant', '', 0, datetime('now')) RETURNING id",
        )
        .bind(chat_id)
        .fetch_one(&pool)
        .await
        .expect("message");

        fail_chat_message_generation(&pool, message_id, "timeout")
            .await
            .expect("fail");

        let message = get_message(&pool, chat_id, message_id)
            .await
            .expect("message");
        assert_eq!(message.content, "[Generation failed: timeout]");
    }
}

#[cfg(test)]
mod settings_update_tests {
    use super::*;
    use dreamwell_types::{InferenceConnectionCreate, SettingsUpdate};
    use sqlx::SqlitePool;

    async fn test_pool() -> SqlitePool {
        let pool = SqlitePool::connect("sqlite::memory:").await.expect("pool");
        sqlx::migrate!("./migrations")
            .run(&pool)
            .await
            .expect("migrate");
        ensure_settings(&pool).await.expect("settings");
        pool
    }

    async fn test_pool_with_connection() -> (SqlitePool, i64) {
        let pool = test_pool().await;
        let conn = create_inference_connection(
            &pool,
            InferenceConnectionCreate {
                name: "Default".into(),
                inference_url: "http://localhost:11434/v1".into(),
                api_key: None,
            },
        )
        .await
        .expect("connection");
        update_settings(
            &pool,
            SettingsUpdate {
                active_connection_id: Some(conn.id),
                ..Default::default()
            },
        )
        .await
        .expect("activate");
        (pool, conn.id)
    }

    #[tokio::test]
    async fn update_settings_preserves_inference_url_with_active_connection() {
        let (pool, active_id) = test_pool_with_connection().await;
        let custom_url = "http://custom.example/v1";

        let updated = update_settings(
            &pool,
            SettingsUpdate {
                inference_url: Some(custom_url.into()),
                active_connection_id: Some(active_id),
                ..Default::default()
            },
        )
        .await
        .expect("update");

        assert_eq!(updated.inference_url, custom_url);

        let fallback: String =
            sqlx::query_scalar("SELECT inference_url FROM app_settings WHERE id = 1")
                .fetch_one(&pool)
                .await
                .expect("fallback url");
        assert_eq!(fallback, custom_url);

        let conn = get_inference_connection(&pool, active_id)
            .await
            .expect("connection");
        assert_eq!(conn.inference_url, custom_url);
    }

    #[tokio::test]
    async fn update_settings_switching_connection_does_not_corrupt_other_profiles() {
        let (pool, first_id) = test_pool_with_connection().await;
        let second = create_inference_connection(
            &pool,
            InferenceConnectionCreate {
                name: "Hosted".into(),
                inference_url: "https://api.featherlight.ai/v1".into(),
                api_key: None,
            },
        )
        .await
        .expect("second connection");

        let first_url = "http://localhost:11434/v1";
        let second_url = "https://api.featherlight.ai/v1";

        // Simulate autosave payload when switching from first → second in the UI.
        let updated = update_settings(
            &pool,
            SettingsUpdate {
                inference_url: Some(second_url.into()),
                active_connection_id: Some(second.id),
                ..Default::default()
            },
        )
        .await
        .expect("switch");

        assert_eq!(updated.active_connection_id, Some(second.id));
        assert_eq!(updated.inference_url, second_url);

        let first = get_inference_connection(&pool, first_id)
            .await
            .expect("first");
        let second_saved = get_inference_connection(&pool, second.id)
            .await
            .expect("second");
        assert_eq!(first.inference_url, first_url);
        assert_eq!(second_saved.inference_url, second_url);
    }

    #[tokio::test]
    async fn update_settings_switching_connection_preserves_per_profile_models() {
        let (pool, first_id) = test_pool_with_connection().await;
        update_inference_connection(
            &pool,
            first_id,
            InferenceConnectionUpdate {
                model: Some("local-model".into()),
                ..Default::default()
            },
        )
        .await
        .expect("first model");

        let second = create_inference_connection(
            &pool,
            InferenceConnectionCreate {
                name: "Hosted".into(),
                inference_url: "https://api.featherlight.ai/v1".into(),
                api_key: None,
            },
        )
        .await
        .expect("second connection");
        update_inference_connection(
            &pool,
            second.id,
            InferenceConnectionUpdate {
                model: Some("hosted-model".into()),
                ..Default::default()
            },
        )
        .await
        .expect("second model");

        let on_second = update_settings(
            &pool,
            SettingsUpdate {
                active_connection_id: Some(second.id),
                ..Default::default()
            },
        )
        .await
        .expect("switch to second");
        assert_eq!(on_second.model, "hosted-model");

        let on_first = update_settings(
            &pool,
            SettingsUpdate {
                active_connection_id: Some(first_id),
                ..Default::default()
            },
        )
        .await
        .expect("switch to first");
        assert_eq!(on_first.model, "local-model");

        let first = get_inference_connection(&pool, first_id)
            .await
            .expect("first");
        let second_saved = get_inference_connection(&pool, second.id)
            .await
            .expect("second");
        assert_eq!(first.model, "local-model");
        assert_eq!(second_saved.model, "hosted-model");
    }

    #[tokio::test]
    async fn update_settings_switching_connection_preserves_per_profile_generation_defaults() {
        let (pool, first_id) = test_pool_with_connection().await;
        update_inference_connection(
            &pool,
            first_id,
            InferenceConnectionUpdate {
                temperature: Some(0.3),
                top_p: Some(0.7),
                context_tokens: Some(4096),
                max_tokens: Some(1024),
                ..Default::default()
            },
        )
        .await
        .expect("first profile");

        let second = create_inference_connection(
            &pool,
            InferenceConnectionCreate {
                name: "Hosted".into(),
                inference_url: "https://api.featherlight.ai/v1".into(),
                api_key: None,
            },
        )
        .await
        .expect("second connection");
        update_inference_connection(
            &pool,
            second.id,
            InferenceConnectionUpdate {
                temperature: Some(1.1),
                top_p: Some(0.95),
                context_tokens: Some(16384),
                max_tokens: Some(2048),
                ..Default::default()
            },
        )
        .await
        .expect("second profile");

        let on_second = update_settings(
            &pool,
            SettingsUpdate {
                active_connection_id: Some(second.id),
                ..Default::default()
            },
        )
        .await
        .expect("switch to second");
        assert_eq!(on_second.temperature, 1.1);
        assert_eq!(on_second.top_p, 0.95);
        assert_eq!(on_second.context_tokens, 16384);
        assert_eq!(on_second.max_tokens, 2048);

        let on_first = update_settings(
            &pool,
            SettingsUpdate {
                active_connection_id: Some(first_id),
                ..Default::default()
            },
        )
        .await
        .expect("switch to first");
        assert_eq!(on_first.temperature, 0.3);
        assert_eq!(on_first.top_p, 0.7);
        assert_eq!(on_first.context_tokens, 4096);
        assert_eq!(on_first.max_tokens, 1024);
    }

    #[tokio::test]
    async fn update_settings_switching_connection_does_not_corrupt_other_profile_model() {
        let (pool, first_id) = test_pool_with_connection().await;
        update_inference_connection(
            &pool,
            first_id,
            InferenceConnectionUpdate {
                model: Some("local-model".into()),
                ..Default::default()
            },
        )
        .await
        .expect("first model");

        let second = create_inference_connection(
            &pool,
            InferenceConnectionCreate {
                name: "Hosted".into(),
                inference_url: "https://api.featherlight.ai/v1".into(),
                api_key: None,
            },
        )
        .await
        .expect("second connection");
        update_inference_connection(
            &pool,
            second.id,
            InferenceConnectionUpdate {
                model: Some("hosted-model".into()),
                ..Default::default()
            },
        )
        .await
        .expect("second model");

        let updated = update_settings(
            &pool,
            SettingsUpdate {
                model: Some("hosted-model".into()),
                active_connection_id: Some(second.id),
                ..Default::default()
            },
        )
        .await
        .expect("switch with model");

        assert_eq!(updated.model, "hosted-model");

        let first = get_inference_connection(&pool, first_id)
            .await
            .expect("first");
        assert_eq!(first.model, "local-model");
    }
}

#[cfg(test)]
mod generation_merge_tests {
    use super::*;

    #[test]
    fn merge_keeps_nonempty_fields_when_incoming_empty() {
        let current = MessageGenerationSnapshot {
            content: "Full reply".into(),
            thought_content: "reasoning".into(),
            thought_duration_ms: Some(1000),
            thought_in_progress: false,
        };
        let (content, thought, duration, in_progress) =
            merge_generation_fields(&current, "", "", None, true);
        assert_eq!(content, "Full reply");
        assert_eq!(thought, "reasoning");
        assert_eq!(duration, Some(1000));
        assert!(!in_progress);
    }

    #[test]
    fn merge_allows_writing_into_empty_message() {
        let current = MessageGenerationSnapshot {
            content: String::new(),
            thought_content: String::new(),
            thought_duration_ms: None,
            thought_in_progress: true,
        };
        let (content, thought, duration, in_progress) =
            merge_generation_fields(&current, "Hello", "plan", Some(500), false);
        assert_eq!(content, "Hello");
        assert_eq!(thought, "plan");
        assert_eq!(duration, Some(500));
        assert!(!in_progress);
    }

    #[test]
    fn merge_allows_overwriting_with_new_content() {
        let current = MessageGenerationSnapshot {
            content: "Old".into(),
            thought_content: String::new(),
            thought_duration_ms: None,
            thought_in_progress: false,
        };
        let (content, thought, _, _) =
            merge_generation_fields(&current, "New reply", "", None, false);
        assert_eq!(content, "New reply");
        assert!(thought.is_empty());
    }

    #[test]
    fn merge_preserves_in_progress_when_keeping_thought_during_stream() {
        let current = MessageGenerationSnapshot {
            content: String::new(),
            thought_content: "reasoning".into(),
            thought_duration_ms: None,
            thought_in_progress: true,
        };
        let (_, thought, _, in_progress) = merge_generation_fields(&current, "", "", None, true);
        assert_eq!(thought, "reasoning");
        assert!(in_progress);
    }
}
