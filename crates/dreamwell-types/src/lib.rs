use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

mod macros;

pub use macros::{substitute_macros, MacroContext};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum JobStatus {
    Queued,
    Running,
    Completed,
    Failed,
    Cancelled,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum JobType {
    ChatMessage,
    StoryChapterOutline,
    StoryBeatOutline,
    StoryBeatProse,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum LengthPreset {
    Flash,
    #[default]
    Short,
    Novella,
    Novel,
}

impl LengthPreset {
    pub fn ref_chapters(self) -> i64 {
        match self {
            Self::Flash => 3,
            Self::Short => 5,
            Self::Novella => 8,
            Self::Novel => 12,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Flash => "Flash (3 chapters)",
            Self::Short => "Short (5 chapters)",
            Self::Novella => "Novella (8 chapters)",
            Self::Novel => "Novel (12 chapters)",
        }
    }
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
    pub character_id: i64,
    pub character_name: String,
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
    pub character_id: i64,
}

fn default_title() -> String {
    "New Chat".to_string()
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct ChatUpdate {
    pub title: Option<String>,
    pub character_id: Option<i64>,
}

/// SillyTavern-style main prompt default.
pub const DEFAULT_SYSTEM_PROMPT_PREFIX: &str =
    "Write {{char}}'s next reply in a fictional chat between {{char}} and {{user}}.";

/// Default user/persona name (SillyTavern `username`).
pub const DEFAULT_USER_NAME: &str = "User";

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
    pub job_type: JobType,
    pub chat_id: Option<i64>,
    pub message_id: Option<i64>,
    pub story_id: Option<i64>,
    pub chapter_id: Option<i64>,
    pub beat_id: Option<i64>,
    pub guidance_notes: String,
    pub status: JobStatus,
    pub error: Option<String>,
    pub position: i64,
    pub created_at: DateTime<Utc>,
    pub started_at: Option<DateTime<Utc>>,
    pub completed_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Story {
    pub id: i64,
    pub title: String,
    pub premise: String,
    pub tone: String,
    pub genre: String,
    pub pov: String,
    pub length_preset: LengthPreset,
    pub notes: String,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub active_job: Option<Job>,
    pub queued_jobs: i64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct StoryChapter {
    pub id: i64,
    pub story_id: i64,
    pub title: String,
    pub synopsis: String,
    pub sort_order: i64,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub beats: Vec<StoryBeat>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct StoryBeat {
    pub id: i64,
    pub chapter_id: i64,
    pub title: String,
    pub synopsis: String,
    pub content: String,
    pub sort_order: i64,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub job_status: Option<JobStatus>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct StoryDetail {
    pub story: Story,
    pub chapters: Vec<StoryChapter>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct StoryCreate {
    #[serde(default = "default_story_title")]
    pub title: String,
    #[serde(default)]
    pub premise: String,
    #[serde(default)]
    pub tone: String,
    #[serde(default)]
    pub genre: String,
    #[serde(default)]
    pub pov: String,
    #[serde(default = "default_length_preset")]
    pub length_preset: LengthPreset,
    #[serde(default)]
    pub notes: String,
}

fn default_story_title() -> String {
    "Untitled Story".to_string()
}

fn default_length_preset() -> LengthPreset {
    LengthPreset::Short
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct StoryUpdate {
    pub title: Option<String>,
    pub premise: Option<String>,
    pub tone: Option<String>,
    pub genre: Option<String>,
    pub pov: Option<String>,
    pub length_preset: Option<LengthPreset>,
    pub notes: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct StoryChapterCreate {
    #[serde(default)]
    pub title: String,
    #[serde(default)]
    pub synopsis: String,
    pub sort_order: Option<i64>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct StoryChapterUpdate {
    pub title: Option<String>,
    pub synopsis: Option<String>,
    pub sort_order: Option<i64>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct StoryBeatCreate {
    #[serde(default)]
    pub title: String,
    #[serde(default)]
    pub synopsis: String,
    #[serde(default)]
    pub content: String,
    pub sort_order: Option<i64>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct StoryBeatUpdate {
    pub title: Option<String>,
    pub synopsis: Option<String>,
    pub content: Option<String>,
    pub sort_order: Option<i64>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct GenerateRequest {
    #[serde(default)]
    pub guidance_notes: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct StoryStreamPayload {
    pub detail: StoryDetail,
    pub active_job: Option<Job>,
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
    pub user_name: String,
    pub persona_description: String,
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
    pub user_name: Option<String>,
    pub persona_description: Option<String>,
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
