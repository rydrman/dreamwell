use chrono::{DateTime, Utc};
use dreamwell_types::{
    Character, CharacterCreate, CharacterUpdate, Chat, ChatUpdate, Fact, Job, JobStatus, Message,
    MessageRole, Settings, SettingsUpdate,
};
use sqlx::{sqlite::SqliteConnectOptions, SqlitePool};

use crate::config::MAX_CONCURRENT_JOBS;
use crate::error::{AppError, AppResult};

pub async fn connect(database_url: &str) -> AppResult<SqlitePool> {
    let url = database_url.strip_prefix("sqlite:").unwrap_or(database_url);
    let options = SqliteConnectOptions::new()
        .filename(url)
        .create_if_missing(true);
    let pool = SqlitePool::connect_with(options).await?;
    sqlx::migrate!("./migrations").run(&pool).await?;
    ensure_settings(&pool).await?;
    Ok(pool)
}

async fn ensure_settings(pool: &SqlitePool) -> AppResult<()> {
    sqlx::query("INSERT OR IGNORE INTO app_settings (id) VALUES (1)")
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

fn parse_job_status(s: &str) -> JobStatus {
    match s {
        "running" => JobStatus::Running,
        "completed" => JobStatus::Completed,
        "failed" => JobStatus::Failed,
        "cancelled" => JobStatus::Cancelled,
        _ => JobStatus::Queued,
    }
}

fn job_status_str(status: JobStatus) -> &'static str {
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
    let result = sqlx::query("DELETE FROM characters WHERE id = ?1")
        .bind(id)
        .execute(pool)
        .await?;
    if result.rows_affected() == 0 {
        return Err(AppError::not_found("Character not found"));
    }
    Ok(())
}

pub async fn list_chats(pool: &SqlitePool) -> AppResult<Vec<Chat>> {
    let rows = sqlx::query_as::<_, ChatRow>(
        "SELECT id, title, character_id, summary, created_at, updated_at FROM chats ORDER BY updated_at DESC",
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
    let row = sqlx::query_as::<_, ChatRow>(
        "SELECT id, title, character_id, summary, created_at, updated_at FROM chats WHERE id = ?1",
    )
    .bind(id)
    .fetch_optional(pool)
    .await?
    .ok_or_else(|| AppError::not_found("Chat not found"))?;
    chat_from_row(pool, row).await
}

async fn chat_from_row(pool: &SqlitePool, row: ChatRow) -> AppResult<Chat> {
    let active_job = get_active_job(pool, row.id).await?;
    let queued_jobs: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM generation_jobs WHERE chat_id = ?1 AND status = 'queued'",
    )
    .bind(row.id)
    .fetch_one(pool)
    .await?;
    Ok(Chat {
        id: row.id,
        title: row.title,
        character_id: row.character_id,
        summary: row.summary,
        created_at: parse_dt(&row.created_at)?,
        updated_at: parse_dt(&row.updated_at)?,
        active_job,
        queued_jobs,
    })
}

pub async fn create_chat(
    pool: &SqlitePool,
    title: String,
    character_id: Option<i64>,
) -> AppResult<Chat> {
    if let Some(cid) = character_id {
        let _ = get_character(pool, cid).await?;
    }
    let now = Utc::now().to_rfc3339();
    let id = sqlx::query_scalar::<_, i64>(
        "INSERT INTO chats (title, character_id, summary, created_at, updated_at) VALUES (?1,?2,'',?3,?3) RETURNING id",
    )
    .bind(&title)
    .bind(character_id)
    .bind(&now)
    .fetch_one(pool)
    .await?;

    if let Some(cid) = character_id {
        let character = get_character(pool, cid).await?;
        if !character.first_message.trim().is_empty() {
            insert_message(
                pool,
                id,
                MessageRole::Assistant,
                character.first_message.trim().to_string(),
                false,
            )
            .await?;
        }
    }
    get_chat(pool, id).await
}

pub async fn update_chat(pool: &SqlitePool, id: i64, payload: ChatUpdate) -> AppResult<Chat> {
    let existing = get_chat(pool, id).await?;
    let title = payload.title.unwrap_or(existing.title);
    let character_id = payload.character_id.or(existing.character_id);
    let now = Utc::now().to_rfc3339();
    sqlx::query("UPDATE chats SET title=?1, character_id=?2, updated_at=?3 WHERE id=?4")
        .bind(&title)
        .bind(character_id)
        .bind(&now)
        .bind(id)
        .execute(pool)
        .await?;
    get_chat(pool, id).await
}

pub async fn delete_chat(pool: &SqlitePool, id: i64) -> AppResult<()> {
    let result = sqlx::query("DELETE FROM chats WHERE id = ?1")
        .bind(id)
        .execute(pool)
        .await?;
    if result.rows_affected() == 0 {
        return Err(AppError::not_found("Chat not found"));
    }
    Ok(())
}

pub async fn list_messages(pool: &SqlitePool, chat_id: i64) -> AppResult<Vec<Message>> {
    let _ = get_chat(pool, chat_id).await?;
    let rows = sqlx::query_as::<_, MessageRow>(
        "SELECT m.id, m.chat_id, m.role, m.content, m.is_summary, m.created_at, j.status as job_status FROM messages m LEFT JOIN generation_jobs j ON j.message_id = m.id WHERE m.chat_id = ?1 ORDER BY m.created_at ASC",
    )
    .bind(chat_id)
    .fetch_all(pool)
    .await?;
    Ok(rows.into_iter().map(message_from_row).collect())
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

pub async fn touch_chat(pool: &SqlitePool, chat_id: i64) -> AppResult<()> {
    let now = Utc::now().to_rfc3339();
    sqlx::query("UPDATE chats SET updated_at = ?1 WHERE id = ?2")
        .bind(&now)
        .bind(chat_id)
        .execute(pool)
        .await?;
    Ok(())
}

pub async fn list_facts(pool: &SqlitePool, chat_id: i64) -> AppResult<Vec<Fact>> {
    let rows = sqlx::query_as::<_, FactRow>(
        "SELECT id, chat_id, key, value, updated_at FROM facts WHERE chat_id = ?1 ORDER BY key ASC",
    )
    .bind(chat_id)
    .fetch_all(pool)
    .await?;
    Ok(rows.into_iter().map(Into::into).collect())
}

pub async fn upsert_fact(
    pool: &SqlitePool,
    chat_id: i64,
    key: String,
    value: String,
) -> AppResult<Fact> {
    let now = Utc::now().to_rfc3339();
    sqlx::query(
        "INSERT INTO facts (chat_id, key, value, updated_at) VALUES (?1,?2,?3,?4) ON CONFLICT(chat_id, key) DO UPDATE SET value=excluded.value, updated_at=excluded.updated_at",
    )
    .bind(chat_id)
    .bind(&key)
    .bind(&value)
    .bind(&now)
    .execute(pool)
    .await?;
    let row = sqlx::query_as::<_, FactRow>(
        "SELECT id, chat_id, key, value, updated_at FROM facts WHERE chat_id = ?1 AND key = ?2",
    )
    .bind(chat_id)
    .bind(&key)
    .fetch_one(pool)
    .await?;
    Ok(row.into())
}

pub async fn delete_fact(pool: &SqlitePool, chat_id: i64, key: &str) -> AppResult<()> {
    let result = sqlx::query("DELETE FROM facts WHERE chat_id = ?1 AND key = ?2")
        .bind(chat_id)
        .bind(key)
        .execute(pool)
        .await?;
    if result.rows_affected() == 0 {
        return Err(AppError::not_found("Fact not found"));
    }
    Ok(())
}

pub async fn get_settings(pool: &SqlitePool) -> AppResult<Settings> {
    let row = sqlx::query_as::<_, SettingsRow>(
        "SELECT inference_url, model, temperature, top_p, max_tokens, system_prompt_prefix, system_prompt_suffix, summarize_enabled, summarize_after_messages, summarize_keep_recent, facts_enabled, max_context_messages FROM app_settings WHERE id = 1",
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
    if let Some(v) = payload.summarize_enabled {
        current.summarize_enabled = v;
    }
    if let Some(v) = payload.summarize_after_messages {
        current.summarize_after_messages = v;
    }
    if let Some(v) = payload.summarize_keep_recent {
        current.summarize_keep_recent = v;
    }
    if let Some(v) = payload.facts_enabled {
        current.facts_enabled = v;
    }
    if let Some(v) = payload.max_context_messages {
        current.max_context_messages = v;
    }
    if let Some(v) = payload.max_concurrent_jobs {
        MAX_CONCURRENT_JOBS.store(v.max(1), std::sync::atomic::Ordering::SeqCst);
        current.max_concurrent_jobs = v.max(1);
    }

    sqlx::query(
        "UPDATE app_settings SET inference_url=?1, model=?2, temperature=?3, top_p=?4, max_tokens=?5, system_prompt_prefix=?6, system_prompt_suffix=?7, summarize_enabled=?8, summarize_after_messages=?9, summarize_keep_recent=?10, facts_enabled=?11, max_context_messages=?12 WHERE id=1",
    )
    .bind(&current.inference_url)
    .bind(&current.model)
    .bind(current.temperature)
    .bind(current.top_p)
    .bind(current.max_tokens)
    .bind(&current.system_prompt_prefix)
    .bind(&current.system_prompt_suffix)
    .bind(current.summarize_enabled as i64)
    .bind(current.summarize_after_messages)
    .bind(current.summarize_keep_recent)
    .bind(current.facts_enabled as i64)
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
        "INSERT INTO generation_jobs (chat_id, message_id, status, position, created_at) VALUES (?1,?2,'queued',?3,?4) RETURNING id",
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
        "SELECT id, chat_id, message_id, status, error, position, created_at, started_at, completed_at FROM generation_jobs WHERE id = ?1",
    )
    .bind(id)
    .fetch_optional(pool)
    .await?
    .ok_or_else(|| AppError::not_found("Job not found"))?;
    Ok(row.into())
}

pub async fn get_active_job(pool: &SqlitePool, chat_id: i64) -> AppResult<Option<Job>> {
    let row = sqlx::query_as::<_, JobRow>(
        "SELECT id, chat_id, message_id, status, error, position, created_at, started_at, completed_at FROM generation_jobs WHERE chat_id = ?1 AND status IN ('queued','running') ORDER BY created_at ASC LIMIT 1",
    )
    .bind(chat_id)
    .fetch_optional(pool)
    .await?;
    Ok(row.map(Into::into))
}

pub async fn list_queue(pool: &SqlitePool) -> AppResult<(Vec<Job>, Vec<Job>)> {
    let running = sqlx::query_as::<_, JobRow>(
        "SELECT id, chat_id, message_id, status, error, position, created_at, started_at, completed_at FROM generation_jobs WHERE status = 'running' ORDER BY started_at ASC",
    )
    .fetch_all(pool)
    .await?
    .into_iter()
    .map(Into::into)
    .collect();
    let queued = sqlx::query_as::<_, JobRow>(
        "SELECT id, chat_id, message_id, status, error, position, created_at, started_at, completed_at FROM generation_jobs WHERE status = 'queued' ORDER BY created_at ASC",
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
        "SELECT id FROM generation_jobs WHERE status = 'queued' ORDER BY created_at ASC LIMIT ?1",
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

fn parse_dt(s: &str) -> AppResult<DateTime<Utc>> {
    DateTime::parse_from_rfc3339(s)
        .map(|dt| dt.with_timezone(&Utc))
        .or_else(|_| {
            chrono::NaiveDateTime::parse_from_str(s, "%Y-%m-%d %H:%M:%S").map(|ndt| ndt.and_utc())
        })
        .map_err(|e| AppError::internal(format!("invalid datetime: {e}")))
}

fn message_from_row(row: MessageRow) -> Message {
    Message {
        id: row.id,
        chat_id: row.chat_id,
        role: parse_role(&row.role),
        content: row.content,
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
    pub character_id: Option<i64>,
    pub summary: String,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(sqlx::FromRow)]
struct MessageRow {
    id: i64,
    chat_id: i64,
    role: String,
    content: String,
    is_summary: i64,
    created_at: String,
    job_status: Option<String>,
}

#[derive(sqlx::FromRow)]
struct FactRow {
    id: i64,
    chat_id: i64,
    key: String,
    value: String,
    updated_at: String,
}

impl From<FactRow> for Fact {
    fn from(row: FactRow) -> Self {
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
struct JobRow {
    id: i64,
    chat_id: i64,
    message_id: i64,
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
            chat_id: row.chat_id,
            message_id: row.message_id,
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
    summarize_enabled: i64,
    summarize_after_messages: i64,
    summarize_keep_recent: i64,
    facts_enabled: i64,
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
            summarize_enabled: self.summarize_enabled != 0,
            summarize_after_messages: self.summarize_after_messages,
            summarize_keep_recent: self.summarize_keep_recent,
            facts_enabled: self.facts_enabled != 0,
            max_context_messages: self.max_context_messages,
            max_concurrent_jobs: MAX_CONCURRENT_JOBS.load(std::sync::atomic::Ordering::SeqCst),
        }
    }
}
