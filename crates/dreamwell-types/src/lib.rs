use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum JobStatus {
    Queued,
    Running,
    Completed,
    Failed,
    Cancelled,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum MessageRole {
    System,
    User,
    Assistant,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Character {
    pub id: i64,
    pub name: String,
    pub description: String,
    pub personality: String,
    pub scenario: String,
    pub first_message: String,
    pub example_dialogue: String,
    pub system_prompt: String,
    pub avatar_url: Option<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CharacterCreate {
    pub name: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub personality: String,
    #[serde(default)]
    pub scenario: String,
    #[serde(default)]
    pub first_message: String,
    #[serde(default)]
    pub example_dialogue: String,
    #[serde(default)]
    pub system_prompt: String,
    pub avatar_url: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct CharacterUpdate {
    pub name: Option<String>,
    pub description: Option<String>,
    pub personality: Option<String>,
    pub scenario: Option<String>,
    pub first_message: Option<String>,
    pub example_dialogue: Option<String>,
    pub system_prompt: Option<String>,
    pub avatar_url: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Chat {
    pub id: i64,
    pub title: String,
    pub character_id: Option<i64>,
    pub summary: String,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub active_job: Option<Job>,
    pub queued_jobs: i64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ChatCreate {
    #[serde(default = "default_title")]
    pub title: String,
    pub character_id: Option<i64>,
}

fn default_title() -> String {
    "New Chat".to_string()
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct ChatUpdate {
    pub title: Option<String>,
    pub character_id: Option<i64>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Message {
    pub id: i64,
    pub chat_id: i64,
    pub role: MessageRole,
    pub content: String,
    pub is_summary: bool,
    pub created_at: DateTime<Utc>,
    pub job_status: Option<JobStatus>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SendMessageRequest {
    pub content: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Fact {
    pub id: i64,
    pub chat_id: i64,
    pub key: String,
    pub value: String,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FactUpdate {
    pub key: String,
    pub value: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Job {
    pub id: i64,
    pub chat_id: i64,
    pub message_id: i64,
    pub status: JobStatus,
    pub error: Option<String>,
    pub position: i64,
    pub created_at: DateTime<Utc>,
    pub started_at: Option<DateTime<Utc>>,
    pub completed_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct QueueStatus {
    pub running: Vec<Job>,
    pub queued: Vec<Job>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Settings {
    pub inference_url: String,
    pub model: String,
    pub temperature: f64,
    pub top_p: f64,
    pub max_tokens: i64,
    pub system_prompt_prefix: String,
    pub system_prompt_suffix: String,
    pub summarize_enabled: bool,
    pub summarize_after_messages: i64,
    pub summarize_keep_recent: i64,
    pub facts_enabled: bool,
    pub max_context_messages: i64,
    pub max_concurrent_jobs: i64,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct SettingsUpdate {
    pub inference_url: Option<String>,
    pub model: Option<String>,
    pub temperature: Option<f64>,
    pub top_p: Option<f64>,
    pub max_tokens: Option<i64>,
    pub system_prompt_prefix: Option<String>,
    pub system_prompt_suffix: Option<String>,
    pub summarize_enabled: Option<bool>,
    pub summarize_after_messages: Option<i64>,
    pub summarize_keep_recent: Option<i64>,
    pub facts_enabled: Option<bool>,
    pub max_context_messages: Option<i64>,
    pub max_concurrent_jobs: Option<i64>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ModelInfo {
    pub id: String,
    pub name: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ImportCharacterResponse {
    pub character: Character,
    pub source: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ChatStreamPayload {
    pub chat: Chat,
    pub messages: Vec<Message>,
    pub active_job: Option<Job>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct HealthResponse {
    pub status: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct OkResponse {
    pub ok: bool,
}
