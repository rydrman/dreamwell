use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

use dreamwell_types::{Chat, Job, JobType, QueueStatus, Story};
use wasm_bindgen::closure::Closure;
use wasm_bindgen::JsCast;
use web_sys::{Notification, NotificationOptions, NotificationPermission};
use yew::Callback;

use crate::queue_ui::AppMode;

const PREF_KEY: &str = "dreamwell.notifications.enabled";

#[derive(Clone, Debug, PartialEq)]
pub struct TrackedJob {
    pub id: i64,
    pub chat_id: Option<i64>,
    pub story_id: Option<i64>,
    pub job_type: JobType,
}

#[derive(Clone, Copy, PartialEq)]
pub struct ViewContext {
    pub mode: AppMode,
    pub selected_chat_id: Option<i64>,
    pub selected_story_id: Option<i64>,
}

pub struct JobCompletionTracker {
    initialized: bool,
    tracked: HashMap<i64, TrackedJob>,
}

impl JobCompletionTracker {
    pub fn new() -> Self {
        Self {
            initialized: false,
            tracked: HashMap::new(),
        }
    }

    pub fn update(&mut self, queue: &QueueStatus) -> Vec<TrackedJob> {
        let current = active_jobs(queue);
        if !self.initialized {
            self.tracked = current;
            self.initialized = true;
            return Vec::new();
        }

        let completed: Vec<TrackedJob> = self
            .tracked
            .iter()
            .filter(|(id, _)| !current.contains_key(*id))
            .map(|(_, job)| job.clone())
            .collect();

        self.tracked = current;
        completed
    }
}

fn active_jobs(queue: &QueueStatus) -> HashMap<i64, TrackedJob> {
    queue
        .running
        .iter()
        .chain(queue.queued.iter())
        .map(tracked_from_job)
        .map(|job| (job.id, job))
        .collect()
}

fn tracked_from_job(job: &Job) -> TrackedJob {
    TrackedJob {
        id: job.id,
        chat_id: job.chat_id,
        story_id: job.story_id,
        job_type: job.job_type,
    }
}

pub fn is_supported() -> bool {
    web_sys::window().is_some_and(|window| {
        js_sys::Reflect::has(&window, &"Notification".into()).unwrap_or(false)
    })
}

pub fn is_enabled() -> bool {
    if !is_supported() {
        return false;
    }
    local_storage_get(PREF_KEY).is_some_and(|value| value == "true")
}

pub fn set_enabled(enabled: bool) {
    if enabled {
        let _ = local_storage_set(PREF_KEY, "true");
    } else {
        let _ = local_storage_remove(PREF_KEY);
    }
}

pub fn permission_granted() -> bool {
    is_supported() && Notification::permission() == NotificationPermission::Granted
}

pub fn permission_denied() -> bool {
    is_supported() && Notification::permission() == NotificationPermission::Denied
}

pub fn permission_label() -> &'static str {
    if !is_supported() {
        return "not supported in this browser";
    }
    match Notification::permission() {
        NotificationPermission::Granted => "allowed",
        NotificationPermission::Denied => "blocked",
        NotificationPermission::Default => "not requested",
        _ => "unknown",
    }
}

pub async fn request_permission() -> bool {
    if !is_supported() {
        return false;
    }
    if Notification::permission() == NotificationPermission::Granted {
        return true;
    }
    if Notification::permission() == NotificationPermission::Denied {
        return false;
    }
    let Ok(promise) = Notification::request_permission() else {
        return false;
    };
    let Ok(result) = wasm_bindgen_futures::JsFuture::from(promise).await else {
        return false;
    };
    result.as_string().is_some_and(|value| value == "granted")
}

pub fn tab_hidden() -> bool {
    web_sys::window()
        .and_then(|window| window.document())
        .is_some_and(|document| document.hidden())
}

pub fn window_unfocused() -> bool {
    web_sys::window()
        .and_then(|window| window.document())
        .is_some_and(|document| !document.has_focus().unwrap_or(false))
}

pub fn should_notify(job: &TrackedJob, view: ViewContext) -> bool {
    if tab_hidden() || window_unfocused() {
        return true;
    }
    if let Some(chat_id) = job.chat_id {
        if view.mode == AppMode::Chats && view.selected_chat_id == Some(chat_id) {
            return false;
        }
    }
    if let Some(story_id) = job.story_id {
        if view.mode == AppMode::Stories && view.selected_story_id == Some(story_id) {
            return false;
        }
    }
    true
}

pub fn job_type_label(job_type: JobType) -> &'static str {
    match job_type {
        JobType::ChatMessage => "Chat reply",
        JobType::ChatSummarize => "Chat summarize",
        JobType::ChatVariableRecheck => "Variable recheck",
        JobType::StoryChapterOutline => "Chapter outline",
        JobType::StoryProposeChapters => "Propose chapters",
        JobType::StoryBeatOutline => "Beat outline",
        JobType::StoryProposeBeats => "Propose beats",
        JobType::StoryBeatProse => "Beat prose",
        JobType::StoryBeatProseContinue => "Continue prose",
        JobType::StoryBeatMechanical => "Beat mechanical",
        JobType::StoryBeatProseRecheck => "Prose align",
        JobType::StoryChapterSummarize => "Chapter summarize",
        JobType::StoryBeatVariableRecheck => "Variable recheck",
        JobType::GameTurnStructuredAgent => "Structured agent",
        JobType::GameTurnProseRegenerate => "Prose regenerate",
        JobType::GameSceneSummarize => "Scene summarize",
        JobType::GameProseRecheck => "Prose align",
        JobType::GameStateRecheck => "State recheck",
    }
}

pub fn notification_copy(
    job: &TrackedJob,
    chats: &[Chat],
    archived_chats: &[Chat],
    stories: &[Story],
) -> (String, String) {
    let label = job_type_label(job.job_type);
    if let Some(chat_id) = job.chat_id {
        let title = chats
            .iter()
            .chain(archived_chats.iter())
            .find(|chat| chat.id == chat_id)
            .map(|chat| chat.title.clone())
            .unwrap_or_else(|| format!("Chat {chat_id}"));
        return (title, format!("{label} is ready"));
    }
    if let Some(story_id) = job.story_id {
        let title = stories
            .iter()
            .find(|story| story.id == story_id)
            .map(|story| story.title.clone())
            .unwrap_or_else(|| format!("Story {story_id}"));
        return (title, format!("{label} is ready"));
    }
    ("Dreamwell".to_string(), format!("{label} is ready"))
}

#[derive(Clone)]
pub struct NotificationActions {
    pub open_chat: Callback<i64>,
    pub open_story: Callback<i64>,
}

thread_local! {
    static ACTIONS: RefCell<Option<Rc<NotificationActions>>> = const { RefCell::new(None) };
}

pub fn set_actions(actions: Rc<NotificationActions>) {
    ACTIONS.with(|slot| *slot.borrow_mut() = Some(actions));
}

pub fn clear_actions() {
    ACTIONS.with(|slot| *slot.borrow_mut() = None);
}

pub fn notify_completion(job: &TrackedJob, title: &str, body: &str) {
    show_notification(title, body, Some(job));
}

pub fn show_test_notification() {
    show_notification("Dreamwell", "Notifications are working.", None);
}

fn show_notification(title: &str, body: &str, job: Option<&TrackedJob>) {
    if !is_enabled() || !permission_granted() {
        return;
    }

    let options = NotificationOptions::new();
    options.set_body(body);
    if let Some(job) = job {
        options.set_tag(&format!("dreamwell-job-{}", job.id));
    } else {
        options.set_tag("dreamwell-test");
    }

    let Ok(notification) = Notification::new_with_options(title, &options) else {
        return;
    };

    let notification_for_click = notification.clone();
    let job_for_click = job.cloned();
    let onclick = Closure::wrap(Box::new(move |_event: web_sys::Event| {
        if let Some(window) = web_sys::window() {
            let _ = window.focus();
        }
        if let Some(job) = job_for_click.as_ref() {
            ACTIONS.with(|slot| {
                if let Some(actions) = slot.borrow().as_ref() {
                    if let Some(chat_id) = job.chat_id {
                        actions.open_chat.emit(chat_id);
                    } else if let Some(story_id) = job.story_id {
                        actions.open_story.emit(story_id);
                    }
                }
            });
        }
        notification_for_click.close();
    }) as Box<dyn FnMut(web_sys::Event)>);
    notification.set_onclick(Some(onclick.as_ref().unchecked_ref()));
    onclick.forget();
}

fn local_storage_get(key: &str) -> Option<String> {
    web_sys::window()?
        .local_storage()
        .ok()
        .flatten()?
        .get_item(key)
        .ok()
        .flatten()
}

fn local_storage_set(key: &str, value: &str) -> bool {
    web_sys::window()
        .and_then(|window| window.local_storage().ok().flatten())
        .and_then(|storage| storage.set_item(key, value).ok())
        .is_some()
}

fn local_storage_remove(key: &str) -> bool {
    web_sys::window()
        .and_then(|window| window.local_storage().ok().flatten())
        .and_then(|storage| storage.remove_item(key).ok())
        .is_some()
}
