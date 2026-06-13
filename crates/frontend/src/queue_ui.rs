use dreamwell_types::*;
use yew::prelude::*;

use crate::api;

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum AppMode {
    Chats,
    Stories,
    Queue,
}

fn job_type_label(job_type: JobType) -> &'static str {
    match job_type {
        JobType::ChatMessage => "chat message",
        JobType::ChatSummarize => "chat summarize",
        JobType::ChatVariableRecheck => "variable recheck",
        JobType::StoryChapterOutline => "chapter outline",
        JobType::StoryProposeChapters => "propose chapters",
        JobType::StoryBeatOutline => "beat outline",
        JobType::StoryProposeBeats => "propose beats",
        JobType::StoryBeatProse => "beat prose",
        JobType::StoryChapterSummarize => "chapter summarize",
        JobType::StoryBeatVariableRecheck => "variable recheck",
    }
}

fn job_target(job: &Job) -> String {
    if let Some(chat_id) = job.chat_id {
        format!("chat {chat_id}")
    } else if let Some(story_id) = job.story_id {
        format!("story {story_id}")
    } else {
        format!("job {}", job.id)
    }
}

fn status_label(status: JobStatus) -> &'static str {
    match status {
        JobStatus::Queued => "queued",
        JobStatus::Running => "running",
        JobStatus::Completed => "completed",
        JobStatus::Failed => "failed",
        JobStatus::Cancelled => "cancelled",
    }
}

#[derive(Properties, PartialEq)]
pub struct TopBarQueueButtonProps {
    pub queue: Option<QueueStatus>,
    pub active: bool,
    pub on_open: Callback<()>,
}

#[function_component(TopBarQueueButton)]
pub fn top_bar_queue_button(props: &TopBarQueueButtonProps) -> Html {
    let total = props
        .queue
        .as_ref()
        .map(|q| q.running.len() + q.queued.len())
        .unwrap_or(0);
    let on_open = props.on_open.clone();
    html! {
        <button
            type="button"
            class={classes!("mode-btn", "mode-btn-icon", props.active.then_some("active"))}
            title="Generation queue"
            aria-label="Generation queue"
            onclick={Callback::from(move |_| on_open.emit(()))}
        >
            <span class="mode-btn-icon-glyph">{"⏱"}</span>
            if total > 0 {
                <span class="mode-btn-badge">{ total }</span>
            }
        </button>
    }
}

#[derive(Properties, PartialEq)]
pub struct QueueBarProps {
    pub queue: Option<QueueStatus>,
    pub on_open: Callback<()>,
}

#[function_component(QueueBar)]
pub fn queue_bar(props: &QueueBarProps) -> Html {
    let Some(queue) = &props.queue else {
        return html! {};
    };
    let total = queue.running.len() + queue.queued.len();
    if total == 0 {
        return html! {};
    }
    let running: Vec<String> = queue.running.iter().map(job_target).collect();
    let waiting = if queue.queued.is_empty() {
        String::new()
    } else {
        format!("{} waiting", queue.queued.len())
    };
    let on_open = props.on_open.clone();
    html! {
        <button type="button" class="queue-bar" onclick={Callback::from(move |_| on_open.emit(()))}>
            <strong>{"Queue: "}</strong>
            if !running.is_empty() {
                <span>{ running.join(", ") }</span>
            }
            if !running.is_empty() && !waiting.is_empty() { <span>{" · "}</span> }
            if !waiting.is_empty() {
                <span>{ waiting }</span>
            }
        </button>
    }
}

#[derive(Properties, PartialEq)]
pub struct QueuePageProps {
    pub queue: Option<QueueStatus>,
    pub on_back: Callback<()>,
    pub on_open_chat: Callback<i64>,
    pub on_open_story: Callback<i64>,
    pub on_queue_change: Callback<QueueStatus>,
}

#[function_component(QueuePage)]
pub fn queue_page(props: &QueuePageProps) -> Html {
    let cancelling = use_state(|| None::<i64>);

    let jobs = props.queue.as_ref().map(|q| {
        let mut all = q.running.clone();
        all.extend(q.queued.clone());
        all
    });

    html! {
        <main class="main queue-page">
            <header class="header">
                <button class="btn secondary" onclick={Callback::from({
                    let on_back = props.on_back.clone();
                    move |_| on_back.emit(())
                })}>{"← Back"}</button>
                <h1 class="header-title">{"Generation queue"}</h1>
                <p class="header-subtitle muted">{"Running and waiting jobs across chats and stories."}</p>
            </header>

            if jobs.as_ref().is_none_or(|j| j.is_empty()) {
                <div class="empty-state muted">
                    <p>{"No jobs in the queue."}</p>
                </div>
            } else {
                <div class="queue-list">
                    { for jobs.unwrap_or_default().iter().map(|job| {
                        let job = job.clone();
                        let cancelling = cancelling.clone();
                        let on_queue_change = props.on_queue_change.clone();
                        let on_open_chat = props.on_open_chat.clone();
                        let on_open_story = props.on_open_story.clone();
                        let is_cancelling = *cancelling == Some(job.id);
                        let target = job_target(&job);
                        let created = job.created_at.format("%Y-%m-%d %H:%M").to_string();
                        html! {
                            <div class="queue-item">
                                <div class="queue-item-main">
                                    <span class={classes!("queue-status", format!("queue-status-{}", status_label(job.status)))}>
                                        { status_label(job.status) }
                                    </span>
                                    <span class="queue-type">{ job_type_label(job.job_type) }</span>
                                    if let Some(chat_id) = job.chat_id {
                                        <button type="button" class="btn link" onclick={{
                                            let on_open_chat = on_open_chat.clone();
                                            Callback::from(move |_| on_open_chat.emit(chat_id))
                                        }}>{ target.clone() }</button>
                                    } else if let Some(story_id) = job.story_id {
                                        <button type="button" class="btn link" onclick={{
                                            let on_open_story = on_open_story.clone();
                                            Callback::from(move |_| on_open_story.emit(story_id))
                                        }}>{ target.clone() }</button>
                                    } else {
                                        <span>{ target.clone() }</span>
                                    }
                                    <span class="muted queue-created">{ created }</span>
                                </div>
                                if matches!(job.status, JobStatus::Queued | JobStatus::Running) {
                                    <button
                                        class="btn secondary"
                                        disabled={is_cancelling}
                                        onclick={Callback::from(move |_| {
                                            let job_id = job.id;
                                            let cancelling = cancelling.clone();
                                            let on_queue_change = on_queue_change.clone();
                                            cancelling.set(Some(job_id));
                                            wasm_bindgen_futures::spawn_local(async move {
                                                let _ = api::cancel_job(job_id).await;
                                                if let Ok(status) = api::get_jobs().await {
                                                    on_queue_change.emit(status);
                                                }
                                                cancelling.set(None);
                                            });
                                        })}
                                    >
                                        { if is_cancelling { "Cancelling…" } else { "Cancel" } }
                                    </button>
                                }
                            </div>
                        }
                    }) }
                </div>
            }
        </main>
    }
}
