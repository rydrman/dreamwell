use dreamwell_types::*;
use yew::prelude::*;

use crate::summary_ui::{chat_summarize_in_progress, SummaryKind};

const GENERATION_FAILED_PREFIX: &str = "[Generation failed: ";

pub fn generation_error_from_content(content: &str) -> Option<String> {
    if content.starts_with(GENERATION_FAILED_PREFIX) && content.ends_with(']') {
        return content
            .strip_prefix(GENERATION_FAILED_PREFIX)
            .and_then(|rest| rest.strip_suffix(']'))
            .map(str::to_string);
    }
    None
}

pub fn generation_error_message(content: &str, explicit: Option<&str>) -> Option<String> {
    if let Some(error) = explicit {
        if !error.is_empty() {
            return Some(error.to_string());
        }
    }
    generation_error_from_content(content)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GenerationPhase {
    Writing,
    Summarizing(SummaryKind),
    ProposingOutline,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GenerationNotice {
    Queued,
    Running(GenerationPhase),
}

impl GenerationNotice {
    pub fn message(self) -> &'static str {
        match self {
            Self::Queued => "Queued — waiting for server…",
            Self::Running(GenerationPhase::Writing) => "Still writing — more coming…",
            Self::Running(GenerationPhase::Summarizing(SummaryKind::ChatHistory)) => {
                "Summarizing earlier messages…"
            }
            Self::Running(GenerationPhase::Summarizing(SummaryKind::ChapterProse)) => {
                "Summarizing chapter prose…"
            }
            Self::Running(GenerationPhase::ProposingOutline) => "Proposing outline…",
        }
    }

    pub fn status_class(self) -> &'static str {
        match self {
            Self::Queued => "composer-status--queued",
            Self::Running(GenerationPhase::Summarizing(_)) => "composer-status--summarize",
            Self::Running(GenerationPhase::Writing)
            | Self::Running(GenerationPhase::ProposingOutline) => "composer-status--writing",
        }
    }

    pub fn textarea_placeholder(self) -> &'static str {
        match self {
            Self::Queued => "Reply waiting in queue…",
            Self::Running(GenerationPhase::Writing) => "Reply still generating…",
            Self::Running(GenerationPhase::Summarizing(SummaryKind::ChatHistory)) => {
                "Summarization in progress…"
            }
            Self::Running(GenerationPhase::Summarizing(SummaryKind::ChapterProse)) => {
                "Chapter summarization in progress…"
            }
            Self::Running(GenerationPhase::ProposingOutline) => "Outline proposal in progress…",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct JobStatusBadge {
    pub label: String,
    pub variant: JobStatusVariant,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum JobStatusVariant {
    Streaming,
    Queued,
}

impl JobStatusBadge {
    pub fn variant_class(&self) -> &'static str {
        match self.variant {
            JobStatusVariant::Streaming => "badge--streaming",
            JobStatusVariant::Queued => "badge--queued",
        }
    }
}

pub fn job_running_label(job_type: JobType) -> &'static str {
    match job_type {
        JobType::ChatMessage => "writing…",
        JobType::ChatSummarize => "summarizing…",
        JobType::ChatVariableRecheck => "checking variables…",
        JobType::StoryProposeChapters => "proposing chapters…",
        JobType::StoryProposeBeats => "proposing beats…",
        JobType::StoryBeatProse => "writing prose…",
        JobType::StoryBeatProseContinue => "continuing prose…",
        JobType::StoryBeatMechanical => "building mechanical plan…",
        JobType::StoryBeatProseRecheck => "aligning prose…",
        JobType::StoryChapterOutline => "outlining chapter…",
        JobType::StoryBeatOutline => "outlining beats…",
        JobType::StoryChapterSummarize => "summarizing chapter…",
        JobType::StoryBeatVariableRecheck => "checking variables…",
    }
}

pub fn job_status_badge(active_job: &Job, queued_jobs: i64) -> Option<JobStatusBadge> {
    match active_job.status {
        JobStatus::Running => Some(JobStatusBadge {
            label: job_running_label(active_job.job_type).to_string(),
            variant: JobStatusVariant::Streaming,
        }),
        JobStatus::Queued => {
            let label = if queued_jobs > 1 {
                format!("queued ({queued_jobs})")
            } else {
                "queued".to_string()
            };
            Some(JobStatusBadge {
                label,
                variant: JobStatusVariant::Queued,
            })
        }
        _ => Some(JobStatusBadge {
            label: format!("{:?}", active_job.status).to_lowercase(),
            variant: JobStatusVariant::Queued,
        }),
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BlockGenerationStatus {
    Queued,
    Streaming,
}

impl BlockGenerationStatus {
    pub fn label(self) -> &'static str {
        match self {
            Self::Queued => "waiting in queue…",
            Self::Streaming => "generating…",
        }
    }

    pub fn variant_class(self) -> &'static str {
        match self {
            Self::Queued => "badge--queued",
            Self::Streaming => "badge--streaming",
        }
    }
}

pub fn composer_notice(chat: &Chat, messages: &[Message]) -> Option<GenerationNotice> {
    if chat_summarize_in_progress(chat, messages) {
        return Some(GenerationNotice::Running(GenerationPhase::Summarizing(
            SummaryKind::ChatHistory,
        )));
    }
    if messages
        .iter()
        .any(|message| message.job_status == Some(JobStatus::Running))
    {
        return Some(GenerationNotice::Running(GenerationPhase::Writing));
    }
    if messages
        .iter()
        .any(|message| message.job_status == Some(JobStatus::Queued))
    {
        return Some(GenerationNotice::Queued);
    }
    let job = chat.active_job.as_ref()?;
    match job.status {
        JobStatus::Running => Some(GenerationNotice::Running(GenerationPhase::Writing)),
        JobStatus::Queued => Some(GenerationNotice::Queued),
        _ => None,
    }
}

pub fn story_notice(detail: &StoryDetail) -> Option<GenerationNotice> {
    for chapter in &detail.chapters {
        for beat in &chapter.beats {
            match beat.job_status {
                Some(JobStatus::Running) => {
                    return Some(GenerationNotice::Running(GenerationPhase::Writing));
                }
                Some(JobStatus::Queued) => return Some(GenerationNotice::Queued),
                _ => {}
            }
        }
    }

    let job = detail.story.active_job.as_ref()?;
    match job.status {
        JobStatus::Running => {
            let phase = match job.job_type {
                JobType::StoryProposeChapters | JobType::StoryProposeBeats => {
                    GenerationPhase::ProposingOutline
                }
                JobType::StoryChapterSummarize => {
                    GenerationPhase::Summarizing(SummaryKind::ChapterProse)
                }
                JobType::StoryBeatProse | JobType::StoryBeatProseContinue => {
                    GenerationPhase::Writing
                }
                _ => GenerationPhase::ProposingOutline,
            };
            Some(GenerationNotice::Running(phase))
        }
        JobStatus::Queued => Some(GenerationNotice::Queued),
        _ => None,
    }
}

pub fn chapter_block_status(
    chapter: &StoryChapter,
    active_job: Option<&Job>,
) -> Option<BlockGenerationStatus> {
    let job = active_job?;
    let scoped = match job.job_type {
        JobType::StoryProposeChapters => chapter.title.is_empty() && chapter.synopsis.is_empty(),
        JobType::StoryProposeBeats | JobType::StoryChapterSummarize => {
            job.chapter_id == Some(chapter.id)
        }
        _ => false,
    };
    if !scoped {
        return None;
    }
    match job.status {
        JobStatus::Queued => Some(BlockGenerationStatus::Queued),
        JobStatus::Running => Some(BlockGenerationStatus::Streaming),
        _ => None,
    }
}

pub fn beat_block_status(beat: &StoryBeat) -> Option<BlockGenerationStatus> {
    match beat.job_status {
        Some(JobStatus::Queued) => Some(BlockGenerationStatus::Queued),
        Some(JobStatus::Running) => Some(BlockGenerationStatus::Streaming),
        _ => None,
    }
}

pub fn chapter_has_substantial_prose(chapter: &StoryChapter) -> bool {
    chapter
        .beats
        .iter()
        .any(|beat| beat.content.chars().count() > 80)
}

pub fn chapter_summary_stale(chapter: &StoryChapter) -> bool {
    chapter_has_substantial_prose(chapter) && !chapter.prose_summary_valid
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StaleChapterItem {
    pub id: i64,
    pub number: i64,
    pub title: String,
}

pub fn stale_chapters_in_story(detail: &StoryDetail) -> Vec<StaleChapterItem> {
    let mut stale = detail
        .chapters
        .iter()
        .filter(|ch| chapter_summary_stale(ch))
        .map(|ch| StaleChapterItem {
            id: ch.id,
            number: ch.sort_order + 1,
            title: if ch.title.is_empty() {
                "…".to_string()
            } else {
                ch.title.clone()
            },
        })
        .collect::<Vec<_>>();
    stale.sort_by_key(|ch| ch.number);
    stale
}

pub fn is_stale_summary_error(err: &str) -> bool {
    err.contains("prose summary is stale")
}

#[derive(Properties, PartialEq)]
pub struct GenerationStatusBarProps {
    pub notice: GenerationNotice,
}

#[function_component(GenerationStatusBar)]
pub fn generation_status_bar(props: &GenerationStatusBarProps) -> Html {
    let notice = props.notice;
    html! {
        <div
            class={classes!("composer-status", notice.status_class())}
            role="status"
            aria-live="polite"
        >
            <span class="settings-save-spinner" aria-hidden="true"></span>
            <span>{ notice.message() }</span>
        </div>
    }
}

fn generation_down_arrow() -> Html {
    html! {
        <svg
            class="generation-btn-arrow"
            width="14"
            height="14"
            viewBox="0 0 14 14"
            fill="none"
            aria-hidden="true"
        >
            <path
                d="M7 2.5v7M7 9.5l-3-3M7 9.5l3-3"
                stroke="currentColor"
                stroke-width="1.5"
                stroke-linecap="round"
                stroke-linejoin="round"
            />
        </svg>
    }
}

#[derive(Properties, PartialEq)]
pub struct GuidanceModalProps {
    pub title: &'static str,
    pub placeholder: &'static str,
    pub guidance: String,
    pub generate_label: &'static str,
    pub loading_label: &'static str,
    pub disabled: bool,
    pub busy: bool,
    pub on_close: Callback<()>,
    pub on_guidance: Callback<String>,
    pub on_generate: Callback<()>,
}

#[function_component(GuidanceModal)]
pub fn guidance_modal(props: &GuidanceModalProps) -> Html {
    let draft = use_state(|| props.guidance.clone());
    {
        let draft = draft.clone();
        let guidance = props.guidance.clone();
        use_effect_with(guidance, move |guidance| {
            draft.set(guidance.clone());
            || ()
        });
    }

    let on_input = {
        let draft = draft.clone();
        Callback::from(move |e: InputEvent| {
            let input: web_sys::HtmlInputElement = e.target_unchecked_into();
            draft.set(input.value());
        })
    };

    let on_generate = {
        let draft = draft.clone();
        let on_guidance = props.on_guidance.clone();
        let on_generate = props.on_generate.clone();
        let on_close = props.on_close.clone();
        Callback::from(move |_| {
            on_guidance.emit((*draft).clone());
            on_generate.emit(());
            on_close.emit(());
        })
    };

    html! {
        <>
            <div class="modal-backdrop" onclick={props.on_close.reform(|_| ())} />
            <div class="modal guidance-modal" role="dialog" aria-labelledby="guidance-modal-title">
                <h2 id="guidance-modal-title">{ props.title }</h2>
                <label class="field">
                    <span class="muted">{"Optional notes for the AI"}</span>
                    <textarea
                        placeholder={props.placeholder}
                        value={(*draft).clone()}
                        rows="4"
                        oninput={on_input}
                    />
                </label>
                <div class="modal-actions">
                    <button
                        class="btn"
                        disabled={props.disabled || props.busy}
                        onclick={on_generate}
                    >
                        { if props.busy { props.loading_label } else { props.generate_label } }
                    </button>
                    <button class="btn secondary" onclick={props.on_close.reform(|_| ())}>
                        {"Cancel"}
                    </button>
                </div>
            </div>
        </>
    }
}

#[derive(Properties, PartialEq)]
pub struct GenerationButtonGroupProps {
    pub label: &'static str,
    pub loading_label: &'static str,
    #[prop_or(false)]
    pub secondary: bool,
    #[prop_or(true)]
    pub show_arrow: bool,
    pub disabled: bool,
    pub busy: bool,
    pub guidance: String,
    pub guidance_title: &'static str,
    pub guidance_placeholder: &'static str,
    pub on_guidance: Callback<String>,
    pub on_generate: Callback<()>,
}

#[function_component(GenerationButtonGroup)]
pub fn generation_button_group(props: &GenerationButtonGroupProps) -> Html {
    let modal_open = use_state(|| false);
    let has_guidance = !props.guidance.trim().is_empty();

    let open_modal = {
        let modal_open = modal_open.clone();
        Callback::from(move |_| modal_open.set(true))
    };
    let close_modal = {
        let modal_open = modal_open.clone();
        Callback::from(move |_| modal_open.set(false))
    };

    let btn_class = if props.secondary {
        classes!("btn", "secondary", "generation-btn")
    } else {
        classes!("btn", "generation-btn")
    };

    html! {
        <>
            <div class="generation-btn-group">
                <button
                    class={btn_class}
                    disabled={props.disabled || props.busy}
                    onclick={props.on_generate.reform(|_| ())}
                >
                    if props.show_arrow {
                        { generation_down_arrow() }
                    }
                    <span>{ if props.busy { props.loading_label } else { props.label } }</span>
                </button>
                <button
                    class={classes!(
                        "btn",
                        "secondary",
                        "btn-compact",
                        "generation-btn-guidance",
                        has_guidance.then_some("generation-btn-guidance--has-notes"),
                    )}
                    disabled={props.disabled || props.busy}
                    title="Add guidance"
                    aria-label="Add guidance"
                    onclick={open_modal}
                >
                    {"✎"}
                </button>
            </div>
            if *modal_open {
                <GuidanceModal
                    title={props.guidance_title}
                    placeholder={props.guidance_placeholder}
                    guidance={props.guidance.clone()}
                    generate_label={props.label}
                    loading_label={props.loading_label}
                    disabled={props.disabled}
                    busy={props.busy}
                    on_close={close_modal}
                    on_guidance={props.on_guidance.clone()}
                    on_generate={props.on_generate.clone()}
                />
            }
        </>
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_job(status: JobStatus, job_type: JobType) -> Job {
        serde_json::from_value(serde_json::json!({
            "id": 1,
            "job_type": job_type,
            "chat_id": 1,
            "message_id": 1,
            "guidance_notes": "",
            "status": status,
            "position": 0,
            "created_at": "2026-01-01T00:00:00Z",
        }))
        .expect("sample job")
    }

    #[test]
    fn parses_generation_error_from_content() {
        assert_eq!(
            generation_error_from_content("[Generation failed: timeout]"),
            Some("timeout".to_string())
        );
        assert_eq!(generation_error_from_content("normal prose"), None);
    }

    #[test]
    fn running_chat_message_shows_writing() {
        let job = sample_job(JobStatus::Running, JobType::ChatMessage);
        let badge = job_status_badge(&job, 0).expect("badge");
        assert_eq!(badge.label, "writing…");
        assert_eq!(badge.variant, JobStatusVariant::Streaming);
    }

    #[test]
    fn running_story_prose_shows_writing_prose() {
        let job = sample_job(JobStatus::Running, JobType::StoryBeatProse);
        let badge = job_status_badge(&job, 0).expect("badge");
        assert_eq!(badge.label, "writing prose…");
    }

    #[test]
    fn chapter_summarize_shows_chapter_specific_notice() {
        let detail: StoryDetail = serde_json::from_value(serde_json::json!({
            "story": {
                "id": 1,
                "title": "Test",
                "premise": "",
                "tone": "",
                "genre": "",
                "pov": "",
                "length_preset": "short",
                "notes": "",
                "created_at": "2026-01-01T00:00:00Z",
                "updated_at": "2026-01-01T00:00:00Z",
                "queued_jobs": 0,
                "active_job": {
                    "id": 2,
                    "job_type": "story_chapter_summarize",
                    "story_id": 1,
                    "chapter_id": 10,
                    "guidance_notes": "",
                    "status": "running",
                    "position": 0,
                    "created_at": "2026-01-01T00:00:00Z"
                }
            },
            "chapters": [{
                "id": 10,
                "story_id": 1,
                "title": "Ch1",
                "synopsis": "",
                "sort_order": 0,
                "created_at": "2026-01-01T00:00:00Z",
                "updated_at": "2026-01-01T00:00:00Z",
                "beats": []
            }]
        }))
        .expect("story detail");
        let notice = story_notice(&detail).expect("notice");
        assert_eq!(notice.message(), "Summarizing chapter prose…");
    }

    #[test]
    fn beat_job_running_takes_precedence_in_story_notice() {
        let detail: StoryDetail = serde_json::from_value(serde_json::json!({
            "story": {
                "id": 1,
                "title": "Test",
                "premise": "",
                "tone": "",
                "genre": "",
                "pov": "",
                "length_preset": "short",
                "notes": "",
                "created_at": "2026-01-01T00:00:00Z",
                "updated_at": "2026-01-01T00:00:00Z",
                "queued_jobs": 0,
                "active_job": {
                    "id": 2,
                    "job_type": "story_propose_chapters",
                    "story_id": 1,
                    "guidance_notes": "",
                    "status": "running",
                    "position": 0,
                    "created_at": "2026-01-01T00:00:00Z"
                }
            },
            "chapters": [{
                "id": 10,
                "story_id": 1,
                "title": "Ch1",
                "synopsis": "",
                "sort_order": 0,
                "created_at": "2026-01-01T00:00:00Z",
                "updated_at": "2026-01-01T00:00:00Z",
                "beats": [{
                    "id": 20,
                    "chapter_id": 10,
                    "title": "Beat",
                    "synopsis": "",
                    "content": "",
                    "sort_order": 0,
                    "created_at": "2026-01-01T00:00:00Z",
                    "updated_at": "2026-01-01T00:00:00Z",
                    "job_status": "running"
                }]
            }]
        }))
        .expect("story detail");
        assert_eq!(
            story_notice(&detail),
            Some(GenerationNotice::Running(GenerationPhase::Writing))
        );
    }

    #[test]
    fn chapter_summary_stale_requires_substantial_prose() {
        let chapter: StoryChapter = serde_json::from_value(serde_json::json!({
            "id": 1,
            "story_id": 1,
            "title": "Ch1",
            "synopsis": "",
            "prose_summary_valid": false,
            "sort_order": 0,
            "created_at": "2026-01-01T00:00:00Z",
            "updated_at": "2026-01-01T00:00:00Z",
            "beats": [{
                "id": 2,
                "chapter_id": 1,
                "title": "Beat",
                "synopsis": "",
                "content": "short",
                "sort_order": 0,
                "created_at": "2026-01-01T00:00:00Z",
                "updated_at": "2026-01-01T00:00:00Z"
            }]
        }))
        .expect("chapter");
        assert!(!chapter_summary_stale(&chapter));
    }

    #[test]
    fn detects_stale_summary_errors() {
        assert!(is_stale_summary_error(
            "Chapter 2 prose summary is stale — summarize it before working on later chapters"
        ));
        assert!(!is_stale_summary_error("network error"));
    }
}
