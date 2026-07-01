use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

mod game_elements;
mod game_import;
mod game_presets;
mod macros;
mod scenario_export;
mod scenario_iw;
mod serde_helpers;
mod state_payload;

pub use game_elements::{
    prose_check_marker, prose_mech_marker, prose_state_marker, BoardDef, BoardTagRule, CardDef,
    DeckDef, DeckInstance, ElementInstances, EngineMode, GameElementsConfig, MechanicalData,
    MechanicalKind, MechanicalResult, TurnObservability, PROSE_CHECK_MARKER_OPEN,
    PROSE_INLINE_MARKER_CLOSE, PROSE_MECH_MARKER_CLOSE, PROSE_MECH_MARKER_OPEN,
    PROSE_STATE_MARKER_OPEN,
};
pub use game_import::{
    game_create_from_character, scenario_create_from_character,
    scenario_create_from_character_record, GameCharacterImportMode,
};
pub use game_presets::{game_tone_preset_by_id, GameTonePreset, GAME_TONE_PRESETS};
pub use macros::{empty_setup_vars, substitute_macros, MacroContext};
pub use scenario_export::{
    is_scenario_export_value, parse_scenario_export_json, scenario_create_from_scenario,
    ScenarioExport, SCENARIO_EXPORT_FORMAT,
};
pub use scenario_iw::{
    merge_character_state, merge_game_state_schema, normalize_target, split_legacy_state_schema,
    tracked_var_to_character_state, CharacterStateDef, ContentFlags, GameTurnSystemRoll,
    GenerateCharacterStateRequest, GenerateCharacterStateResponse, GenerateCharacterStateTarget,
    PcOption, RulesBlock, ScenarioNpc, ScenarioTrigger, SetupVarChoice, SourceMeta,
    SystemRollRequest, TrackedVarDef, TraitDef, TriggerCondition, TriggerEffect, TurnPlan,
    WinCondition,
};
pub use state_payload::{clamp_measurement, SequencePayload, StepSequenceResult};

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
    GameSceneSummarize,
    GameProseRecheck,
    GameStateRecheck,
    GameTurnStructuredAgent,
    GameTurnProseRegenerate,
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
    /// Transient notice while generation retries on a fallback model.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub generation_notice: String,
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
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub num_value: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_value: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub float_value: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub float_min: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub float_max: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub unit: Option<String>,
    pub source_message_id: i64,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ChatStateEntryUpdate {
    pub value: Option<String>,
    pub num_value: Option<i64>,
    pub max_value: Option<i64>,
    pub float_value: Option<f64>,
    pub float_min: Option<f64>,
    pub float_max: Option<f64>,
    pub unit: Option<String>,
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
    /// Active provider connection label while the job is running.
    #[serde(default)]
    pub generation_provider: String,
    /// Active model id while the job is running.
    #[serde(default)]
    pub generation_model: String,
    /// Transient fallback notice while retrying another provider.
    #[serde(default)]
    pub generation_notice: String,
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
    #[serde(skip_serializing_if = "Option::is_none")]
    pub archived_at: Option<DateTime<Utc>>,
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
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub num_value: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_value: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub float_value: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub float_min: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub float_max: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub unit: Option<String>,
    pub source_beat_id: i64,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct StoryStateEntryUpdate {
    pub value: Option<String>,
    pub num_value: Option<i64>,
    pub max_value: Option<i64>,
    pub float_value: Option<f64>,
    pub float_min: Option<f64>,
    pub float_max: Option<f64>,
    pub unit: Option<String>,
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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum JsonFormatStrategy {
    /// Probe on first structured JSON call, then cache per connection.
    #[default]
    Auto,
    /// OpenAI `response_format.type = json_schema`.
    ResponseJsonSchema,
    /// vLLM top-level `guided_json` (Featherless and similar).
    GuidedJson,
    /// OpenAI `response_format.type = json_object`.
    JsonObject,
}

impl JsonFormatStrategy {
    pub fn label(self) -> &'static str {
        match self {
            Self::Auto => "Auto (detect on first use)",
            Self::ResponseJsonSchema => "OpenAI json_schema",
            Self::GuidedJson => "vLLM guided_json",
            Self::JsonObject => "json_object",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct InferenceConnection {
    pub id: i64,
    pub name: String,
    pub inference_url: String,
    pub api_key_set: bool,
    #[serde(default)]
    pub model: String,
    /// When false, skipped during provider fallback.
    #[serde(default = "default_connection_enabled")]
    pub enabled: bool,
    /// Priority order for provider fallback (lower = tried first).
    #[serde(default)]
    pub sort_order: i64,
    #[serde(default)]
    pub json_format_strategy: JsonFormatStrategy,
    /// Text-embedded tool-call parser for streaming game narration (`auto`, `none`, or a dynamo parser name).
    #[serde(default = "default_tool_call_parser")]
    pub tool_call_parser: String,
    #[serde(default = "default_connection_temperature")]
    pub temperature: f64,
    #[serde(default = "default_connection_top_p")]
    pub top_p: f64,
    #[serde(default = "default_connection_max_tokens")]
    pub max_tokens: i64,
    #[serde(default = "default_connection_context_tokens")]
    pub context_tokens: i64,
    #[serde(default = "default_connection_max_context_messages")]
    pub max_context_messages: i64,
    #[serde(default = "default_connection_auto_context_on_model_change")]
    pub auto_context_on_model_change: bool,
}

fn default_connection_enabled() -> bool {
    true
}

/// Human-readable label for a provider connection.
pub fn connection_label(conn: &InferenceConnection) -> String {
    let name = conn.name.trim();
    if !name.is_empty() {
        return name.to_string();
    }
    let url = conn.inference_url.trim();
    if !url.is_empty() {
        return url.to_string();
    }
    format!("Connection {}", conn.id)
}

/// Whether a connection has enough config to attempt inference.
pub fn connection_inference_ready(conn: &InferenceConnection) -> bool {
    conn.enabled && !conn.inference_url.trim().is_empty() && !conn.model.trim().is_empty()
}

/// Enabled connections in fallback priority order (`sort_order`, then `id`).
pub fn fallback_connections(connections: &[InferenceConnection]) -> Vec<&InferenceConnection> {
    let mut ready: Vec<&InferenceConnection> = connections
        .iter()
        .filter(|conn| connection_inference_ready(conn))
        .collect();
    ready.sort_by_key(|conn| (conn.sort_order, conn.id));
    ready
}

pub fn has_inference_provider(connections: &[InferenceConnection]) -> bool {
    !fallback_connections(connections).is_empty()
}

fn default_tool_call_parser() -> String {
    "auto".to_string()
}

fn default_connection_temperature() -> f64 {
    0.8
}

fn default_connection_top_p() -> f64 {
    0.9
}

fn default_connection_max_tokens() -> i64 {
    512
}

fn default_connection_context_tokens() -> i64 {
    8192
}

fn default_connection_max_context_messages() -> i64 {
    40
}

fn default_connection_auto_context_on_model_change() -> bool {
    true
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct InferenceConnectionCreate {
    pub name: String,
    pub inference_url: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub api_key: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct InferenceConnectionUpdate {
    pub name: Option<String>,
    pub inference_url: Option<String>,
    /// Omitted keeps the existing key; an empty string clears it.
    pub api_key: Option<String>,
    pub model: Option<String>,
    pub enabled: Option<bool>,
    pub sort_order: Option<i64>,
    pub json_format_strategy: Option<JsonFormatStrategy>,
    pub tool_call_parser: Option<String>,
    pub temperature: Option<f64>,
    pub top_p: Option<f64>,
    pub max_tokens: Option<i64>,
    pub context_tokens: Option<i64>,
    pub max_context_messages: Option<i64>,
    pub auto_context_on_model_change: Option<bool>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Settings {
    pub inference_url: String,
    pub active_connection_id: Option<i64>,
    pub connections: Vec<InferenceConnection>,
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
    pub active_connection_id: Option<i64>,
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

/// Tokens for schema-constrained JSON completions (plan, checks, resolve).
///
/// Uses the configured `max_tokens`, reserving at least one third of `context_tokens`
/// for the prompt when context size is known.
pub fn structured_output_tokens(settings: &Settings) -> i64 {
    const FLOOR: i64 = 512;
    let requested = settings.max_tokens.max(FLOOR);
    if settings.context_tokens > 0 {
        requested.min(settings.context_tokens / 3)
    } else {
        requested
    }
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
mod regenerate_turn_request_tests {
    use super::{RegenerateTurnRequest, RegenerateTurnScope};

    #[test]
    fn regenerate_turn_request_serializes_prose_only_scope() {
        let req = RegenerateTurnRequest {
            scope: RegenerateTurnScope::ProseOnly,
        };
        assert_eq!(
            serde_json::to_string(&req).unwrap(),
            r#"{"scope":"prose_only"}"#
        );
    }

    #[test]
    fn regenerate_turn_request_deserializes_prose_only_scope() {
        let req: RegenerateTurnRequest = serde_json::from_str(r#"{"scope":"prose_only"}"#).unwrap();
        assert_eq!(req.scope, RegenerateTurnScope::ProseOnly);
    }
}

#[cfg(test)]
mod context_budget_tests {
    use super::{
        prompt_token_budget, structured_output_tokens, suggested_response_tokens, Settings,
    };

    fn sample_settings(max_tokens: i64, context_tokens: i64) -> Settings {
        Settings {
            inference_url: String::new(),
            active_connection_id: None,
            connections: Vec::new(),
            model: String::new(),
            temperature: 0.7,
            top_p: 1.0,
            max_tokens,
            system_prompt_prefix: String::new(),
            system_prompt_suffix: String::new(),
            user_name: String::new(),
            persona_description: String::new(),
            summarize_enabled: false,
            summarize_adaptive: false,
            summarize_after_messages: 12,
            summarize_keep_recent: 4,
            variables_enabled: false,
            thought_blocks_enabled: false,
            max_context_messages: 0,
            context_tokens,
            auto_context_on_model_change: false,
            max_concurrent_jobs: 1,
        }
    }

    #[test]
    fn suggested_response_scales_with_context() {
        assert_eq!(suggested_response_tokens(8192), 1024);
        assert_eq!(suggested_response_tokens(2048), 256);
    }

    #[test]
    fn prompt_budget_subtracts_response() {
        assert_eq!(prompt_token_budget(8192, 1024), 7168);
    }

    #[test]
    fn structured_output_uses_max_tokens_not_hard_cap() {
        let s = sample_settings(4096, 32768);
        assert_eq!(structured_output_tokens(&s), 4096);
    }

    #[test]
    fn structured_output_respects_context_budget() {
        let s = sample_settings(4096, 8192);
        assert_eq!(structured_output_tokens(&s), 2730);
    }

    #[test]
    fn structured_output_floor_when_max_tokens_low() {
        let s = sample_settings(128, 8192);
        assert_eq!(structured_output_tokens(&s), 512);
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

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum StateKind {
    #[default]
    #[serde(alias = "fact")]
    Variable,
    Condition,
    #[serde(alias = "resource", alias = "gauge")]
    Measurement,
    #[serde(alias = "clock")]
    Sequence,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum StateOp {
    #[serde(alias = "replace")]
    Set,
    Add,
    Remove,
    SetMin,
    SetMax,
    Step,
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
    /// GM guidance applied to the auto-submitted first turn when starting a game from this scenario.
    #[serde(default)]
    pub opening_guidance: String,
    pub pc_name: String,
    pub pc_description: String,
    #[serde(default)]
    pub pc_initial_state: Vec<CharacterStateDef>,
    #[serde(default)]
    pub traits: std::collections::HashMap<String, i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub character_id: Option<i64>,
    #[serde(default)]
    pub rules_blocks: Vec<RulesBlock>,
    #[serde(default)]
    pub objective: String,
    #[serde(default)]
    pub setup_text: String,
    #[serde(default)]
    pub trait_defs: Vec<TraitDef>,
    #[serde(default)]
    pub cast: Vec<ScenarioNpc>,
    #[serde(default)]
    pub pc_options: Vec<PcOption>,
    #[serde(default)]
    pub state_schema: Vec<TrackedVarDef>,
    /// State copied onto every cast member at game start; per-NPC entries override by key.
    #[serde(default)]
    pub cast_uniform_state: Vec<CharacterStateDef>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub win_condition: Option<WinCondition>,
    #[serde(default)]
    pub content_flags: ContentFlags,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_meta: Option<SourceMeta>,
    #[serde(default)]
    pub scenario_triggers: Vec<ScenarioTrigger>,
    #[serde(default)]
    pub game_elements: GameElementsConfig,
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
    pub opening_guidance: String,
    #[serde(default)]
    pub pc_name: String,
    #[serde(default)]
    pub pc_description: String,
    #[serde(default)]
    pub pc_initial_state: Vec<CharacterStateDef>,
    #[serde(default = "default_game_traits")]
    pub traits: std::collections::HashMap<String, i64>,
    #[serde(default)]
    pub character_id: Option<i64>,
    #[serde(default)]
    pub rules_blocks: Vec<RulesBlock>,
    #[serde(default)]
    pub objective: String,
    #[serde(default)]
    pub setup_text: String,
    #[serde(default)]
    pub trait_defs: Vec<TraitDef>,
    #[serde(default)]
    pub cast: Vec<ScenarioNpc>,
    #[serde(default)]
    pub pc_options: Vec<PcOption>,
    #[serde(default)]
    pub state_schema: Vec<TrackedVarDef>,
    #[serde(default)]
    pub cast_uniform_state: Vec<CharacterStateDef>,
    #[serde(default)]
    pub win_condition: Option<WinCondition>,
    #[serde(default)]
    pub content_flags: ContentFlags,
    #[serde(default)]
    pub source_meta: Option<SourceMeta>,
    #[serde(default)]
    pub scenario_triggers: Vec<ScenarioTrigger>,
    #[serde(default)]
    pub game_elements: GameElementsConfig,
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
    pub opening_guidance: Option<String>,
    pub pc_name: Option<String>,
    pub pc_description: Option<String>,
    pub pc_initial_state: Option<Vec<CharacterStateDef>>,
    pub traits: Option<std::collections::HashMap<String, i64>>,
    pub character_id: Option<Option<i64>>,
    pub rules_blocks: Option<Vec<RulesBlock>>,
    pub objective: Option<String>,
    pub setup_text: Option<String>,
    pub trait_defs: Option<Vec<TraitDef>>,
    pub cast: Option<Vec<ScenarioNpc>>,
    pub pc_options: Option<Vec<PcOption>>,
    pub state_schema: Option<Vec<TrackedVarDef>>,
    pub cast_uniform_state: Option<Vec<CharacterStateDef>>,
    pub win_condition: Option<Option<WinCondition>>,
    pub content_flags: Option<ContentFlags>,
    pub source_meta: Option<Option<SourceMeta>>,
    pub scenario_triggers: Option<Vec<ScenarioTrigger>>,
    pub game_elements: Option<GameElementsConfig>,
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
    pub engine_mode: EngineMode,
    #[serde(default)]
    pub game_elements: GameElementsConfig,
    #[serde(default)]
    pub element_instances: ElementInstances,
    #[serde(default)]
    pub model_checks: String,
    #[serde(default)]
    pub model_resolve: String,
    #[serde(default)]
    pub model_prose: String,
    #[serde(default)]
    pub rules_blocks: Vec<RulesBlock>,
    #[serde(default)]
    pub state_schema: Vec<TrackedVarDef>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub win_condition: Option<WinCondition>,
    #[serde(default)]
    pub scenario_triggers: Vec<ScenarioTrigger>,
    #[serde(default)]
    pub trait_defs: Vec<TraitDef>,
    /// GM-only rolling scene memory (narrative considerations, directions, NPC interior).
    #[serde(default)]
    pub author_notes: String,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub archived_at: Option<DateTime<Utc>>,
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
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub num_value: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_value: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub float_value: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub float_min: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub float_max: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub unit: Option<String>,
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
    /// Previous float value for measurements (full precision).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub prev_float: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub prev_float_min: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub prev_float_max: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub prev_unit: Option<String>,
    /// Unit label for measurement changes (canonical UCUM or custom).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub unit: Option<String>,
    /// Kind of the entry before this change, when an existing slot was updated or replaced.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub prev_kind: Option<StateKind>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct GameTurn {
    pub id: i64,
    pub game_id: i64,
    pub sort_order: i64,
    pub player_action: String,
    #[serde(default)]
    pub guidance_notes: String,
    pub phase: String,
    pub scene_beats: Vec<String>,
    pub prose: String,
    #[serde(default)]
    pub thought_content: String,
    #[serde(default)]
    pub thought_duration_ms: Option<i64>,
    #[serde(default)]
    pub thought_in_progress: bool,
    pub state_changes: Vec<AppliedStateChange>,
    pub checks: Vec<GameTurnCheck>,
    #[serde(default)]
    pub system_rolls: Vec<GameTurnSystemRoll>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub plan: Option<TurnPlan>,
    #[serde(default)]
    pub mechanical_results: Vec<MechanicalResult>,
    #[serde(default)]
    pub observability: TurnObservability,
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
    #[serde(default)]
    pub rules_blocks: Vec<RulesBlock>,
    #[serde(default)]
    pub state_schema: Vec<TrackedVarDef>,
    #[serde(default)]
    pub win_condition: Option<WinCondition>,
    #[serde(default)]
    pub scenario_triggers: Vec<ScenarioTrigger>,
    #[serde(default)]
    pub trait_defs: Vec<TraitDef>,
    #[serde(default)]
    pub cast_selections: Vec<String>,
    #[serde(default)]
    pub invited_cast: Vec<ScenarioNpc>,
    #[serde(default)]
    pub setup_var_values: std::collections::HashMap<String, String>,
    /// When true, `opening_message` is submitted as the first player turn instead of
    /// seeding a static narrator opening bubble (Infinite Worlds `firstInput`).
    #[serde(default)]
    pub opening_as_player_action: bool,
    /// GM guidance for the auto-submitted first turn (from scenario `opening_guidance`).
    #[serde(default)]
    pub opening_guidance: String,
    #[serde(default)]
    pub engine_mode: EngineMode,
    #[serde(default)]
    pub game_elements: GameElementsConfig,
    #[serde(default)]
    pub element_instances: ElementInstances,
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
    pub engine_mode: Option<EngineMode>,
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
    pub float_value: Option<f64>,
    pub float_min: Option<f64>,
    pub float_max: Option<f64>,
    pub unit: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SubmitTurnRequest {
    pub player_action: String,
    #[serde(default)]
    pub guidance_notes: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RewindTurnRequest {
    /// When true, delete the selected turn as well (rewind before a player action).
    /// When false, keep the selected turn and delete later turns (rewind after a GM response).
    #[serde(default)]
    pub include_turn: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum RegenerateTurnScope {
    #[default]
    Full,
    ProseOnly,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct RegenerateTurnRequest {
    #[serde(default)]
    pub scope: RegenerateTurnScope,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TurnEditField {
    Prose,
    PlayerAction,
    Mechanicals,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct UpdateTurnRequest {
    pub field: TurnEditField,
    pub content: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct GameStreamPayload {
    pub detail: GameDetail,
    pub active_job: Option<Job>,
}

/// LLM output for Phase 1 — declare checks.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DeclareChecksResponse {
    #[serde(default, alias = "check", alias = "dramatic_checks", alias = "rolls")]
    pub checks: Vec<DeclaredCheck>,
    #[serde(default, alias = "reason", alias = "no_checks_reason")]
    pub no_check_reason: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DeclaredCheck {
    #[serde(alias = "name", alias = "title", alias = "check_label")]
    pub label: String,
    #[serde(alias = "trait", alias = "trait_name", alias = "skill_name")]
    pub skill: String,
    #[serde(alias = "mod", alias = "mod_value", alias = "bonus")]
    pub modifier: i64,
    pub stakes: String,
    #[serde(alias = "reason", alias = "rationale")]
    pub justification: String,
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
    #[serde(
        default,
        deserialize_with = "serde_helpers::deserialize_optional_literal_string",
        serialize_with = "serde_helpers::serialize_optional_literal_string"
    )]
    pub value: Option<String>,
    #[serde(default)]
    pub delta: Option<i64>,
    #[serde(default)]
    pub float_value: Option<f64>,
    #[serde(default)]
    pub float_min: Option<f64>,
    #[serde(default)]
    pub float_max: Option<f64>,
    #[serde(default)]
    pub unit: Option<String>,
    #[serde(default)]
    pub sequence_items: Option<Vec<String>>,
    #[serde(default)]
    pub sequence_position: Option<i64>,
    #[serde(default)]
    pub sequence_loop: Option<bool>,
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
