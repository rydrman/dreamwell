use dreamwell_types::*;
use yew::prelude::*;

use crate::generation_ui::job_status_badge;
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
    pub on_open_characters: Callback<()>,
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
                    <>
                        <button class="btn" onclick={props.on_new_chat.reform(|_| ())}>{"New chat"}</button>
                        <button class="btn secondary" onclick={props.on_open_characters.reform(|_| ())}>{"Characters"}</button>
                    </>
                } else {
                    <button class="btn" onclick={props.on_new_story.reform(|_| ())}>{"New story"}</button>
                }
            </div>
            <div class="sidebar-scroll">
                if chats_active {
                    { for props.chats.iter().map(|chat| {
                        let id = chat.id;
                        let status = chat.active_job.as_ref().and_then(|job| job_status_badge(job, chat.queued_jobs));
                        let selected = props.selected_chat_id == Some(chat.id);
                        html! {
                            <div key={id} class={classes!("chat-item", selected.then_some("selected"))}>
                                <div style="display:flex;gap:0.5rem;align-items:flex-start;">
                                    <div style="flex:1;min-width:0;" onclick={props.on_select_chat.reform(move |_| id)}>
                                        <div class="chat-item-title">{ &chat.title }</div>
                                        <div class="chat-character">{ &chat.character_name }</div>
                                        if let Some(status) = status {
                                            <span class={classes!("badge", status.variant_class())}>{ status.label }</span>
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
                        let status = story.active_job.as_ref().and_then(|job| job_status_badge(job, story.queued_jobs));
                        html! {
                            <div key={id} class={classes!("chat-item", selected.then_some("selected"))}>
                                <div style="display:flex;gap:0.5rem;">
                                    <div style="flex:1;min-width:0;" onclick={props.on_select_story.reform(move |_| id)}>
                                        <div style="overflow:hidden;text-overflow:ellipsis;white-space:nowrap;">{ &story.title }</div>
                                        if let Some(status) = status {
                                            <span class={classes!("badge", status.variant_class())}>{ status.label }</span>
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::generation_ui::{job_status_badge, JobStatusVariant};

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

    fn sample_chat(active_job: Option<Job>, queued_jobs: i64) -> Chat {
        let mut chat: Chat = serde_json::from_value(serde_json::json!({
            "id": 1,
            "title": "Test",
            "character_id": 1,
            "character_name": "Char",
            "summary": "",
            "created_at": "2026-01-01T00:00:00Z",
            "updated_at": "2026-01-01T00:00:00Z",
            "queued_jobs": queued_jobs,
        }))
        .expect("sample chat");
        chat.active_job = active_job;
        chat
    }

    #[test]
    fn running_chat_message_shows_writing() {
        let chat = sample_chat(
            Some(sample_job(JobStatus::Running, JobType::ChatMessage)),
            0,
        );
        let badge = chat
            .active_job
            .as_ref()
            .and_then(|job| job_status_badge(job, chat.queued_jobs))
            .expect("badge");
        assert_eq!(badge.label, "writing…");
        assert_eq!(badge.variant, JobStatusVariant::Streaming);
    }

    #[test]
    fn running_summarize_shows_summarizing() {
        let chat = sample_chat(
            Some(sample_job(JobStatus::Running, JobType::ChatSummarize)),
            0,
        );
        let badge = chat
            .active_job
            .as_ref()
            .and_then(|job| job_status_badge(job, chat.queued_jobs))
            .expect("badge");
        assert_eq!(badge.label, "summarizing…");
        assert_eq!(badge.variant, JobStatusVariant::Streaming);
    }

    #[test]
    fn queued_job_shows_queued_not_writing() {
        let chat = sample_chat(Some(sample_job(JobStatus::Queued, JobType::ChatMessage)), 1);
        let badge = chat
            .active_job
            .as_ref()
            .and_then(|job| job_status_badge(job, chat.queued_jobs))
            .expect("badge");
        assert_eq!(badge.label, "queued");
        assert_eq!(badge.variant, JobStatusVariant::Queued);
    }

    #[test]
    fn queued_jobs_show_count() {
        let chat = sample_chat(Some(sample_job(JobStatus::Queued, JobType::ChatMessage)), 3);
        let badge = chat
            .active_job
            .as_ref()
            .and_then(|job| job_status_badge(job, chat.queued_jobs))
            .expect("badge");
        assert_eq!(badge.label, "queued (3)");
        assert_eq!(badge.variant, JobStatusVariant::Queued);
    }

    #[test]
    fn no_active_job_shows_no_status() {
        let chat = sample_chat(None, 0);
        assert_eq!(
            chat.active_job
                .as_ref()
                .and_then(|job| job_status_badge(job, chat.queued_jobs)),
            None
        );
    }
}
