use dreamwell_types::{Chat, JobType, Message};
use yew::prelude::*;

use crate::markdown;

pub const CHAT_SUMMARIZE_PLACEHOLDER: &str = "Summarizing earlier messages…";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SummaryKind {
    ChatHistory,
    ChapterProse,
}

impl SummaryKind {
    pub fn break_label(self, pending: bool) -> &'static str {
        match (self, pending) {
            (Self::ChatHistory, true) => "Summarizing",
            (Self::ChatHistory, false) => "Summary",
            (Self::ChapterProse, true) => "Summarizing",
            (Self::ChapterProse, false) => "Prose summary",
        }
    }

    pub fn pending_message(self) -> &'static str {
        match self {
            Self::ChatHistory => "Compressing chat history…",
            Self::ChapterProse => "Compressing chapter prose…",
        }
    }

    pub fn toggle_label(self) -> &'static str {
        "View summary"
    }
}

pub fn is_chat_summarize_pending(message: &Message) -> bool {
    message.is_summary && message.content.starts_with("Summarizing earlier")
}

pub fn chat_summarize_in_progress(chat: &Chat, _messages: &[Message]) -> bool {
    chat.active_job
        .as_ref()
        .is_some_and(|job| job.job_type == JobType::ChatSummarize)
}

#[derive(Properties, PartialEq)]
pub struct SummaryBreakProps {
    pub kind: SummaryKind,
    pub pending: bool,
}

#[function_component(SummaryBreak)]
pub fn summary_break(props: &SummaryBreakProps) -> Html {
    html! {
        <div class="summary-break" aria-hidden="true">
            <span class="summary-break-line"></span>
            <span class="summary-break-label">{ props.kind.break_label(props.pending) }</span>
            <span class="summary-break-line"></span>
        </div>
    }
}

#[derive(Properties, PartialEq)]
pub struct SummaryViewProps {
    pub text: String,
    pub pending: bool,
    pub kind: SummaryKind,
    #[prop_or(true)]
    pub default_expanded: bool,
    #[prop_or_default]
    pub extra_actions: Html,
}

#[function_component(SummaryView)]
pub fn summary_view(props: &SummaryViewProps) -> Html {
    let expanded = use_state(|| props.default_expanded);
    let has_text = !props.text.trim().is_empty();

    let toggle = {
        let expanded = expanded.clone();
        Callback::from(move |_| expanded.set(!*expanded))
    };

    let summary_html = if props.text.is_empty() {
        html! { <span class="muted">{"(Empty summary)"}</span> }
    } else {
        markdown::render_message_content(&props.text)
    };

    html! {
        <div class="summary-view">
            if props.pending {
                <p class="summary-pending muted">
                    <span class="settings-save-spinner" aria-hidden="true"></span>
                    { format!(" {}", props.kind.pending_message()) }
                </p>
            } else if has_text {
                <>
                    <div class="summary-actions">
                        <button type="button" class="summary-toggle" onclick={toggle}>
                            <span class="summary-chevron">{ if *expanded { "▾" } else { "▸" } }</span>
                            <span>{ props.kind.toggle_label() }</span>
                        </button>
                        { props.extra_actions.clone() }
                    </div>
                    if *expanded {
                        <div class="summary-content">{ summary_html }</div>
                    }
                </>
            }
        </div>
    }
}
