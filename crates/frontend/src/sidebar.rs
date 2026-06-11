use dreamwell_types::*;
use yew::prelude::*;

use crate::queue_ui::AppMode;

#[derive(Properties, PartialEq)]
pub struct AppSidebarProps {
    pub mode: AppMode,
    pub chats: Vec<Chat>,
    pub archived_chats: Vec<Chat>,
    pub stories: Vec<Story>,
    pub selected_chat_id: Option<i64>,
    pub selected_story_id: Option<i64>,
    pub on_mode: Callback<AppMode>,
    pub on_select_chat: Callback<i64>,
    pub on_new_chat: Callback<()>,
    pub on_archive_chat: Callback<i64>,
    pub on_restore_chat: Callback<i64>,
    pub on_permanent_delete_chat: Callback<i64>,
    pub on_select_story: Callback<i64>,
    pub on_new_story: Callback<()>,
    pub on_delete_story: Callback<i64>,
}

#[function_component(AppSidebar)]
pub fn app_sidebar(props: &AppSidebarProps) -> Html {
    let archive_open = use_state(|| false);
    let archive_count = props.archived_chats.len();
    let chats_active = props.mode == AppMode::Chats;

    html! {
        <aside class="sidebar">
            <div class="sidebar-section-tabs">
                <button
                    class={classes!("sidebar-section-btn", chats_active.then_some("active"))}
                    onclick={props.on_mode.reform(|_| AppMode::Chats)}
                >
                    {"Chats"}
                </button>
                <button
                    class={classes!("sidebar-section-btn", (!chats_active).then_some("active"))}
                    onclick={props.on_mode.reform(|_| AppMode::Stories)}
                >
                    {"Stories"}
                </button>
            </div>
            <div class="sidebar-toolbar">
                if chats_active {
                    <button class="btn" onclick={props.on_new_chat.reform(|_| ())}>{"New chat"}</button>
                } else {
                    <button class="btn" onclick={props.on_new_story.reform(|_| ())}>{"New story"}</button>
                }
            </div>
            <div class="sidebar-scroll">
                if chats_active {
                    { for props.chats.iter().map(|chat| {
                        let id = chat.id;
                        let status = chat_status(chat);
                        let selected = props.selected_chat_id == Some(chat.id);
                        html! {
                            <div key={id} class={classes!("chat-item", selected.then_some("selected"))}>
                                <div style="display:flex;gap:0.5rem;align-items:flex-start;">
                                    <div style="flex:1;min-width:0;" onclick={props.on_select_chat.reform(move |_| id)}>
                                        <div class="chat-item-title">{ &chat.title }</div>
                                        <div class="chat-character">{ &chat.character_name }</div>
                                        if let Some(label) = status {
                                            <span class="badge">{ label }</span>
                                        }
                                    </div>
                                    <button
                                        class="btn secondary btn-compact"
                                        title="Archive chat"
                                        onclick={props.on_archive_chat.reform(move |_| id)}
                                    >
                                        {"✕"}
                                    </button>
                                </div>
                            </div>
                        }
                    }) }
                } else {
                    { for props.stories.iter().map(|story| {
                        let id = story.id;
                        let selected = props.selected_story_id == Some(story.id);
                        let status = story_status(story);
                        html! {
                            <div key={id} class={classes!("chat-item", selected.then_some("selected"))}>
                                <div style="display:flex;gap:0.5rem;">
                                    <div style="flex:1;min-width:0;" onclick={props.on_select_story.reform(move |_| id)}>
                                        <div style="overflow:hidden;text-overflow:ellipsis;white-space:nowrap;">{ &story.title }</div>
                                        if let Some(label) = status {
                                            <span class="badge">{ label }</span>
                                        }
                                    </div>
                                    <button class="btn secondary btn-compact" onclick={props.on_delete_story.reform(move |_| id)}>{"✕"}</button>
                                </div>
                            </div>
                        }
                    }) }
                }
            </div>
            if chats_active && archive_count > 0 {
                <div class="archive-panel">
                    <button class="archive-toggle" onclick={{
                        let archive_open = archive_open.clone();
                        Callback::from(move |_| archive_open.set(!*archive_open))
                    }}>
                        <span>{ if *archive_open { "▾" } else { "▸" } }</span>
                        <span>{ format!("Archive ({archive_count})") }</span>
                    </button>
                    if *archive_open {
                        <div class="archive-list">
                            { for props.archived_chats.iter().map(|chat| {
                                let id = chat.id;
                                let days_left = chat
                                    .archived_at
                                    .map(dreamwell_types::days_until_chat_archive_purge);
                                html! {
                                    <div key={id} class="chat-item archived">
                                        <div class="archive-item-title">{ &chat.title }</div>
                                        <div class="chat-character">{ &chat.character_name }</div>
                                        if let Some(days) = days_left {
                                            <div class="archive-meta muted">{ format!("{days} days left") }</div>
                                        }
                                        <div class="archive-actions">
                                            <button class="btn secondary btn-compact" onclick={props.on_restore_chat.reform(move |_| id)}>{"Restore"}</button>
                                            <button class="btn secondary btn-compact text-danger" onclick={props.on_permanent_delete_chat.reform(move |_| id)}>{"Delete"}</button>
                                        </div>
                                    </div>
                                }
                            }) }
                        </div>
                    }
                </div>
            }
        </aside>
    }
}

fn chat_status(chat: &Chat) -> Option<String> {
    let job = chat.active_job.as_ref()?;
    match job.status {
        JobStatus::Running => Some(chat_job_label(job)),
        JobStatus::Queued => {
            let label = chat_job_label(job);
            if chat.queued_jobs > 1 {
                Some(format!("{label} ({})", chat.queued_jobs))
            } else {
                Some(label)
            }
        }
        _ => Some(format!("{:?}", job.status).to_lowercase()),
    }
}

fn chat_job_label(job: &Job) -> String {
    match job.job_type {
        JobType::ChatSummarize => "summarizing…".to_string(),
        _ => "writing…".to_string(),
    }
}

fn story_status(story: &Story) -> Option<String> {
    let job = story.active_job.as_ref()?;
    match job.status {
        JobStatus::Running => Some("generating…".to_string()),
        JobStatus::Queued => {
            if story.queued_jobs > 1 {
                Some(format!("queued ({})", story.queued_jobs))
            } else {
                Some("queued".to_string())
            }
        }
        _ => None,
    }
}
