use chrono::{DateTime, Utc};
use dreamwell_types::{
    Character, CharacterCreate, CharacterUpdate, Chat, ChatUpdate, ChatVariable, Job, JobStatus,
    JobType, Message, MessageRole, Settings, SettingsUpdate, CHAT_ARCHIVE_RETENTION_DAYS,
    DEFAULT_SYSTEM_PROMPT_PREFIX, DEFAULT_USER_NAME,
};
use std::time::Duration;

#[path = "stories_db.rs"]
mod stories_db;
use sqlx::{sqlite::SqliteConnectOptions, SqlitePool};
pub use stories_db::*;

use crate::config::MAX_CONCURRENT_JOBS;
use crate::error::{AppError, AppResult};

pub async fn connect(database_url: &str) -> AppResult<SqlitePool> {
    let url = database_url.strip_prefix("sqlite:").unwrap_or(database_url);
    let options = SqliteConnectOptions::new()
        .filename(url)
        .create_if_missing(true)
        .journal_mode(sqlx::sqlite::SqliteJournalMode::Wal)
        .synchronous(sqlx::sqlite::SqliteSynchronous::Normal)
        .busy_timeout(Duration::from_secs(10));
    let pool = SqlitePool::connect_with(options).await?;
    sqlx::migrate!("./migrations").run(&pool).await?;
    ensure_settings(&pool).await?;
    prepare_on_startup(&pool).await?;
    Ok(pool)
}

/// Release stale WAL locks and normalize journal state after an unclean shutdown.
pub async fn prepare_on_startup(pool: &SqlitePool) -> AppResult<()> {
    sqlx::query("PRAGMA wal_checkpoint(TRUNCATE)")
        .execute(pool)
        .await?;
    purge_expired_archived_chats(pool).await?;
    Ok(())
}

async fn ensure_settings(pool: &SqlitePool) -> AppResult<()> {
    sqlx::query(
        "INSERT OR IGNORE INTO app_settings (id, system_prompt_prefix, user_name) VALUES (1, ?1, ?2)",
    )
    .bind(DEFAULT_SYSTEM_PROMPT_PREFIX)
    .bind(DEFAULT_USER_NAME)
    .execute(pool)
    .await?;
    Ok(())
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
        _ => JobType::ChatMessage,
    }
}

pub(crate) fn job_type_str(job_type: JobType) -> &'static str {
    match job_type {
        JobType::ChatMessage => "chat_message",
        JobType::StoryChapterOutline => "story_chapter_outline",
        JobType::StoryProposeChapters => "story_propose_chapters",
        JobType::StoryBeatOutline => "story_beat_outline",
        JobType::StoryProposeBeats => "story_propose_beats",
        JobType::StoryBeatProse => "story_beat_prose",
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
         ORDER BY c.updated_at DESC",
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
         ORDER BY c.archived_at DESC",
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
            "SELECT COUNT(*) FROM generation_jobs WHERE chat_id = ?1 AND job_type = 'chat_message' AND status = 'queued'",
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
    get_chat(pool, id).await
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
    let _ = get_character(pool, character_id).await?;
    let now = Utc::now().to_rfc3339();
    sqlx::query("UPDATE chats SET title=?1, character_id=?2, updated_at=?3 WHERE id=?4")
        .bind(&title)
        .bind(character_id)
        .bind(&now)
        .bind(id)
        .execute(pool)
        .await?;

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
        "SELECT id, job_type, chat_id, message_id, story_id, chapter_id, beat_id, guidance_notes, status, error, position, created_at, started_at, completed_at FROM generation_jobs WHERE chat_id = ?1 AND job_type = 'chat_message' AND status IN ('queued','running') ORDER BY created_at ASC",
    )
    .bind(chat_id)
    .fetch_all(pool)
    .await?;
    Ok(rows.into_iter().map(Into::into).collect())
}

pub async fn list_messages(pool: &SqlitePool, chat_id: i64) -> AppResult<Vec<Message>> {
    let _ = get_chat(pool, chat_id).await?;
    let rows = sqlx::query_as::<_, MessageRow>(
        "SELECT m.id, m.chat_id, m.role, m.content, m.thought_content, m.thought_duration_ms, m.thought_in_progress, m.variable_updates, m.is_summary, m.created_at, j.status as job_status FROM messages m LEFT JOIN generation_jobs j ON j.id = (SELECT id FROM generation_jobs WHERE message_id = m.id AND status IN ('queued','running') ORDER BY created_at DESC LIMIT 1) WHERE m.chat_id = ?1 ORDER BY m.created_at ASC",
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
        "SELECT id, job_type, chat_id, message_id, story_id, chapter_id, beat_id, guidance_notes, status, error, position, created_at, started_at, completed_at FROM generation_jobs WHERE message_id = ?1 AND status IN ('queued','running') ORDER BY created_at ASC",
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
        is_summary,
        created_at: parse_dt(&now)?,
        job_status: None,
    })
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

pub async fn update_message_generation(
    pool: &SqlitePool,
    message_id: i64,
    content: &str,
    thought_content: &str,
    thought_duration_ms: Option<i64>,
    thought_in_progress: bool,
) -> AppResult<()> {
    sqlx::query(
        "UPDATE messages SET content = ?1, thought_content = ?2, thought_duration_ms = ?3, thought_in_progress = ?4 WHERE id = ?5",
    )
    .bind(content)
    .bind(thought_content)
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
    let variable_updates_json = serde_json::to_string(variable_updates)
        .map_err(|e| AppError::internal(format!("serialize variable updates: {e}")))?;
    sqlx::query(
        "UPDATE messages SET content = ?1, thought_content = ?2, thought_duration_ms = ?3, thought_in_progress = ?4, variable_updates = ?5 WHERE id = ?6",
    )
    .bind(content)
    .bind(thought_content)
    .bind(thought_duration_ms)
    .bind(thought_in_progress as i64)
    .bind(variable_updates_json)
    .bind(message_id)
    .execute(pool)
    .await?;
    Ok(())
}

pub async fn get_variable_value(
    pool: &SqlitePool,
    chat_id: i64,
    key: &str,
) -> AppResult<Option<String>> {
    let row = sqlx::query_scalar::<_, String>(
        "SELECT value FROM chat_variables WHERE chat_id = ?1 AND key = ?2",
    )
    .bind(chat_id)
    .bind(key)
    .fetch_optional(pool)
    .await?;
    Ok(row)
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
    let rows = sqlx::query_as::<_, VariableRow>(
        "SELECT id, chat_id, key, value, updated_at FROM chat_variables WHERE chat_id = ?1 ORDER BY key ASC",
    )
    .bind(chat_id)
    .fetch_all(pool)
    .await?;
    Ok(rows.into_iter().map(Into::into).collect())
}

pub async fn upsert_variable(
    pool: &SqlitePool,
    chat_id: i64,
    key: String,
    value: String,
) -> AppResult<ChatVariable> {
    let now = Utc::now().to_rfc3339();
    sqlx::query(
        "INSERT INTO chat_variables (chat_id, key, value, updated_at) VALUES (?1,?2,?3,?4) ON CONFLICT(chat_id, key) DO UPDATE SET value=excluded.value, updated_at=excluded.updated_at",
    )
    .bind(chat_id)
    .bind(&key)
    .bind(&value)
    .bind(&now)
    .execute(pool)
    .await?;
    let row = sqlx::query_as::<_, VariableRow>(
        "SELECT id, chat_id, key, value, updated_at FROM chat_variables WHERE chat_id = ?1 AND key = ?2",
    )
    .bind(chat_id)
    .bind(&key)
    .fetch_one(pool)
    .await?;
    Ok(row.into())
}

pub async fn delete_variable(pool: &SqlitePool, chat_id: i64, key: &str) -> AppResult<()> {
    let result = sqlx::query("DELETE FROM chat_variables WHERE chat_id = ?1 AND key = ?2")
        .bind(chat_id)
        .bind(key)
        .execute(pool)
        .await?;
    if result.rows_affected() == 0 {
        return Err(AppError::not_found("Variable not found"));
    }
    Ok(())
}

pub async fn get_settings(pool: &SqlitePool) -> AppResult<Settings> {
    let row = sqlx::query_as::<_, SettingsRow>(
        "SELECT inference_url, model, temperature, top_p, max_tokens, system_prompt_prefix, system_prompt_suffix, user_name, persona_description, summarize_enabled, summarize_after_messages, summarize_keep_recent, variables_enabled, thought_blocks_enabled, max_context_messages FROM app_settings WHERE id = 1",
    )
    .fetch_one(pool)
    .await?;
    Ok(row.into_settings())
}

pub async fn update_settings(pool: &SqlitePool, payload: SettingsUpdate) -> AppResult<Settings> {
    let mut current = get_settings(pool).await?;
    if let Some(v) = payload.inference_url {
        current.inference_url = v;
    }
    if let Some(v) = payload.model {
        current.model = v;
    }
    if let Some(v) = payload.temperature {
        current.temperature = v;
    }
    if let Some(v) = payload.top_p {
        current.top_p = v;
    }
    if let Some(v) = payload.max_tokens {
        current.max_tokens = v;
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
    }
    if let Some(v) = payload.max_concurrent_jobs {
        MAX_CONCURRENT_JOBS.store(v.max(1), std::sync::atomic::Ordering::SeqCst);
        current.max_concurrent_jobs = v.max(1);
    }

    sqlx::query(
        "UPDATE app_settings SET inference_url=?1, model=?2, temperature=?3, top_p=?4, max_tokens=?5, system_prompt_prefix=?6, system_prompt_suffix=?7, user_name=?8, persona_description=?9, summarize_enabled=?10, summarize_after_messages=?11, summarize_keep_recent=?12, variables_enabled=?13, thought_blocks_enabled=?14, max_context_messages=?15 WHERE id=1",
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
    .bind(current.summarize_after_messages)
    .bind(current.summarize_keep_recent)
    .bind(current.variables_enabled as i64)
    .bind(current.thought_blocks_enabled as i64)
    .bind(current.max_context_messages)
    .execute(pool)
    .await?;
    get_settings(pool).await
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
        "SELECT id, job_type, chat_id, message_id, story_id, chapter_id, beat_id, guidance_notes, status, error, position, created_at, started_at, completed_at FROM generation_jobs WHERE id = ?1",
    )
    .bind(id)
    .fetch_optional(pool)
    .await?
    .ok_or_else(|| AppError::not_found("Job not found"))?;
    Ok(row.into())
}

pub async fn get_active_job(pool: &SqlitePool, chat_id: i64) -> AppResult<Option<Job>> {
    let row = sqlx::query_as::<_, JobRow>(
        "SELECT id, job_type, chat_id, message_id, story_id, chapter_id, beat_id, guidance_notes, status, error, position, created_at, started_at, completed_at FROM generation_jobs WHERE chat_id = ?1 AND job_type = 'chat_message' AND status IN ('queued','running') ORDER BY created_at ASC LIMIT 1",
    )
    .bind(chat_id)
    .fetch_optional(pool)
    .await?;
    Ok(row.map(Into::into))
}

pub async fn list_queue(pool: &SqlitePool) -> AppResult<(Vec<Job>, Vec<Job>)> {
    let running = sqlx::query_as::<_, JobRow>(
        "SELECT id, job_type, chat_id, message_id, story_id, chapter_id, beat_id, guidance_notes, status, error, position, created_at, started_at, completed_at FROM generation_jobs WHERE status = 'running' ORDER BY started_at ASC",
    )
    .fetch_all(pool)
    .await?
    .into_iter()
    .map(Into::into)
    .collect();
    let queued = sqlx::query_as::<_, JobRow>(
        "SELECT id, job_type, chat_id, message_id, story_id, chapter_id, beat_id, guidance_notes, status, error, position, created_at, started_at, completed_at FROM generation_jobs WHERE status = 'queued' ORDER BY created_at ASC",
    )
    .fetch_all(pool)
    .await?
    .into_iter()
    .map(Into::into)
    .collect();
    Ok((running, queued))
}

pub async fn claim_jobs(pool: &SqlitePool, limit: i64) -> AppResult<Vec<i64>> {
    let mut tx = pool.begin().await?;
    let running: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM generation_jobs WHERE status = 'running'")
            .fetch_one(&mut *tx)
            .await?;
    let max = MAX_CONCURRENT_JOBS.load(std::sync::atomic::Ordering::SeqCst);
    let slots = (max - running).max(0).min(limit);
    if slots == 0 {
        tx.commit().await?;
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
    .fetch_all(&mut *tx)
    .await?;
    let now = Utc::now().to_rfc3339();
    for id in &ids {
        sqlx::query("UPDATE generation_jobs SET status='running', started_at=?1 WHERE id=?2")
            .bind(&now)
            .bind(id)
            .execute(&mut *tx)
            .await?;
    }
    tx.commit().await?;
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
        is_summary: row.is_summary != 0,
        created_at: DateTime::parse_from_rfc3339(&row.created_at)
            .map(|dt| dt.with_timezone(&Utc))
            .unwrap_or_else(|_| Utc::now()),
        job_status: row.job_status.map(|s| parse_job_status(&s)),
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
    is_summary: i64,
    created_at: String,
    job_status: Option<String>,
}

#[derive(sqlx::FromRow)]
struct VariableRow {
    id: i64,
    chat_id: i64,
    key: String,
    value: String,
    updated_at: String,
}

impl From<VariableRow> for ChatVariable {
    fn from(row: VariableRow) -> Self {
        Self {
            id: row.id,
            chat_id: row.chat_id,
            key: row.key,
            value: row.value,
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
    model: String,
    temperature: f64,
    top_p: f64,
    max_tokens: i64,
    system_prompt_prefix: String,
    system_prompt_suffix: String,
    user_name: String,
    persona_description: String,
    summarize_enabled: i64,
    summarize_after_messages: i64,
    summarize_keep_recent: i64,
    variables_enabled: i64,
    thought_blocks_enabled: i64,
    max_context_messages: i64,
}

impl SettingsRow {
    fn into_settings(self) -> Settings {
        Settings {
            inference_url: self.inference_url,
            model: self.model,
            temperature: self.temperature,
            top_p: self.top_p,
            max_tokens: self.max_tokens,
            system_prompt_prefix: self.system_prompt_prefix,
            system_prompt_suffix: self.system_prompt_suffix,
            user_name: self.user_name,
            persona_description: self.persona_description,
            summarize_enabled: self.summarize_enabled != 0,
            summarize_after_messages: self.summarize_after_messages,
            summarize_keep_recent: self.summarize_keep_recent,
            variables_enabled: self.variables_enabled != 0,
            thought_blocks_enabled: self.thought_blocks_enabled != 0,
            max_context_messages: self.max_context_messages,
            max_concurrent_jobs: MAX_CONCURRENT_JOBS.load(std::sync::atomic::Ordering::SeqCst),
        }
    }
}
