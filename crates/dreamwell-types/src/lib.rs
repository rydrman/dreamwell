use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

mod game_import;
mod game_presets;
mod macros;

pub use game_import::{
    game_create_from_character, scenario_create_from_character,
    scenario_create_from_character_record, GameCharacterImportMode,
};
pub use game_presets::{game_tone_preset_by_id, GameTonePreset, GAME_TONE_PRESETS};
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
    ChatSummarize,
    ChatVariableRecheck,
    StoryChapterOutline,
    StoryProposeChapters,
    StoryBeatOutline,
    StoryProposeBeats,
    StoryBeatProse,
    StoryBeatProseContinue,
    StoryBeatMechanical,
    StoryBeatProseRecheck,
    StoryChapterSummarize,
    StoryBeatVariableRecheck,
    GameTurnCheck,
    GameTurnResolve,
    GameTurnScenePlan,
    GameTurnProse,
    GameSceneSummarize,
    GameProseRecheck,
    GameStateRecheck,
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

pub const CHAT_ARCHIVE_RETENTION_DAYS: i64 = 90;

pub fn days_until_chat_archive_purge(archived_at: DateTime<Utc>) -> i64 {
    let purge_at = archived_at + chrono::Duration::days(CHAT_ARCHIVE_RETENTION_DAYS);
    (purge_at - Utc::now()).num_days().max(0)
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
    #[serde(skip_serializing_if = "Option::is_none")]
    pub archived_at: Option<DateTime<Utc>>,
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
    pub summary: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RegenerateSummaryRequest {
    pub marker_id: i64,
}

/// SillyTavern-style main prompt default.
pub const DEFAULT_SYSTEM_PROMPT_PREFIX: &str =
    "Write {{char}}'s next reply in a fictional chat between {{char}} and {{user}}.";

/// Default user/persona name (SillyTavern `username`).
pub const DEFAULT_USER_NAME: &str = "User";

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct MessageVariableUpdate {
    pub key: String,
    pub value: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub previous_value: Option<String>,
}

impl MessageVariableUpdate {
    pub fn clears(&self) -> bool {
        self.value.is_empty()
    }
}

#[derive(Deserialize)]
struct MessageVariableUpdateRaw {
    key: String,
    #[serde(default)]
    value: String,
    #[serde(default)]
    previous_value: Option<String>,
    #[serde(default)]
    deleted: bool,
}

impl<'de> Deserialize<'de> for MessageVariableUpdate {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let raw = MessageVariableUpdateRaw::deserialize(deserializer)?;
        Ok(Self {
            key: raw.key,
            value: if raw.deleted {
                String::new()
            } else {
                raw.value
            },
            previous_value: raw.previous_value,
        })
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Message {
    pub id: i64,
    pub chat_id: i64,
    pub role: MessageRole,
    pub content: String,
    #[serde(default)]
    pub thought_content: String,
    #[serde(default)]
    pub thought_duration_ms: Option<i64>,
    #[serde(default)]
    pub thought_in_progress: bool,
    #[serde(default)]
    pub variable_updates: Vec<MessageVariableUpdate>,
    #[serde(default)]
    pub reply_beats: Vec<String>,
    #[serde(default)]
    pub state_changes: Vec<AppliedStateChange>,
    #[serde(default)]
    pub generation_phase: String,
    pub is_summary: bool,
    /// True when this message has been folded into `chat.summary` (kept in UI, omitted from model context).
    #[serde(default)]
    pub in_summary: bool,
    pub created_at: DateTime<Utc>,
    pub job_status: Option<JobStatus>,
    /// Set when the most recent generation job for this message failed.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub generation_error: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SendMessageRequest {
    pub content: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct UpdateMessageRequest {
    pub content: String,
    #[serde(default)]
    pub rewind: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RegenerateMessageRequest {
    #[serde(default)]
    pub rewind: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ChatVariable {
    pub id: i64,
    pub chat_id: i64,
    pub key: String,
    pub value: String,
    /// `-1` = manual / session-wide panel entry; otherwise the message that introduced this value.
    pub source_message_id: i64,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ChatVariableUpdate {
    pub key: String,
    pub value: String,
    /// `-1` or omitted = manual. Otherwise anchors the change to a message.
    #[serde(default)]
    pub source_message_id: Option<i64>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ChatActor {
    pub id: i64,
    pub chat_id: i64,
    pub role: String,
    pub name: String,
    pub description: String,
    #[serde(default)]
    pub skills: std::collections::HashMap<String, i64>,
    pub sort_order: i64,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ChatStateEntry {
    pub id: i64,
    pub chat_id: i64,
    pub actor_id: Option<i64>,
    pub kind: StateKind,
    pub key: String,
    pub value: String,
    pub num_value: Option<i64>,
    pub max_value: Option<i64>,
    pub source_message_id: i64,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ChatStateEntryUpdate {
    pub value: Option<String>,
    pub num_value: Option<i64>,
    pub max_value: Option<i64>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ChatDetail {
    pub chat: Chat,
    pub actors: Vec<ChatActor>,
    pub state: Vec<ChatStateEntry>,
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
    pub game_id: Option<i64>,
    pub turn_id: Option<i64>,
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
    #[serde(default)]
    pub tracked_details: String,
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
    #[serde(default)]
    pub prose_summary: String,
    #[serde(default)]
    pub prose_summary_valid: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub prose_summary_at: Option<DateTime<Utc>>,
    pub sort_order: i64,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub beats: Vec<StoryBeat>,
}

/// Per-beat audit trail for story variable changes (same shape as chat message updates).
pub type BeatVariableUpdate = MessageVariableUpdate;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct StoryBeat {
    pub id: i64,
    pub chapter_id: i64,
    pub title: String,
    pub synopsis: String,
    #[serde(default)]
    pub mechanical: String,
    pub content: String,
    #[serde(default)]
    pub variable_updates: Vec<BeatVariableUpdate>,
    #[serde(default)]
    pub plan_beats: Vec<String>,
    #[serde(default)]
    pub state_changes: Vec<AppliedStateChange>,
    pub sort_order: i64,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub job_status: Option<JobStatus>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct StoryVariable {
    pub id: i64,
    pub story_id: i64,
    pub key: String,
    pub value: String,
    pub source_chapter_order: i64,
    pub source_beat_order: i64,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct StoryVariableUpdate {
    pub key: String,
    pub value: String,
    #[serde(default)]
    pub source_chapter_order: Option<i64>,
    #[serde(default)]
    pub source_beat_order: Option<i64>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct StoryDetail {
    pub story: Story,
    pub chapters: Vec<StoryChapter>,
    #[serde(default)]
    pub actors: Vec<StoryActor>,
    #[serde(default)]
    pub state: Vec<StoryStateEntry>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct StoryActor {
    pub id: i64,
    pub story_id: i64,
    pub role: String,
    pub name: String,
    pub description: String,
    #[serde(default)]
    pub skills: std::collections::HashMap<String, i64>,
    pub sort_order: i64,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct StoryStateEntry {
    pub id: i64,
    pub story_id: i64,
    pub actor_id: Option<i64>,
    pub kind: StateKind,
    pub key: String,
    pub value: String,
    pub num_value: Option<i64>,
    pub max_value: Option<i64>,
    pub source_beat_id: i64,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct StoryStateEntryUpdate {
    pub value: Option<String>,
    pub num_value: Option<i64>,
    pub max_value: Option<i64>,
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
    #[serde(default)]
    pub tracked_details: String,
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
    pub tracked_details: Option<String>,
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
    pub mechanical: String,
    #[serde(default)]
    pub content: String,
    pub sort_order: Option<i64>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct StoryBeatUpdate {
    pub title: Option<String>,
    pub synopsis: Option<String>,
    pub mechanical: Option<String>,
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
    /// When true, trigger summarization from estimated token budget (context − response).
    pub summarize_adaptive: bool,
    /// Minimum total messages before summarization can run.
    pub summarize_after_messages: i64,
    /// Minimum recent messages to always keep verbatim.
    pub summarize_keep_recent: i64,
    pub variables_enabled: bool,
    pub thought_blocks_enabled: bool,
    pub max_context_messages: i64,
    /// Total model context window (prompt + response). Used for budgeting hints.
    pub context_tokens: i64,
    /// When true, selecting a model probes the backend and updates context_tokens.
    pub auto_context_on_model_change: bool,
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
    pub summarize_adaptive: Option<bool>,
    pub summarize_after_messages: Option<i64>,
    pub summarize_keep_recent: Option<i64>,
    pub variables_enabled: Option<bool>,
    pub thought_blocks_enabled: Option<bool>,
    pub max_context_messages: Option<i64>,
    pub context_tokens: Option<i64>,
    pub auto_context_on_model_change: Option<bool>,
    pub max_concurrent_jobs: Option<i64>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ModelInfo {
    pub id: String,
    pub name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub context_length: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub context_source: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ModelCapabilities {
    pub model: String,
    pub context_length: Option<i64>,
    pub context_source: Option<String>,
}

/// Suggested response length when auto-tuning after context detection.
pub fn suggested_response_tokens(context_tokens: i64) -> i64 {
    if context_tokens <= 0 {
        return 512;
    }
    (context_tokens / 8).clamp(256, 4096)
}

/// Tokens available for the prompt after reserving response length.
pub fn prompt_token_budget(context_tokens: i64, response_tokens: i64) -> i64 {
    if context_tokens <= 0 {
        return 0;
    }
    (context_tokens - response_tokens).max(0)
}

/// Rough token estimate (~4 characters per token).
pub fn estimate_token_count(text: &str) -> i64 {
    ((text.len() as i64) + 3) / 4
}

#[cfg(test)]
mod context_budget_tests {
    use super::{prompt_token_budget, suggested_response_tokens};

    #[test]
    fn suggested_response_scales_with_context() {
        assert_eq!(suggested_response_tokens(8192), 1024);
        assert_eq!(suggested_response_tokens(2048), 256);
    }

    #[test]
    fn prompt_budget_subtracts_response() {
        assert_eq!(prompt_token_budget(8192, 1024), 7168);
    }
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
    #[serde(default)]
    pub actors: Vec<ChatActor>,
    #[serde(default)]
    pub state: Vec<ChatStateEntry>,
    pub active_job: Option<Job>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ResolutionSystem {
    Pbta2d6,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum CheckTier {
    Fail,
    Mixed,
    Strong,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum StateKind {
    Resource,
    Condition,
    Fact,
    Clock,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum StateOp {
    Set,
    Add,
    Remove,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Scenario {
    pub id: i64,
    pub title: String,
    pub premise: String,
    pub setting: String,
    pub gm_style: String,
    #[serde(default)]
    pub opening_message: String,
    pub pc_name: String,
    pub pc_description: String,
    #[serde(default)]
    pub traits: std::collections::HashMap<String, i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub character_id: Option<i64>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct ScenarioCreate {
    #[serde(default = "default_scenario_title")]
    pub title: String,
    #[serde(default)]
    pub premise: String,
    #[serde(default)]
    pub setting: String,
    #[serde(default)]
    pub gm_style: String,
    #[serde(default)]
    pub opening_message: String,
    #[serde(default)]
    pub pc_name: String,
    #[serde(default)]
    pub pc_description: String,
    #[serde(default = "default_game_traits")]
    pub traits: std::collections::HashMap<String, i64>,
    #[serde(default)]
    pub character_id: Option<i64>,
}

fn default_scenario_title() -> String {
    "Untitled Scenario".to_string()
}

pub fn default_game_traits() -> std::collections::HashMap<String, i64> {
    [
        ("Finesse", 0),
        ("Force", 0),
        ("Flair", 0),
        ("Focus", 0),
        ("Sway", 0),
    ]
    .into_iter()
    .map(|(name, value)| (name.to_string(), value))
    .collect()
}

pub fn normalize_game_traits(
    traits: std::collections::HashMap<String, i64>,
) -> std::collections::HashMap<String, i64> {
    if traits.is_empty() {
        default_game_traits()
    } else {
        traits
    }
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct ScenarioUpdate {
    pub title: Option<String>,
    pub premise: Option<String>,
    pub setting: Option<String>,
    pub gm_style: Option<String>,
    pub opening_message: Option<String>,
    pub pc_name: Option<String>,
    pub pc_description: Option<String>,
    pub traits: Option<std::collections::HashMap<String, i64>>,
    pub character_id: Option<Option<i64>>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ImportScenarioResponse {
    pub scenario: Scenario,
    pub source: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ImportGameDraftResponse {
    pub draft: GameCreate,
    pub source: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Game {
    pub id: i64,
    pub title: String,
    pub premise: String,
    pub setting: String,
    pub gm_style: String,
    #[serde(default)]
    pub opening_message: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub character_id: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub scenario_id: Option<i64>,
    pub resolution_system: ResolutionSystem,
    pub modifier_min: i64,
    pub modifier_max: i64,
    pub merge_resolve_scene: bool,
    pub step_mode: bool,
    #[serde(default)]
    pub model_checks: String,
    #[serde(default)]
    pub model_resolve: String,
    #[serde(default)]
    pub model_prose: String,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub active_job: Option<Job>,
    pub queued_jobs: i64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct GameActor {
    pub id: i64,
    pub game_id: i64,
    pub role: String,
    pub name: String,
    pub description: String,
    pub skills: std::collections::HashMap<String, i64>,
    pub sort_order: i64,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

/// Shared actor row used by chat, story, and game state engines.
pub type SessionActor = GameActor;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct GameStateEntry {
    pub id: i64,
    pub game_id: i64,
    pub actor_id: Option<i64>,
    pub kind: StateKind,
    pub key: String,
    pub value: String,
    pub num_value: Option<i64>,
    pub max_value: Option<i64>,
    pub source_turn: i64,
    pub updated_at: DateTime<Utc>,
}

/// Shared materialized state entry used by chat, story, and game state engines.
pub type StateEntry = GameStateEntry;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct GameTurnCheck {
    pub id: i64,
    pub turn_id: i64,
    pub label: String,
    pub skill: String,
    pub modifier: i64,
    pub stakes: String,
    pub justification: String,
    pub dice_expr: String,
    pub seed: i64,
    pub rolls: Vec<i64>,
    pub total: i64,
    pub tier: Option<CheckTier>,
    pub margin: i64,
    pub sort_order: i64,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AppliedStateChange {
    pub target: String,
    pub kind: StateKind,
    pub key: String,
    pub op: StateOp,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub value: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub delta: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub prev_value: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub prev_num: Option<i64>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct GameTurn {
    pub id: i64,
    pub game_id: i64,
    pub sort_order: i64,
    pub player_action: String,
    pub phase: String,
    pub scene_beats: Vec<String>,
    pub prose: String,
    pub state_changes: Vec<AppliedStateChange>,
    pub checks: Vec<GameTurnCheck>,
    #[serde(default)]
    pub is_opening: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub generation_error: Option<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct GameScene {
    pub id: i64,
    pub game_id: i64,
    pub title: String,
    pub summary: String,
    pub summary_valid: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub summary_at: Option<DateTime<Utc>>,
    pub start_turn: i64,
    pub sort_order: i64,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct GameDetail {
    pub game: Game,
    pub actors: Vec<GameActor>,
    pub state: Vec<GameStateEntry>,
    pub turns: Vec<GameTurn>,
    pub scenes: Vec<GameScene>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct GameCreate {
    #[serde(default = "default_game_title")]
    pub title: String,
    #[serde(default)]
    pub premise: String,
    #[serde(default)]
    pub setting: String,
    #[serde(default)]
    pub gm_style: String,
    #[serde(default)]
    pub opening_message: String,
    #[serde(default)]
    pub character_id: Option<i64>,
    #[serde(default)]
    pub scenario_id: Option<i64>,
    #[serde(default)]
    pub pc_name: String,
    #[serde(default)]
    pub pc_description: String,
    #[serde(default)]
    pub pc_traits: std::collections::HashMap<String, i64>,
}

fn default_game_title() -> String {
    "Untitled Game".to_string()
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct GameUpdate {
    pub title: Option<String>,
    pub premise: Option<String>,
    pub setting: Option<String>,
    pub gm_style: Option<String>,
    pub opening_message: Option<String>,
    pub modifier_min: Option<i64>,
    pub modifier_max: Option<i64>,
    pub merge_resolve_scene: Option<bool>,
    pub step_mode: Option<bool>,
    pub resolution_system: Option<ResolutionSystem>,
    pub model_checks: Option<String>,
    pub model_resolve: Option<String>,
    pub model_prose: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct GameActorUpdate {
    pub name: Option<String>,
    pub description: Option<String>,
    pub skills: Option<std::collections::HashMap<String, i64>>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct GameStateEntryUpdate {
    pub value: Option<String>,
    pub num_value: Option<i64>,
    pub max_value: Option<i64>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SubmitTurnRequest {
    pub player_action: String,
    #[serde(default)]
    pub guidance_notes: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct GameStreamPayload {
    pub detail: GameDetail,
    pub active_job: Option<Job>,
}

/// LLM output for Phase 1 — declare checks.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DeclareChecksResponse {
    #[serde(default)]
    pub checks: Vec<DeclaredCheck>,
    #[serde(default)]
    pub no_check_reason: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DeclaredCheck {
    pub label: String,
    pub skill: String,
    pub modifier: i64,
    pub stakes: String,
    pub justification: String,
}

/// LLM output for Phase 2 — resolve + state delta (+ scene beats when merged).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ResolveTurnResponse {
    #[serde(default)]
    pub scene_beats: Vec<String>,
    #[serde(default)]
    pub state_changes: Vec<StateChangeRequest>,
}

/// Shared plan-phase JSON for chat, story, and game modes.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PlanPhaseResponse {
    #[serde(
        default,
        alias = "scene_beats",
        alias = "reply_beats",
        alias = "plan_beats"
    )]
    pub beats: Vec<String>,
    #[serde(default)]
    pub state_changes: Vec<StateChangeRequest>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct StateChangeRequest {
    pub target: String,
    pub kind: StateKind,
    pub key: String,
    pub op: StateOp,
    #[serde(default)]
    pub value: Option<String>,
    #[serde(default)]
    pub delta: Option<i64>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct HealthResponse {
    pub status: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub git_sha: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct OkResponse {
    pub ok: bool,
}
