mod api;
mod generation_ui;
mod install;
mod markdown;
mod notifications;
mod queue_ui;
mod router;
mod sidebar;
mod stories_ui;
mod title_editor;
mod variables;

use std::cell::RefCell;
use std::rc::Rc;

thread_local! {
    static APP: RefCell<Option<yew::AppHandle<App>>> = const { RefCell::new(None) };
}

use wasm_bindgen::closure::Closure;
use wasm_bindgen::JsCast;

use dreamwell_types::*;
use generation_ui::{
    composer_notice, generation_error_message, GenerationNotice, GenerationStatusBar,
};
use gloo_timers::callback::{Interval, Timeout};
use install::{InstallButton, InstallKind, InstallSettings};
use queue_ui::{AppMode, QueueBar, QueuePage, TopBarQueueButton};
use router::{use_router, AppRoute, Overlay, StoryNav};
use sidebar::AppSidebar;
use stories_ui::StoriesShell;
use title_editor::TitleEditor;
use web_sys::{DomRect, Element, HtmlElement, HtmlInputElement};
use yew::prelude::*;

fn is_mobile_viewport() -> bool {
    web_sys::window()
        .and_then(|window| window.match_media("(max-width: 768px)").ok().flatten())
        .map(|mq| mq.matches())
        .unwrap_or(false)
}

fn window_scroll_y() -> f64 {
    web_sys::window()
        .and_then(|window| window.scroll_y().ok())
        .unwrap_or(0.0)
}

fn scroll_chat_view_to_bottom(messages_el: Option<&HtmlElement>) {
    let Some(window) = web_sys::window() else {
        return;
    };
    if is_mobile_viewport() {
        let height = window
            .document()
            .map(|document| {
                document
                    .document_element()
                    .map(|root| root.scroll_height())
                    .or_else(|| document.body().map(|body| body.scroll_height()))
                    .unwrap_or(0)
            })
            .unwrap_or(0) as f64;
        window.scroll_to_with_x_and_y(0.0, height);
    } else if let Some(el) = messages_el {
        el.set_scroll_top(el.scroll_height());
    }
}

fn chat_id_from_route(route: &AppRoute) -> Option<i64> {
    match route {
        AppRoute::Chats { chat_id, .. } => *chat_id,
        _ => None,
    }
}

fn story_id_from_route(route: &AppRoute) -> Option<i64> {
    match route {
        AppRoute::Stories { story_id, .. } => *story_id,
        _ => None,
    }
}

fn sidebar_open_from_route(route: &AppRoute) -> bool {
    matches!(
        route,
        AppRoute::Chats { sidebar: true, .. } | AppRoute::Stories { sidebar: true, .. }
    )
}

#[derive(Clone, PartialEq)]
struct ChatHeaderSnapshot {
    id: i64,
    title: String,
    character_name: String,
    character_id: i64,
}

fn chat_snapshot_storage_key(chat_id: i64, field: &str) -> String {
    format!("dreamwell.chat.{chat_id}.{field}")
}

fn session_storage() -> Option<web_sys::Storage> {
    web_sys::window().and_then(|w| w.session_storage().ok().flatten())
}

fn cache_chat_snapshot(chat: &Chat) {
    let Some(storage) = session_storage() else {
        return;
    };
    let _ = storage.set_item(&chat_snapshot_storage_key(chat.id, "title"), &chat.title);
    let _ = storage.set_item(
        &chat_snapshot_storage_key(chat.id, "character_name"),
        &chat.character_name,
    );
    let _ = storage.set_item(
        &chat_snapshot_storage_key(chat.id, "character_id"),
        &chat.character_id.to_string(),
    );
}

fn cached_chat_snapshot(chat_id: i64) -> Option<ChatHeaderSnapshot> {
    let storage = session_storage()?;
    let title = storage
        .get_item(&chat_snapshot_storage_key(chat_id, "title"))
        .ok()
        .flatten()
        .filter(|title| !title.is_empty())
        .or_else(|| {
            storage
                .get_item(&format!("dreamwell.chat.{chat_id}.title"))
                .ok()
                .flatten()
                .filter(|title| !title.is_empty())
        })?;
    let character_name = storage
        .get_item(&chat_snapshot_storage_key(chat_id, "character_name"))
        .ok()
        .flatten()
        .unwrap_or_default();
    let character_id = storage
        .get_item(&chat_snapshot_storage_key(chat_id, "character_id"))
        .ok()
        .flatten()
        .and_then(|id| id.parse().ok())
        .unwrap_or(0);
    Some(ChatHeaderSnapshot {
        id: chat_id,
        title,
        character_name,
        character_id,
    })
}

fn active_chat_header(
    selected: &Option<Chat>,
    selected_chat_id: Option<i64>,
) -> Option<ChatHeaderSnapshot> {
    if let Some(chat) = selected {
        cache_chat_snapshot(chat);
        return Some(ChatHeaderSnapshot {
            id: chat.id,
            title: chat.title.clone(),
            character_name: chat.character_name.clone(),
            character_id: chat.character_id,
        });
    }
    if let Some(snapshot) = selected_chat_id.and_then(cached_chat_snapshot) {
        return Some(snapshot);
    }
    selected_chat_id.map(|id| ChatHeaderSnapshot {
        id,
        title: "Chat".to_string(),
        character_name: String::new(),
        character_id: 0,
    })
}

const CHATS_LIST_CACHE_KEY: &str = "dreamwell.chat_list";
const ARCHIVED_CHATS_LIST_CACHE_KEY: &str = "dreamwell.archived_chat_list";

fn load_cached_chat_list(key: &str) -> Vec<Chat> {
    session_storage()
        .and_then(|s| s.get_item(key).ok().flatten())
        .and_then(|json| serde_json::from_str::<Vec<Chat>>(&json).ok())
        .unwrap_or_default()
}

fn load_cached_chats() -> Vec<Chat> {
    sort_chats(load_cached_chat_list(CHATS_LIST_CACHE_KEY))
}

fn load_cached_archived_chats() -> Vec<Chat> {
    sort_archived_chats(load_cached_chat_list(ARCHIVED_CHATS_LIST_CACHE_KEY))
}

fn chat_list_for_cache(chats: &[Chat]) -> Vec<Chat> {
    chats
        .iter()
        .map(|chat| Chat {
            active_job: None,
            queued_jobs: 0,
            ..chat.clone()
        })
        .collect()
}

fn cache_chat_list(key: &str, chats: &[Chat]) {
    let Some(storage) = session_storage() else {
        return;
    };
    let cached = chat_list_for_cache(chats);
    if let Ok(json) = serde_json::to_string(&cached) {
        let _ = storage.set_item(key, &json);
    }
}

fn publish_chats(chats: &UseStateHandle<Vec<Chat>>, next: Vec<Chat>) {
    let next = sort_chats(next);
    if **chats != next {
        cache_chat_list(CHATS_LIST_CACHE_KEY, &next);
        chats.set(next);
    }
}

/// Merge one chat into the sidebar list without re-sorting.
///
/// Stream updates touch `updated_at` frequently; re-sorting on every payload
/// makes the list jump even when the visible order should stay put.
fn update_chat_in_list(chats: &UseStateHandle<Vec<Chat>>, updated: Chat) {
    let current = (**chats).clone();
    let next: Vec<Chat> = current
        .iter()
        .map(|chat| {
            if chat.id == updated.id {
                updated.clone()
            } else {
                chat.clone()
            }
        })
        .collect();
    if current != next {
        cache_chat_list(CHATS_LIST_CACHE_KEY, &sort_chats(next.clone()));
        chats.set(next);
    }
}

fn publish_archived_chats(archived_chats: &UseStateHandle<Vec<Chat>>, next: Vec<Chat>) {
    let next = sort_archived_chats(next);
    if **archived_chats != next {
        cache_chat_list(ARCHIVED_CHATS_LIST_CACHE_KEY, &next);
        archived_chats.set(next);
    }
}

#[function_component(App)]
fn app() -> Html {
    let router = use_router();
    let route = router.route();
    let mode = route.mode();
    let selected_chat_id = chat_id_from_route(&route);
    let _selected_story_id = story_id_from_route(&route);
    let chats = use_state(load_cached_chats);
    let stories = use_state(Vec::<Story>::new);
    let archived_chats = use_state(load_cached_archived_chats);
    let characters = use_state(Vec::<Character>::new);
    let messages = use_state(Vec::<Message>::new);
    let messages_loading = use_state(|| false);
    let settings = use_state(|| None::<Settings>);
    let queue = use_state(|| None::<QueueStatus>);
    let loading = use_state(|| true);
    let refresh_generation = use_state(|| 0u32);
    let chat_stream_nudge = use_mut_ref(|| None::<api::StreamNudge>);
    let summarize_watch = use_mut_ref(|| None::<i64>);
    let job_tracker = use_mut_ref(notifications::JobCompletionTracker::new);
    let install_kind = use_state(install::install_kind);
    let install_ui_tick = use_state(|| 0u32);

    let refresh_install_ui = {
        let install_kind = install_kind.clone();
        let install_ui_tick = install_ui_tick.clone();
        Callback::from(move |_| {
            install_kind.set(install::install_kind());
            install_ui_tick.set(*install_ui_tick + 1);
        })
    };

    {
        let install_kind = install_kind.clone();
        let refresh_install_ui = refresh_install_ui.clone();
        use_effect_with((), move |_| {
            install::init(Callback::from(move |_| {
                refresh_install_ui.emit(());
            }));
            install_kind.set(install::install_kind());
            || ()
        });
    }

    let show_install = {
        let _ = *install_ui_tick;
        matches!(install::install_kind(), InstallKind::NativePrompt)
    };

    let navigate = {
        let router = router.clone();
        Callback::from(move |(next, push): (AppRoute, bool)| router.navigate(next, push))
    };

    let close_overlay = {
        let router = router.clone();
        let route = route.clone();
        Callback::from(move |_| {
            if route.overlay().is_some() {
                router.back();
            }
        })
    };

    {
        let chats = chats.clone();
        let archived_chats = archived_chats.clone();
        let characters = characters.clone();
        let settings = settings.clone();
        let stories = stories.clone();
        let loading = loading.clone();
        let router = router.clone();
        use_effect_with((), move |_| {
            wasm_bindgen_futures::spawn_local(async move {
                let mut chat_list = Vec::<Chat>::new();
                if let Ok(list) = api::list_chats().await {
                    chat_list = sort_chats(list);
                    publish_chats(&chats, chat_list.clone());
                }
                if let Ok(list) = api::list_archived_chats().await {
                    publish_archived_chats(&archived_chats, list);
                }
                if let Ok(list) = api::list_characters().await {
                    characters.set(list);
                }
                if let Ok(s) = api::get_settings().await {
                    settings.set(Some(s));
                }
                if let Ok(list) = api::list_stories().await {
                    stories.set(list);
                }
                loading.set(false);

                if !chat_list.is_empty() {
                    match router.route() {
                        AppRoute::Chats {
                            chat_id: None,
                            overlay: None,
                            sidebar: false,
                        } => {
                            if let Some(id) = chat_list.first().map(|c| c.id) {
                                router.navigate(
                                    AppRoute::Chats {
                                        chat_id: Some(id),
                                        overlay: None,
                                        sidebar: false,
                                    },
                                    false,
                                );
                            }
                        }
                        AppRoute::Chats {
                            chat_id: Some(id),
                            overlay,
                            sidebar,
                        } if !chat_list.iter().any(|c| c.id == id) => {
                            router.navigate(
                                AppRoute::Chats {
                                    chat_id: chat_list.first().map(|c| c.id),
                                    overlay,
                                    sidebar,
                                },
                                false,
                            );
                        }
                        _ => {}
                    }
                }
            });
            || ()
        });
    }

    {
        let messages = messages.clone();
        let messages_loading = messages_loading.clone();
        let chats = chats.clone();
        let chat_stream_nudge = chat_stream_nudge.clone();
        let summarize_watch = summarize_watch.clone();
        let refresh_generation = *refresh_generation;
        use_effect_with(
            (selected_chat_id, refresh_generation),
            move |(chat_id, _)| {
                let mut stream_holder = None::<api::ChatStream>;
                *chat_stream_nudge.borrow_mut() = None;
                *summarize_watch.borrow_mut() = None;
                if let Some(chat_id) = *chat_id {
                    messages_loading.set(true);
                    let messages_for_fetch = messages.clone();
                    let messages_loading_for_fetch = messages_loading.clone();
                    let chats = chats.clone();
                    wasm_bindgen_futures::spawn_local(async move {
                        if let Ok(msgs) = api::get_messages(chat_id).await {
                            messages_for_fetch.set(msgs);
                        }
                        messages_loading_for_fetch.set(false);
                    });
                    let summarize_watch = summarize_watch.clone();
                    let stream = api::ChatStream::new(chat_id, move |payload| {
                        if let Some(marker_id) = *summarize_watch.borrow() {
                            let completed = payload.messages.iter().any(|message| {
                                message.id == marker_id
                                    && message.is_summary
                                    && message
                                        .content
                                        .starts_with("**Earlier conversation summarized**")
                            });
                            let vanished = !payload
                                .messages
                                .iter()
                                .any(|message| message.id == marker_id);
                            if completed {
                                *summarize_watch.borrow_mut() = None;
                            } else if vanished {
                                *summarize_watch.borrow_mut() = None;
                                if let Some(window) = web_sys::window() {
                                    let _ = window.alert_with_message(
                                        "Summarization failed. Check that your inference server is reachable and try again.",
                                    );
                                }
                            }
                        }
                        if let Some(marker_id) = summarize_placeholder_id(&payload.messages) {
                            *summarize_watch.borrow_mut() = Some(marker_id);
                        }
                        messages.set(payload.messages.clone());
                        messages_loading.set(false);
                        update_chat_in_list(&chats, payload.chat.clone());
                    });
                    *chat_stream_nudge.borrow_mut() = Some(stream.nudge());
                    stream_holder = Some(stream);
                } else {
                    messages.set(vec![]);
                    messages_loading.set(false);
                }
                move || {
                    *chat_stream_nudge.borrow_mut() = None;
                    drop(stream_holder);
                }
            },
        );
    }

    {
        let chats = chats.clone();
        let archived_chats = archived_chats.clone();
        let queue = queue.clone();
        let chat_stream_nudge = chat_stream_nudge.clone();
        use_effect_with((), move |_| {
            let chats = chats.clone();
            let archived_chats = archived_chats.clone();
            let queue = queue.clone();
            let chat_stream_nudge = chat_stream_nudge.clone();
            let resume: Rc<dyn Fn()> = Rc::new(move || {
                if let Some(nudge) = chat_stream_nudge.borrow().clone() {
                    nudge.reconnect();
                }
                let chats = chats.clone();
                let archived_chats = archived_chats.clone();
                let queue = queue.clone();
                wasm_bindgen_futures::spawn_local(async move {
                    if let Ok(status) = api::get_queue().await {
                        queue.set(Some(status));
                    }
                    if let Ok(list) = api::list_chats().await {
                        publish_chats(&chats, list);
                    }
                    if let Ok(list) = api::list_archived_chats().await {
                        publish_archived_chats(&archived_chats, list);
                    }
                });
            });

            let resume_visibility = resume.clone();
            let visibility_callback = Closure::wrap(Box::new(move |_event: web_sys::Event| {
                if web_sys::window()
                    .and_then(|window| window.document())
                    .is_some_and(|document| {
                        document.visibility_state() == web_sys::VisibilityState::Visible
                    })
                {
                    resume_visibility();
                }
            }) as Box<dyn FnMut(_)>);

            let document = web_sys::window().and_then(|window| window.document());
            if let Some(document) = document.as_ref() {
                let _ = document.add_event_listener_with_callback(
                    "visibilitychange",
                    visibility_callback.as_ref().unchecked_ref(),
                );
            }

            let resume_online = resume.clone();
            let online_callback = Closure::wrap(Box::new(move |_event: web_sys::Event| {
                resume_online();
            }) as Box<dyn FnMut(_)>);

            let window = web_sys::window();
            if let Some(window) = window.as_ref() {
                let _ = window.add_event_listener_with_callback(
                    "online",
                    online_callback.as_ref().unchecked_ref(),
                );
            }

            move || {
                if let Some(document) = document.as_ref() {
                    let _ = document.remove_event_listener_with_callback(
                        "visibilitychange",
                        visibility_callback.as_ref().unchecked_ref(),
                    );
                }
                if let Some(window) = window.as_ref() {
                    let _ = window.remove_event_listener_with_callback(
                        "online",
                        online_callback.as_ref().unchecked_ref(),
                    );
                }
            }
        });
    }

    let load_messages_for_chat = {
        let messages = messages.clone();
        let messages_loading = messages_loading.clone();
        Callback::from(move |chat_id: i64| {
            let messages = messages.clone();
            let messages_loading = messages_loading.clone();
            messages_loading.set(true);
            wasm_bindgen_futures::spawn_local(async move {
                if let Ok(msgs) = api::get_messages(chat_id).await {
                    messages.set(msgs);
                }
                messages_loading.set(false);
            });
        })
    };

    {
        let router = router.clone();
        let load_messages_for_chat = load_messages_for_chat.clone();
        use_effect_with((), move |_| {
            let actions = Rc::new(notifications::NotificationActions {
                open_chat: Callback::from({
                    let router = router.clone();
                    let load_messages_for_chat = load_messages_for_chat.clone();
                    move |chat_id| {
                        router.navigate(
                            AppRoute::Chats {
                                chat_id: Some(chat_id),
                                overlay: None,
                                sidebar: false,
                            },
                            true,
                        );
                        load_messages_for_chat.emit(chat_id);
                    }
                }),
                open_story: Callback::from({
                    let router = router.clone();
                    move |story_id| {
                        router.navigate(
                            AppRoute::Stories {
                                story_id: Some(story_id),
                                nav: StoryNav::None,
                                overlay: None,
                                sidebar: false,
                            },
                            true,
                        );
                    }
                }),
            });
            notifications::set_actions(actions);
            || notifications::clear_actions()
        });
    }

    {
        let queue = queue.clone();
        let chats = chats.clone();
        let archived_chats = archived_chats.clone();
        let stories = stories.clone();
        let router = router.clone();
        let job_tracker = job_tracker.clone();
        use_effect_with((), move |_| {
            let queue = queue.clone();
            let chats = chats.clone();
            let archived_chats = archived_chats.clone();
            let stories = stories.clone();
            let router = router.clone();
            let job_tracker = job_tracker.clone();
            let poll_ms = if notifications::is_enabled() {
                1500
            } else {
                3000
            };
            let handle = Interval::new(poll_ms, move || {
                let queue = queue.clone();
                let chats = chats.clone();
                let archived_chats = archived_chats.clone();
                let stories = stories.clone();
                let current = router.route();
                let view = notifications::ViewContext {
                    mode: current.mode(),
                    selected_chat_id: chat_id_from_route(&current),
                    selected_story_id: story_id_from_route(&current),
                };
                let notifications_on = notifications::is_enabled();
                let job_tracker = job_tracker.clone();
                wasm_bindgen_futures::spawn_local(async move {
                    let status = api::get_queue().await.ok();
                    let chat_list = api::list_chats().await.ok().map(sort_chats);
                    let archived_list = api::list_archived_chats()
                        .await
                        .ok()
                        .map(sort_archived_chats);
                    let story_list = api::list_stories().await.ok();

                    if let Some(status) = status.as_ref() {
                        let completed = job_tracker.borrow_mut().update(status);
                        if notifications_on {
                            let fallback_chats = (*chats).clone();
                            let fallback_archived = (*archived_chats).clone();
                            let fallback_stories = (*stories).clone();
                            let chats_for_copy = chat_list.as_deref().unwrap_or(&fallback_chats);
                            let archived_for_copy =
                                archived_list.as_deref().unwrap_or(&fallback_archived);
                            let stories_for_copy =
                                story_list.as_deref().unwrap_or(&fallback_stories);
                            for job in completed {
                                if notifications::should_notify(&job, view) {
                                    let (title, body) = notifications::notification_copy(
                                        &job,
                                        chats_for_copy,
                                        archived_for_copy,
                                        stories_for_copy,
                                    );
                                    notifications::notify_completion(&job, &title, &body);
                                }
                            }
                        }
                    }
                    if let Some(status) = status {
                        queue.set(Some(status));
                    }
                    if let Some(list) = chat_list {
                        publish_chats(&chats, list);
                    }
                    if let Some(list) = archived_list {
                        publish_archived_chats(&archived_chats, list);
                    }
                    if let Some(list) = story_list {
                        stories.set(list);
                    }
                });
            });
            move || drop(handle)
        });
    }

    let open_queue = {
        let navigate = navigate.clone();
        Callback::from(move |_| navigate.emit((AppRoute::Queue { overlay: None }, true)))
    };

    let open_settings = {
        let navigate = navigate.clone();
        let route = route.clone();
        Callback::from(move |_| {
            navigate.emit((route.clone().with_overlay(Overlay::Settings), true));
        })
    };

    let toggle_sidebar = {
        let navigate = navigate.clone();
        let route = route.clone();
        Callback::from(move |_| {
            let open = sidebar_open_from_route(&route);
            navigate.emit((route.clone().with_sidebar(!open), true));
        })
    };

    let close_sidebar = {
        let navigate = navigate.clone();
        let route = route.clone();
        Callback::from(move |_| {
            navigate.emit((route.clone().with_sidebar(false), false));
        })
    };

    if *loading && mode == AppMode::Chats {
        return html! { <div class="loading-screen muted">{"Loading Dreamwell…"}</div> };
    }

    let on_mode = {
        let navigate = navigate.clone();
        let route = route.clone();
        Callback::from(move |next_mode: AppMode| {
            let next = match next_mode {
                AppMode::Chats => AppRoute::Chats {
                    chat_id: chat_id_from_route(&route),
                    overlay: None,
                    sidebar: false,
                },
                AppMode::Stories => AppRoute::Stories {
                    story_id: story_id_from_route(&route),
                    nav: match &route {
                        AppRoute::Stories { nav, .. } => *nav,
                        _ => StoryNav::None,
                    },
                    overlay: None,
                    sidebar: false,
                },
                AppMode::Queue => AppRoute::Queue { overlay: None },
            };
            navigate.emit((next, true));
        })
    };

    if mode == AppMode::Queue {
        return html! {
            <div class="app-layout">
                <ModeBar
                    mode={mode}
                    queue={(*queue).clone()}
                    settings_open={route.overlay() == Some(Overlay::Settings)}
                    show_sidebar_toggle={false}
                    sidebar_open={false}
                    show_install={show_install}
                    on_install_change={refresh_install_ui.clone()}
                    on_toggle_sidebar={toggle_sidebar.clone()}
                    on_open_settings={open_settings.clone()}
                    on_open_queue={open_queue.clone()}
                    on_close_overlay={close_overlay.clone()}
                />
                if route.overlay() == Some(Overlay::Settings) {
                    <SettingsOverlay
                        settings={settings.clone()}
                        install_kind={*install_kind}
                        on_install_change={refresh_install_ui.clone()}
                        on_close={close_overlay.clone()}
                    />
                }
                <QueuePage
                    queue={(*queue).clone()}
                    on_back={Callback::from({
                        let router = router.clone();
                        move |_| router.back()
                    })}
                    on_open_chat={Callback::from({
                        let navigate = navigate.clone();
                        let load_messages_for_chat = load_messages_for_chat.clone();
                        move |chat_id| {
                            navigate.emit((
                                AppRoute::Chats {
                                    chat_id: Some(chat_id),
                                    overlay: None,
                                    sidebar: false,
                                },
                                true,
                            ));
                            load_messages_for_chat.emit(chat_id);
                        }
                    })}
                    on_open_story={Callback::from({
                        let navigate = navigate.clone();
                        move |story_id| {
                            navigate.emit((
                                AppRoute::Stories {
                                    story_id: Some(story_id),
                                    nav: StoryNav::None,
                                    overlay: None,
                                    sidebar: false,
                                },
                                true,
                            ));
                        }
                    })}
                    on_queue_change={Callback::from({
                        let queue = queue.clone();
                        move |status| queue.set(Some(status))
                    })}
                />
            </div>
        };
    }

    let selected = selected_chat_id.and_then(|id| chats.iter().find(|c| c.id == id).cloned());
    let active_header = active_chat_header(&selected, selected_chat_id);
    let mobile_chrome_visible = use_state(|| false);

    {
        let mobile_chrome_visible = mobile_chrome_visible.clone();
        use_effect_with((mode, selected_chat_id), move |(mode, chat_id)| {
            mobile_chrome_visible.set(false);
            let mode = *mode;
            let chat_id = *chat_id;
            let mobile_chrome_visible = mobile_chrome_visible.clone();
            let last_scroll_y = Rc::new(RefCell::new(window_scroll_y()));
            let scroll_callback = Closure::wrap(Box::new(move |_event: web_sys::Event| {
                if mode != AppMode::Chats || chat_id.is_none() || !is_mobile_viewport() {
                    return;
                }
                let current = window_scroll_y();
                let mut last = last_scroll_y.borrow_mut();
                if current <= 0.0 || current < *last {
                    mobile_chrome_visible.set(true);
                } else if current > *last {
                    mobile_chrome_visible.set(false);
                }
                *last = current;
            }) as Box<dyn FnMut(_)>);

            let window = web_sys::window();
            if let Some(window) = window.as_ref() {
                let _ = window.add_event_listener_with_callback(
                    "scroll",
                    scroll_callback.as_ref().unchecked_ref(),
                );
            }

            move || {
                if let Some(window) = window.as_ref() {
                    let _ = window.remove_event_listener_with_callback(
                        "scroll",
                        scroll_callback.as_ref().unchecked_ref(),
                    );
                }
            }
        });
    }

    let sidebar_open = sidebar_open_from_route(&route);
    let overlay = route.overlay();
    let picker_open = overlay == Some(Overlay::NewChat);

    let bump_stream = {
        let refresh_generation = refresh_generation.clone();
        Callback::from(move |_| refresh_generation.set(*refresh_generation + 1))
    };

    let start_chat = {
        let chats = chats.clone();
        let navigate = navigate.clone();
        let load_messages_for_chat = load_messages_for_chat.clone();
        Callback::from(move |(character_id, character_name): (i64, String)| {
            let title = default_chat_title(&character_name, character_id, &chats);
            let chats = chats.clone();
            let navigate = navigate.clone();
            let load_messages_for_chat = load_messages_for_chat.clone();
            wasm_bindgen_futures::spawn_local(async move {
                match api::create_chat(&title, character_id).await {
                    Ok(chat) => {
                        if let Ok(list) = api::list_chats().await {
                            publish_chats(&chats, list);
                        }
                        navigate.emit((
                            AppRoute::Chats {
                                chat_id: Some(chat.id),
                                overlay: None,
                                sidebar: false,
                            },
                            true,
                        ));
                        load_messages_for_chat.emit(chat.id);
                    }
                    Err(err) => {
                        if let Some(window) = web_sys::window() {
                            let _ =
                                window.alert_with_message(&format!("Could not create chat: {err}"));
                        }
                    }
                }
            });
        })
    };

    let open_overlay = {
        let navigate = navigate.clone();
        let route = route.clone();
        Callback::from(move |overlay: Overlay| {
            navigate.emit((route.clone().with_overlay(overlay), true));
        })
    };

    html! {
        <div class={classes!(
            "app-layout",
            (mode == AppMode::Chats).then_some("app-layout--chat-scroll-chrome"),
            (*mobile_chrome_visible).then_some("mobile-chrome-visible"),
        )}>
            <ModeBar
                mode={mode}
                queue={(*queue).clone()}
                settings_open={overlay == Some(Overlay::Settings)}
                on_toggle_sidebar={toggle_sidebar.clone()}
                show_sidebar_toggle={true}
                sidebar_open={sidebar_open}
                show_install={show_install}
                on_install_change={refresh_install_ui.clone()}
                on_open_settings={open_settings.clone()}
                on_open_queue={open_queue.clone()}
                on_close_overlay={close_overlay.clone()}
            />
            if picker_open {
                <CharacterPickerModal
                    characters={(*characters).clone()}
                    on_close={close_overlay.clone()}
                    on_select={Callback::from({
                        let start_chat = start_chat.clone();
                        move |character: Character| {
                            start_chat.emit((character.id, character.name.clone()));
                        }
                    })}
                />
            }
            if overlay == Some(Overlay::Settings) {
                <SettingsOverlay
                    settings={settings.clone()}
                    install_kind={*install_kind}
                    on_install_change={refresh_install_ui.clone()}
                    on_close={close_overlay.clone()}
                />
            }
            if mode == AppMode::Chats
                && (overlay == Some(Overlay::Character) || overlay == Some(Overlay::Variables))
            {
                <ChatPanelOverlay
                    overlay={overlay.unwrap()}
                    chat_id={selected_chat_id}
                    character_id={active_header.as_ref().and_then(|h| {
                        (h.character_id != 0).then_some(h.character_id)
                    })}
                    messages={(*messages).clone()}
                    on_close={close_overlay.clone()}
                    on_character_change={Callback::from({
                        let chats = chats.clone();
                        move |(chat_id, character_id)| {
                            let chats = chats.clone();
                            wasm_bindgen_futures::spawn_local(async move {
                                let payload = ChatUpdate {
                                    character_id: Some(character_id),
                                    title: None,
                                    summary: None,
                                };
                                let _ = api::update_chat(chat_id, &payload).await;
                                if let Ok(list) = api::list_chats().await {
                                    publish_chats(&chats, list);
                                }
                            });
                        }
                    })}
                    on_start_chat={start_chat.clone()}
                    on_chat_created={Callback::from({
                        let chats = chats.clone();
                        let navigate = navigate.clone();
                        let load_messages_for_chat = load_messages_for_chat.clone();
                        move |chat_id| {
                            let chats = chats.clone();
                            let navigate = navigate.clone();
                            let load_messages_for_chat = load_messages_for_chat.clone();
                            wasm_bindgen_futures::spawn_local(async move {
                                if let Ok(list) = api::list_chats().await {
                                    publish_chats(&chats, list);
                                }
                                navigate.emit((
                                    AppRoute::Chats {
                                        chat_id: Some(chat_id),
                                        overlay: None,
                                        sidebar: false,
                                    },
                                    true,
                                ));
                                load_messages_for_chat.emit(chat_id);
                            });
                        }
                    })}
                    on_chats_changed={Callback::from({
                        let chats = chats.clone();
                        let archived_chats = archived_chats.clone();
                        let navigate = navigate.clone();
                        let route = route.clone();
                        move |_| {
                            let chats = chats.clone();
                            let archived_chats = archived_chats.clone();
                            let navigate = navigate.clone();
                            let route = route.clone();
                            wasm_bindgen_futures::spawn_local(async move {
                                if let Ok(list) = api::list_chats().await {
                                    let list = sort_chats(list);
                                    if let AppRoute::Chats {
                                        chat_id: Some(id),
                                        ..
                                    } = route
                                    {
                                        if !list.iter().any(|c| c.id == id) {
                                            navigate.emit((
                                                AppRoute::Chats {
                                                    chat_id: list.first().map(|c| c.id),
                                                    overlay: None,
                                                    sidebar: false,
                                                },
                                                false,
                                            ));
                                        }
                                    }
                                    publish_chats(&chats, list);
                                }
                                if let Ok(list) = api::list_archived_chats().await {
                                    publish_archived_chats(&archived_chats, list);
                                }
                            });
                        }
                    })}
                    on_characters_changed={Callback::from({
                        let characters = characters.clone();
                        move |_| {
                            let characters = characters.clone();
                            wasm_bindgen_futures::spawn_local(async move {
                                if let Ok(list) = api::list_characters().await {
                                    characters.set(list);
                                }
                            });
                        }
                    })}
                />
            }
            if sidebar_open {
                <div class="drawer-backdrop" onclick={close_sidebar.clone()} />
            }
            <div class={classes!(
                "app-shell",
                sidebar_open.then_some("pane-sidebar"),
            )}>
            <AppSidebar
                mode={mode}
                chats={(*chats).clone()}
                archived_chats={(*archived_chats).clone()}
                stories={(*stories).clone()}
                selected_chat_id={selected_chat_id}
                selected_story_id={story_id_from_route(&route)}
                on_mode={on_mode.clone()}
                on_select_chat={Callback::from({
                    let navigate = navigate.clone();
                    let load_messages_for_chat = load_messages_for_chat.clone();
                    move |id| {
                        navigate.emit((
                            AppRoute::Chats {
                                chat_id: Some(id),
                                overlay: None,
                                sidebar: false,
                            },
                            true,
                        ));
                        load_messages_for_chat.emit(id);
                    }
                })}
                on_new_chat={Callback::from({
                    let navigate = navigate.clone();
                    let route = route.clone();
                    move |_| {
                        let next = AppRoute::Chats {
                            chat_id: chat_id_from_route(&route),
                            overlay: Some(Overlay::NewChat),
                            sidebar: false,
                        };
                        navigate.emit((next, true));
                    }
                })}
                on_archive_chat={Callback::from({
                    let chats = chats.clone();
                    let archived_chats = archived_chats.clone();
                    let navigate = navigate.clone();
                    let route = route.clone();
                    move |id| {
                        let chats = chats.clone();
                        let archived_chats = archived_chats.clone();
                        let navigate = navigate.clone();
                        let route = route.clone();
                        wasm_bindgen_futures::spawn_local(async move {
                            let _ = api::archive_chat(id).await;
                            if let Ok(list) = api::list_chats().await {
                                let list = sort_chats(list);
                                if chat_id_from_route(&route) == Some(id) {
                                    navigate.emit((
                                        AppRoute::Chats {
                                            chat_id: list.first().map(|c| c.id),
                                            overlay: None,
                                            sidebar: false,
                                        },
                                        false,
                                    ));
                                }
                                publish_chats(&chats, list);
                            }
                            if let Ok(list) = api::list_archived_chats().await {
                                publish_archived_chats(&archived_chats, list);
                            }
                        });
                    }
                })}
                on_restore_chat={Callback::from({
                    let chats = chats.clone();
                    let archived_chats = archived_chats.clone();
                    let navigate = navigate.clone();
                    move |id| {
                        let chats = chats.clone();
                        let archived_chats = archived_chats.clone();
                        let navigate = navigate.clone();
                        wasm_bindgen_futures::spawn_local(async move {
                            if api::restore_chat(id).await.is_ok() {
                                if let Ok(list) = api::list_chats().await {
                                    publish_chats(&chats, list);
                                }
                                if let Ok(list) = api::list_archived_chats().await {
                                    publish_archived_chats(&archived_chats, list);
                                }
                                navigate.emit((
                                    AppRoute::Chats {
                                        chat_id: Some(id),
                                        overlay: None,
                                        sidebar: false,
                                    },
                                    true,
                                ));
                            }
                        });
                    }
                })}
                on_permanent_delete_chat={Callback::from({
                    let archived_chats = archived_chats.clone();
                    move |id| {
                        let archived_chats = archived_chats.clone();
                        wasm_bindgen_futures::spawn_local(async move {
                            if !confirm_permanent_chat_delete() {
                                return;
                            }
                            let _ = api::permanently_delete_chat(id).await;
                            if let Ok(list) = api::list_archived_chats().await {
                                publish_archived_chats(&archived_chats, list);
                            }
                        });
                    }
                })}
                on_select_story={Callback::from({
                    let navigate = navigate.clone();
                    move |id| {
                        navigate.emit((
                            AppRoute::Stories {
                                story_id: Some(id),
                                nav: StoryNav::None,
                                overlay: None,
                                sidebar: false,
                            },
                            true,
                        ));
                    }
                })}
                on_new_story={Callback::from({
                    let stories = stories.clone();
                    let navigate = navigate.clone();
                    move |_| {
                        let stories = stories.clone();
                        let navigate = navigate.clone();
                        wasm_bindgen_futures::spawn_local(async move {
                            let payload = StoryCreate {
                                title: format!("Story {}", stories.len() + 1),
                                ..Default::default()
                            };
                            if let Ok(d) = api::create_story(&payload).await {
                                if let Ok(list) = api::list_stories().await {
                                    stories.set(list);
                                }
                                navigate.emit((
                                    AppRoute::Stories {
                                        story_id: Some(d.story.id),
                                        nav: StoryNav::Basics,
                                        overlay: None,
                                        sidebar: false,
                                    },
                                    true,
                                ));
                            }
                        });
                    }
                })}
                on_delete_story={Callback::from({
                    let stories = stories.clone();
                    let navigate = navigate.clone();
                    let route = route.clone();
                    move |id| {
                        let stories = stories.clone();
                        let navigate = navigate.clone();
                        let route = route.clone();
                        wasm_bindgen_futures::spawn_local(async move {
                            let _ = api::delete_story(id).await;
                            if let Ok(list) = api::list_stories().await {
                                if story_id_from_route(&route) == Some(id) {
                                    navigate.emit((
                                        AppRoute::Stories {
                                            story_id: list.first().map(|s| s.id),
                                            nav: StoryNav::None,
                                            overlay: None,
                                            sidebar: false,
                                        },
                                        false,
                                    ));
                                }
                                stories.set(list);
                            }
                        });
                    }
                })}
            />
            <main class="main">
                <QueueBar queue={(*queue).clone()} on_open={open_queue.clone()} />
                if mode == AppMode::Stories {
                    <StoriesShell route={route.clone()} on_navigate={navigate.clone()} />
                } else {
                <div class="chat-pane">
                <header class="header content-header">
                    if let Some(header) = active_header.as_ref() {
                        <div class="content-header-row">
                            <TitleEditor
                                title={header.title.clone()}
                                class="header-title"
                                placeholder="Chat name"
                                on_save={Callback::from({
                                    let chats = chats.clone();
                                    let chat_id = header.id;
                                    move |title| {
                                        let chats = chats.clone();
                                        wasm_bindgen_futures::spawn_local(async move {
                                            let payload = ChatUpdate {
                                                title: Some(title),
                                                character_id: None,
                                                summary: None,
                                            };
                                            if api::update_chat(chat_id, &payload).await.is_ok() {
                                                if let Ok(list) = api::list_chats().await {
                                                    publish_chats(&chats, list);
                                                }
                                            }
                                        });
                                    }
                                })}
                            />
                            <div class="header-actions">
                                <button
                                    class="btn secondary btn-compact header-icon-btn"
                                    title="Character"
                                    onclick={open_overlay.reform(|_| Overlay::Character)}
                                >
                                    {"Character"}
                                </button>
                                <button
                                    class="btn secondary btn-compact header-icon-btn"
                                    title="Variables"
                                    onclick={open_overlay.reform(|_| Overlay::Variables)}
                                >
                                    {"Variables"}
                                </button>
                                <button
                                    class="btn secondary btn-compact"
                                    title="Compress older messages into a summary"
                                    disabled={
                                        selected.as_ref().is_some_and(|chat| {
                                            summarize_in_progress(chat, &messages)
                                        }) || !can_summarize_chat(&messages, &settings)
                                    }
                                    onclick={{
                                        let chat_id = header.id;
                                        let messages = messages.clone();
                                        let chats = chats.clone();
                                        let queue = queue.clone();
                                        let bump_stream = bump_stream.clone();
                                        Callback::from(move |_| {
                                            let messages = messages.clone();
                                            let chats = chats.clone();
                                            let queue = queue.clone();
                                            let bump_stream = bump_stream.clone();
                                            wasm_bindgen_futures::spawn_local(async move {
                                                match api::summarize_chat(chat_id).await {
                                                    Ok(_) => {
                                                        bump_stream.emit(());
                                                        if let Ok(msgs) = api::get_messages(chat_id).await {
                                                            messages.set(msgs);
                                                        }
                                                        if let Ok(list) = api::list_chats().await {
                                                            publish_chats(&chats, list);
                                                        }
                                                        if let Ok(status) = api::get_queue().await {
                                                            queue.set(Some(status));
                                                        }
                                                    }
                                                    Err(err) => {
                                                        if let Some(window) = web_sys::window() {
                                                            let _ = window.alert_with_message(&format!(
                                                                "Could not summarize: {err}"
                                                            ));
                                                        }
                                                    }
                                                }
                                            });
                                        })
                                    }}
                                >{"Summarize"}</button>
                            </div>
                        </div>
                        if !header.character_name.is_empty() {
                            <p class="header-subtitle muted">{ format!("With {}", header.character_name) }</p>
                        }
                    } else {
                        <h1 class="header-title">{"Select a chat"}</h1>
                        if chats.is_empty() {
                            <p class="header-subtitle muted">{"Create a character, then start a chat from the sidebar."}</p>
                        } else {
                            <p class="header-subtitle muted">{"Responses stream on the server — switch chats freely while they generate."}</p>
                        }
                    }
                </header>
                <div class="content-scroll">
                if selected_chat_id.is_some() {
                    <MessageList
                        chat_id={selected_chat_id}
                        chat_summary={selected.as_ref().map(|c| c.summary.clone()).unwrap_or_default()}
                        summarize_busy={selected.as_ref().is_some_and(|chat| {
                            summarize_in_progress(chat, &messages)
                        })}
                        messages={(*messages).clone()}
                        loading={*messages_loading}
                        settings={(*settings).clone()}
                        character={active_header.as_ref().and_then(|header| {
                            characters.iter().find(|c| c.id == header.character_id).cloned()
                        })}
                        char_name={active_header.as_ref().map(|h| h.character_name.clone())}
                        on_messages_change={Callback::from({
                            let messages = messages.clone();
                            let chats = chats.clone();
                            let queue = queue.clone();
                            let bump_stream = bump_stream.clone();
                            move |_| {
                                let Some(chat_id) = selected_chat_id else { return };
                                let messages = messages.clone();
                                let chats = chats.clone();
                                let queue = queue.clone();
                                bump_stream.emit(());
                                wasm_bindgen_futures::spawn_local(async move {
                                    if let Ok(msgs) = api::get_messages(chat_id).await {
                                        messages.set(msgs);
                                    }
                                    if let Ok(list) = api::list_chats().await {
                                        publish_chats(&chats, list);
                                    }
                                    if let Ok(status) = api::get_queue().await {
                                        queue.set(Some(status));
                                    }
                                });
                            }
                        })}
                    />
                } else if chats.is_empty() {
                    <div class="empty-state muted">
                        if characters.is_empty() {
                            <p>{"No characters yet. Open Character from the menu to create or import one."}</p>
                        } else {
                            <p>{"No chats yet. Click New in the sidebar to pick a character."}</p>
                            <button class="btn" style="margin-top:0.75rem;" onclick={{
                                let navigate = navigate.clone();
                                let route = route.clone();
                                Callback::from(move |_| {
                                    navigate.emit((
                                        AppRoute::Chats {
                                            chat_id: chat_id_from_route(&route),
                                            overlay: Some(Overlay::NewChat),
                                            sidebar: false,
                                        },
                                        true,
                                    ));
                                })
                            }}>{"Start a chat"}</button>
                        }
                    </div>
                } else {
                    <div class="empty-state muted">
                        <p>{"Select a chat from the sidebar."}</p>
                    </div>
                }
                <Composer
                    disabled={
                        selected_chat_id.is_none()
                            || selected.as_ref().is_some_and(|chat| {
                                summarize_in_progress(chat, &messages)
                            })
                    }
                    notice={selected.as_ref().and_then(|chat| composer_notice(chat, &messages))}
                    on_send={Callback::from({
                        let messages = messages.clone();
                        let chats = chats.clone();
                        let queue = queue.clone();
                        let bump_stream = bump_stream.clone();
                        move |content: String| {
                            let Some(chat_id) = selected_chat_id else { return };
                            let messages = messages.clone();
                            let chats = chats.clone();
                            let queue = queue.clone();
                            bump_stream.emit(());
                            wasm_bindgen_futures::spawn_local(async move {
                                let _ = api::send_message(chat_id, &content).await;
                                if let Ok(msgs) = api::get_messages(chat_id).await {
                                    messages.set(msgs);
                                }
                                if let Ok(list) = api::list_chats().await {
                                    publish_chats(&chats, list);
                                }
                                if let Ok(status) = api::get_queue().await {
                                    queue.set(Some(status));
                                }
                            });
                        }
                    })}
                />
                </div>
                </div>
                }
            </main>
        </div>
        </div>
    }
}

#[derive(Properties, PartialEq)]
struct ModeBarProps {
    mode: AppMode,
    queue: Option<QueueStatus>,
    settings_open: bool,
    show_sidebar_toggle: bool,
    sidebar_open: bool,
    show_install: bool,
    on_install_change: Callback<()>,
    on_toggle_sidebar: Callback<()>,
    on_open_settings: Callback<()>,
    on_open_queue: Callback<()>,
    on_close_overlay: Callback<()>,
}

#[function_component(ModeBar)]
fn mode_bar(props: &ModeBarProps) -> Html {
    html! {
        <>
            if props.settings_open {
                <div class="settings-backdrop" onclick={props.on_close_overlay.reform(|_| ())} />
            }
            <div class="mode-bar">
                <div class="mode-bar-start">
                    if props.show_sidebar_toggle {
                        <button
                            class={classes!(
                                "mode-btn",
                                "mode-btn-menu",
                                "mode-bar-sidebar-toggle",
                                props.sidebar_open.then_some("active"),
                            )}
                            aria-label="Toggle sidebar"
                            aria-expanded={if props.sidebar_open { "true" } else { "false" }}
                            onclick={props.on_toggle_sidebar.reform(|_| ())}
                        >
                            {"☰"}
                        </button>
                    }
                    <span class="mode-bar-title">{"Dreamwell"}</span>
                </div>
                <div class="mode-bar-actions">
                    if props.show_install {
                        <InstallButton on_change={props.on_install_change.clone()} />
                    }
                    <TopBarQueueButton
                        queue={props.queue.clone()}
                        active={props.mode == AppMode::Queue}
                        on_open={props.on_open_queue.clone()}
                    />
                    <button
                        type="button"
                        class={classes!("mode-btn", "mode-btn-icon", props.settings_open.then_some("active"))}
                        title="Settings"
                        aria-label="Settings"
                        onclick={props.on_open_settings.reform(|_| ())}
                    >
                        <span class="mode-btn-icon-glyph">{"⚙"}</span>
                    </button>
                </div>
            </div>
        </>
    }
}

#[derive(Properties, PartialEq)]
struct SettingsOverlayProps {
    settings: UseStateHandle<Option<Settings>>,
    install_kind: InstallKind,
    on_install_change: Callback<()>,
    on_close: Callback<()>,
}

#[function_component(SettingsOverlay)]
fn settings_overlay(props: &SettingsOverlayProps) -> Html {
    let draft = use_state(|| None::<Settings>);
    let draft_ref = use_mut_ref(|| None::<Settings>);
    let last_saved = use_state(|| None::<Settings>);
    let phase = use_state(|| SettingsSavePhase::Synced);
    let save_timeout = use_mut_ref(|| None::<Timeout>);
    let save_ctx = SettingsSaveContext {
        draft: draft.clone(),
        draft_ref: draft_ref.clone(),
        last_saved: last_saved.clone(),
        parent_settings: props.settings.clone(),
        phase: phase.clone(),
        save_timeout: save_timeout.clone(),
    };

    {
        let save_ctx = save_ctx.clone();
        use_effect_with((*props.settings).clone(), move |loaded| {
            if let Some(settings) = loaded.clone() {
                if (*save_ctx.draft).is_none() {
                    save_ctx.load_from(settings);
                }
            }
            || ()
        });
    }

    {
        let save_ctx = save_ctx.clone();
        use_effect_with((), move |_| {
            save_ctx.ensure_loaded();
            if (*save_ctx.draft).is_none() {
                wasm_bindgen_futures::spawn_local(async move {
                    if let Ok(settings) = api::get_settings().await {
                        save_ctx.parent_settings.set(Some(settings.clone()));
                        save_ctx.load_from(settings);
                    }
                });
            }
            || ()
        });
    }

    html! {
        <div class="settings-popover panel-overlay">
            <div class="settings-header">
                <h2>{"Settings"}</h2>
                <button class="btn secondary btn-compact" onclick={props.on_close.reform(|_| ())}>{"Close"}</button>
            </div>
            <SettingsPanel
                save_ctx={save_ctx}
                install_kind={props.install_kind}
                on_install_change={props.on_install_change.clone()}
            />
        </div>
    }
}

#[derive(Properties, PartialEq)]
struct ChatPanelOverlayProps {
    overlay: Overlay,
    chat_id: Option<i64>,
    character_id: Option<i64>,
    messages: Vec<Message>,
    on_close: Callback<()>,
    on_character_change: Callback<(i64, i64)>,
    on_start_chat: Callback<(i64, String)>,
    on_chat_created: Callback<i64>,
    on_chats_changed: Callback<()>,
    on_characters_changed: Callback<()>,
}

#[function_component(ChatPanelOverlay)]
fn chat_panel_overlay(props: &ChatPanelOverlayProps) -> Html {
    let title = match props.overlay {
        Overlay::Character => "Character",
        Overlay::Variables => "Variables",
        _ => "",
    };

    html! {
        <div class="settings-popover panel-overlay">
            <div class="settings-header">
                <h2>{ title }</h2>
                <button class="btn secondary btn-compact" onclick={props.on_close.reform(|_| ())}>{"Close"}</button>
            </div>
            <div class="panel-overlay-body">
                if props.overlay == Overlay::Character {
                    <CharacterPanel
                        selected_character_id={props.character_id}
                        on_character_change={props.on_character_change.clone()}
                        on_start_chat={props.on_start_chat.clone()}
                        on_chat_created={props.on_chat_created.clone()}
                        on_chats_changed={props.on_chats_changed.clone()}
                        on_characters_changed={props.on_characters_changed.clone()}
                        chat_id={props.chat_id}
                    />
                } else if props.overlay == Overlay::Variables {
                    <VariablesPanel chat_id={props.chat_id} messages={props.messages.clone()} />
                }
            </div>
        </div>
    }
}

fn default_chat_title(character_name: &str, character_id: i64, chats: &[Chat]) -> String {
    let same = chats
        .iter()
        .filter(|chat| chat.character_id == character_id)
        .count();
    if same == 0 {
        character_name.to_string()
    } else {
        format!("{character_name} ({})", same + 1)
    }
}

fn chat_title_cmp(a: &Chat, b: &Chat) -> std::cmp::Ordering {
    a.title
        .to_lowercase()
        .cmp(&b.title.to_lowercase())
        .then_with(|| a.id.cmp(&b.id))
}

fn sort_chats(mut chats: Vec<Chat>) -> Vec<Chat> {
    chats.sort_by(|a, b| {
        b.updated_at
            .cmp(&a.updated_at)
            .then_with(|| chat_title_cmp(a, b))
    });
    chats
}

fn sort_archived_chats(mut chats: Vec<Chat>) -> Vec<Chat> {
    chats.sort_by(|a, b| {
        b.archived_at
            .cmp(&a.archived_at)
            .then_with(|| chat_title_cmp(a, b))
    });
    chats
}

fn confirm_permanent_chat_delete() -> bool {
    web_sys::window()
        .and_then(|w| {
            w.confirm_with_message("Permanently delete this archived chat? This cannot be undone.")
                .ok()
        })
        .unwrap_or(false)
}

fn summarize_placeholder_id(messages: &[Message]) -> Option<i64> {
    messages
        .iter()
        .find(|message| message.is_summary && message.content.starts_with("Summarizing earlier"))
        .map(|message| message.id)
}

fn summarize_in_progress(chat: &Chat, messages: &[Message]) -> bool {
    chat.active_job
        .as_ref()
        .is_some_and(|job| job.job_type == JobType::ChatSummarize)
        || messages
            .iter()
            .any(|message| message.is_summary && message.content.starts_with("Summarizing earlier"))
}

fn message_generation_error(message: &Message) -> Option<String> {
    generation_error_message(&message.content, message.generation_error.as_deref())
}

fn legacy_failure_only_message(message: &Message) -> bool {
    message_generation_error(message).is_some()
        && message.content.starts_with("[Generation failed: ")
}

fn can_summarize_chat(messages: &[Message], settings: &Option<Settings>) -> bool {
    let Some(settings) = settings else {
        return false;
    };
    if settings.model.is_empty() {
        return false;
    }
    let min_keep = settings.summarize_keep_recent.max(2) as usize;
    let count = messages
        .iter()
        .filter(|message| {
            !message.is_summary && message.role != MessageRole::System && !message.in_summary
        })
        .count();
    count > min_keep
}

fn confirm_character_delete(name: &str) -> bool {
    web_sys::window()
        .and_then(|w| {
            w.confirm_with_message(&format!(
                "Delete character \"{name}\"? Linked chats will also be deleted."
            ))
            .ok()
        })
        .unwrap_or(false)
}

fn confirm_delete_chat_summary() -> bool {
    web_sys::window()
        .and_then(|w| {
            w.confirm_with_message(
                "Delete this chat summary and remove summary breaks from the timeline? All messages will stay in the chat; they will be included in the model context again.",
            )
            .ok()
        })
        .unwrap_or(false)
}

fn confirm_delete_after(count: usize) -> bool {
    if count == 0 {
        return true;
    }
    web_sys::window()
        .and_then(|w| {
            w.confirm_with_message(&format!(
                "Delete {count} message{} after this one?",
                if count == 1 { "" } else { "s" }
            ))
            .ok()
        })
        .unwrap_or(false)
}

fn format_thought_duration(ms: i64) -> String {
    let total_secs = (ms / 1000).max(0);
    if total_secs < 60 {
        format!("{total_secs}s")
    } else {
        let mins = total_secs / 60;
        let secs = total_secs % 60;
        format!("{mins}m{secs}s")
    }
}

#[derive(Properties, PartialEq)]
struct ThoughtBlockProps {
    thought_content: String,
    thought_duration_ms: Option<i64>,
    thought_in_progress: bool,
}

fn format_variable_update_value(update: &MessageVariableUpdate) -> String {
    if update.deleted {
        if let Some(previous) = &update.previous_value {
            format!("{previous} → (deleted)")
        } else {
            "(deleted)".to_string()
        }
    } else if let Some(previous) = &update.previous_value {
        format!("{previous} → {}", update.value)
    } else {
        update.value.clone()
    }
}

#[derive(Properties, PartialEq)]
struct VariableUpdatesBlockProps {
    updates: Vec<MessageVariableUpdate>,
}

#[function_component(VariableUpdatesBlock)]
fn variable_updates_block(props: &VariableUpdatesBlockProps) -> Html {
    let expanded = use_state(|| false);
    let count = props.updates.len();

    let toggle = {
        let expanded = expanded.clone();
        Callback::from(move |_| expanded.set(!*expanded))
    };

    html! {
        <div class="message-variable-updates">
            <button type="button" class="message-variable-updates-toggle" onclick={toggle}>
                <span class="message-variable-updates-label">
                    { format!("Updated variables ({count})") }
                </span>
                <span class="message-variable-updates-chevron" aria-hidden="true">
                    { if *expanded { "▾" } else { "▸" } }
                </span>
            </button>
            if *expanded {
                <div class="message-variable-updates-body">
                    <div class="message-variable-updates-grid" role="table">
                        <div class="message-variable-updates-grid-header" role="columnheader">{"Name"}</div>
                        <div class="message-variable-updates-grid-header" role="columnheader">{"Value"}</div>
                        { for props.updates.iter().map(|update| {
                            html! {
                                <>
                                    <div class="message-variable-updates-key" role="cell">{ &update.key }</div>
                                    <div class="message-variable-updates-value" role="cell">{ format_variable_update_value(update) }</div>
                                </>
                            }
                        }) }
                    </div>
                </div>
            }
        </div>
    }
}

#[function_component(ThoughtBlock)]
fn thought_block(props: &ThoughtBlockProps) -> Html {
    let expanded = use_state(|| false);

    let label = if props.thought_in_progress {
        "thinking...".to_string()
    } else if let Some(ms) = props.thought_duration_ms {
        format!("thought for {}", format_thought_duration(ms))
    } else {
        "thought".to_string()
    };

    let toggle = {
        let expanded = expanded.clone();
        Callback::from(move |_| expanded.set(!*expanded))
    };

    html! {
        <div class="message-thought">
            <button type="button" class="message-thought-toggle" onclick={toggle}>
                if props.thought_in_progress {
                    <span class="thought-spinner" aria-hidden="true"></span>
                }
                <span class="message-thought-label">{ label }</span>
                <span class="message-thought-chevron" aria-hidden="true">
                    { if *expanded { "▾" } else { "▸" } }
                </span>
            </button>
            if *expanded {
                <pre class="message-thought-body">{ &props.thought_content }</pre>
            }
        </div>
    }
}

const MESSAGE_MENU_VIEWPORT_PADDING: f64 = 8.0;
const MESSAGE_MENU_ANCHOR_GAP: f64 = 4.0;

#[derive(Clone, Copy, PartialEq)]
struct MenuPlacementBounds {
    min_x: f64,
    min_y: f64,
    max_x: f64,
    max_y: f64,
}

impl MenuPlacementBounds {
    fn from_viewport_and_container(container: Option<&DomRect>) -> Self {
        let (viewport_width, viewport_height) = viewport_size();
        let padding = MESSAGE_MENU_VIEWPORT_PADDING;
        let mut bounds = Self {
            min_x: padding,
            min_y: padding,
            max_x: viewport_width - padding,
            max_y: viewport_height - padding,
        };
        if let Some(container) = container {
            bounds.min_x = bounds.min_x.max(container.left());
            bounds.min_y = bounds.min_y.max(container.top());
            bounds.max_x = bounds.max_x.min(container.right());
            bounds.max_y = bounds.max_y.min(container.bottom());
        }
        bounds
    }

    fn height(self) -> f64 {
        (self.max_y - self.min_y).max(0.0)
    }
}

#[derive(Clone, PartialEq)]
struct MenuPlacement {
    top: f64,
    left: f64,
    max_height: Option<f64>,
}

fn viewport_size() -> (f64, f64) {
    web_sys::window()
        .map(|window| {
            let width = window
                .inner_width()
                .ok()
                .and_then(|value| value.as_f64())
                .unwrap_or(0.0);
            let height = window
                .inner_height()
                .ok()
                .and_then(|value| value.as_f64())
                .unwrap_or(0.0);
            (width, height)
        })
        .unwrap_or((0.0, 0.0))
}

fn messages_container_element(anchor: &HtmlElement) -> Option<Element> {
    let mut element = anchor.parent_element();
    while let Some(current) = element {
        if current.class_list().contains("messages") {
            return Some(current);
        }
        element = current.parent_element();
    }
    None
}

fn messages_container_rect(anchor: &HtmlElement) -> Option<DomRect> {
    messages_container_element(anchor).map(|element| element.get_bounding_client_rect())
}

fn compute_message_menu_placement(
    anchor: &DomRect,
    menu_width: f64,
    menu_height: f64,
    bounds: MenuPlacementBounds,
    align_end: bool,
) -> MenuPlacement {
    let gap = MESSAGE_MENU_ANCHOR_GAP;
    let mut left = if align_end {
        anchor.right() - menu_width
    } else {
        anchor.left()
    };

    let below_top = anchor.bottom() + gap;
    let above_top = anchor.top() - gap - menu_height;
    let fits_below = below_top + menu_height <= bounds.max_y;
    let fits_above = above_top >= bounds.min_y;
    let mut top = if fits_below {
        below_top
    } else if fits_above {
        above_top
    } else {
        let space_below = bounds.max_y - below_top;
        let space_above = anchor.top() - gap - bounds.min_y;
        if space_below >= space_above {
            below_top
        } else {
            above_top
        }
    };

    if left + menu_width > bounds.max_x {
        left = bounds.max_x - menu_width;
    }
    if left < bounds.min_x {
        left = bounds.min_x;
    }

    let available_height = bounds.height();
    let max_height = if menu_height > available_height {
        Some(available_height.floor())
    } else {
        None
    };
    let effective_height = max_height.unwrap_or(menu_height);

    if top + effective_height > bounds.max_y {
        top = bounds.max_y - effective_height;
    }
    if top < bounds.min_y {
        top = bounds.min_y;
    }

    MenuPlacement {
        top,
        left,
        max_height,
    }
}

fn update_message_menu_position(
    menu_btn_ref: &NodeRef,
    menu_ref: &NodeRef,
    menu_style: &UseStateHandle<Option<String>>,
    align_end: bool,
) {
    let Some(button) = menu_btn_ref.cast::<HtmlElement>() else {
        return;
    };
    let Some(menu) = menu_ref.cast::<HtmlElement>() else {
        return;
    };

    let anchor = button.get_bounding_client_rect();
    let menu_rect = menu.get_bounding_client_rect();
    let menu_width = menu_rect.width().max(menu.offset_width() as f64);
    let menu_height = menu_rect.height().max(menu.offset_height() as f64);
    if menu_width <= 0.0 || menu_height <= 0.0 {
        return;
    }

    let container = messages_container_rect(&button);
    let bounds = MenuPlacementBounds::from_viewport_and_container(container.as_ref());
    let placement =
        compute_message_menu_placement(&anchor, menu_width, menu_height, bounds, align_end);

    let mut style = format!(
        "top:{}px;left:{}px;",
        placement.top.round(),
        placement.left.round()
    );
    if let Some(max_height) = placement.max_height {
        style.push_str(&format!(
            "max-height:{}px;overflow-y:auto;",
            max_height.round()
        ));
    }
    menu_style.set(Some(style));
}

#[derive(Clone, Copy, PartialEq)]
enum MessageBubbleMode {
    View,
    Edit,
}

#[derive(Properties, PartialEq)]
struct MessageBubbleProps {
    message: Message,
    chat_id: i64,
    is_last: bool,
    after_count: usize,
    display_content: String,
    rendered_content: Html,
    show_thoughts: bool,
    show_variables: bool,
    on_changed: Callback<()>,
}

#[function_component(MessageBubble)]
fn message_bubble(props: &MessageBubbleProps) -> Html {
    let menu_open = use_state(|| false);
    let menu_btn_ref = use_node_ref();
    let menu_ref = use_node_ref();
    let menu_style = use_state(|| None::<String>);
    let mode = use_state(|| MessageBubbleMode::View);
    let edit_text = use_state(String::new);
    let acting = use_state(|| false);

    let role = match props.message.role {
        MessageRole::User => "user",
        MessageRole::Assistant => "assistant",
        MessageRole::System => "system",
    };
    let queued = matches!(props.message.job_status, Some(JobStatus::Queued));
    let streaming = matches!(props.message.job_status, Some(JobStatus::Running));
    let active = queued || streaming;
    let can_menu =
        !props.message.is_summary && props.message.role != MessageRole::System && !active;
    let show_regenerate = props.message.role == MessageRole::Assistant;
    let show_recheck_variables = props.show_variables
        && props.is_last
        && props.message.role == MessageRole::Assistant
        && !props.message.content.trim().is_empty();
    let show_thought_block = props.show_thoughts
        && props.message.role == MessageRole::Assistant
        && (!props.message.thought_content.is_empty()
            || (props.message.thought_in_progress && active));
    let show_variable_updates = props.show_variables
        && props.message.role == MessageRole::Assistant
        && !props.message.variable_updates.is_empty();
    let align_menu_end = props.message.role == MessageRole::User;

    {
        let menu_open = menu_open.clone();
        let menu_btn_ref = menu_btn_ref.clone();
        let menu_ref = menu_ref.clone();
        let menu_style = menu_style.clone();
        use_effect_with(
            (*menu_open, show_regenerate, props.after_count),
            move |(open, _, _)| {
                let open = *open;
                let reposition = {
                    let menu_btn_ref = menu_btn_ref.clone();
                    let menu_ref = menu_ref.clone();
                    let menu_style = menu_style.clone();
                    Rc::new(move || {
                        update_message_menu_position(
                            &menu_btn_ref,
                            &menu_ref,
                            &menu_style,
                            align_menu_end,
                        );
                    })
                };

                let scroll_container = if open {
                    menu_btn_ref
                        .cast::<HtmlElement>()
                        .and_then(|button| messages_container_element(&button))
                } else {
                    menu_style.set(None);
                    None
                };

                let scroll_callback = Closure::wrap(Box::new({
                    let reposition = reposition.clone();
                    move |_event: web_sys::Event| reposition()
                }) as Box<dyn FnMut(_)>);

                let resize_callback = Closure::wrap(Box::new({
                    let reposition = reposition.clone();
                    move |_event: web_sys::Event| reposition()
                }) as Box<dyn FnMut(_)>);

                if open {
                    Timeout::new(0, {
                        let reposition = reposition.clone();
                        move || reposition()
                    })
                    .forget();

                    if let Some(container) = scroll_container.as_ref() {
                        let _ = container.add_event_listener_with_callback(
                            "scroll",
                            scroll_callback.as_ref().unchecked_ref(),
                        );
                    }
                    if let Some(window) = web_sys::window() {
                        let _ = window.add_event_listener_with_callback(
                            "resize",
                            resize_callback.as_ref().unchecked_ref(),
                        );
                    }
                }

                move || {
                    if open {
                        if let Some(container) = scroll_container.as_ref() {
                            let _ = container.remove_event_listener_with_callback(
                                "scroll",
                                scroll_callback.as_ref().unchecked_ref(),
                            );
                        }
                        if let Some(window) = web_sys::window() {
                            let _ = window.remove_event_listener_with_callback(
                                "resize",
                                resize_callback.as_ref().unchecked_ref(),
                            );
                        }
                    }
                }
            },
        );
    }

    let close_menu = {
        let menu_open = menu_open.clone();
        Callback::from(move |_| menu_open.set(false))
    };

    let start_edit = {
        let mode = mode.clone();
        let edit_text = edit_text.clone();
        let menu_open = menu_open.clone();
        let content = props.message.content.clone();
        Callback::from(move |_| {
            edit_text.set(content.clone());
            mode.set(MessageBubbleMode::Edit);
            menu_open.set(false);
        })
    };

    let cancel_edit = {
        let mode = mode.clone();
        Callback::from(move |_| mode.set(MessageBubbleMode::View))
    };

    let save_edit = {
        let mode = mode.clone();
        let edit_text = edit_text.clone();
        let acting = acting.clone();
        let on_changed = props.on_changed.clone();
        let chat_id = props.chat_id;
        let message_id = props.message.id;
        Callback::from(move |_| {
            let content = (*edit_text).trim().to_string();
            if content.is_empty() || *acting {
                return;
            }
            acting.set(true);
            let mode = mode.clone();
            let acting = acting.clone();
            let on_changed = on_changed.clone();
            wasm_bindgen_futures::spawn_local(async move {
                match api::update_message(chat_id, message_id, &content, false).await {
                    Ok(_) => {
                        mode.set(MessageBubbleMode::View);
                        on_changed.emit(());
                    }
                    Err(err) => {
                        if let Some(window) = web_sys::window() {
                            let _ = window.alert_with_message(&format!("Could not save: {err}"));
                        }
                    }
                }
                acting.set(false);
            });
        })
    };

    let rewind_here = {
        let menu_open = menu_open.clone();
        let acting = acting.clone();
        let on_changed = props.on_changed.clone();
        let chat_id = props.chat_id;
        let message_id = props.message.id;
        let after_count = props.after_count;
        Callback::from(move |_| {
            if *acting || after_count == 0 {
                return;
            }
            if !confirm_delete_after(after_count) {
                return;
            }
            menu_open.set(false);
            acting.set(true);
            let acting = acting.clone();
            let on_changed = on_changed.clone();
            wasm_bindgen_futures::spawn_local(async move {
                match api::rewind_message(chat_id, message_id).await {
                    Ok(_) => on_changed.emit(()),
                    Err(err) => {
                        if let Some(window) = web_sys::window() {
                            let _ = window.alert_with_message(&format!("Could not rewind: {err}"));
                        }
                    }
                }
                acting.set(false);
            });
        })
    };

    let regenerate = {
        let menu_open = menu_open.clone();
        let acting = acting.clone();
        let on_changed = props.on_changed.clone();
        let chat_id = props.chat_id;
        let message_id = props.message.id;
        let is_last = props.is_last;
        let after_count = props.after_count;
        Callback::from(move |_| {
            if *acting {
                return;
            }
            let delete_count = if is_last { 0 } else { after_count };
            if !confirm_delete_after(delete_count) {
                return;
            }
            menu_open.set(false);
            acting.set(true);
            let acting = acting.clone();
            let on_changed = on_changed.clone();
            wasm_bindgen_futures::spawn_local(async move {
                match api::regenerate_message(chat_id, message_id, false).await {
                    Ok(_) => on_changed.emit(()),
                    Err(err) => {
                        if let Some(window) = web_sys::window() {
                            let _ =
                                window.alert_with_message(&format!("Could not regenerate: {err}"));
                        }
                    }
                }
                acting.set(false);
            });
        })
    };

    let recheck_variables = {
        let menu_open = menu_open.clone();
        let acting = acting.clone();
        let on_changed = props.on_changed.clone();
        let chat_id = props.chat_id;
        let message_id = props.message.id;
        Callback::from(move |_| {
            if *acting {
                return;
            }
            menu_open.set(false);
            acting.set(true);
            let acting = acting.clone();
            let on_changed = on_changed.clone();
            wasm_bindgen_futures::spawn_local(async move {
                match api::recheck_message_variables(chat_id, message_id).await {
                    Ok(_) => on_changed.emit(()),
                    Err(err) => {
                        if let Some(window) = web_sys::window() {
                            let _ = window
                                .alert_with_message(&format!("Could not recheck variables: {err}"));
                        }
                    }
                }
                acting.set(false);
            });
        })
    };

    let pending = queued && props.display_content.is_empty();
    let generation_error = message_generation_error(&props.message);
    let failure_only = legacy_failure_only_message(&props.message);
    let show_body = !props.display_content.is_empty() && !failure_only;
    html! {
        <div class={classes!(
            "message",
            role,
            (*mode == MessageBubbleMode::Edit).then_some("message--editing"),
            pending.then_some("message--pending"),
            streaming.then_some("message--streaming"),
            generation_error.is_some().then_some("message--failed"),
        )}>
            <div class="message-header">
                <div class="message-meta muted">
                    { role.to_string() }
                    if queued { <span>{" · waiting in queue"}</span> }
                    if streaming { <span>{" · still writing"}</span> }
                    if generation_error.is_some() && !streaming && !queued {
                        <span class="message-meta-error">{" · generation failed"}</span>
                    }
                </div>
                if can_menu {
                    <div class="message-menu-wrap">
                        if *menu_open {
                            <div class="message-menu-backdrop" onclick={close_menu.clone()} />
                        }
                        <button
                            type="button"
                            class="message-menu-btn"
                            ref={menu_btn_ref.clone()}
                            title="Message options"
                            onclick={Callback::from({
                                let menu_open = menu_open.clone();
                                move |e: MouseEvent| {
                                    e.stop_propagation();
                                    menu_open.set(!*menu_open);
                                }
                            })}
                            disabled={*acting}
                        >
                            {"⋯"}
                        </button>
                        if *menu_open {
                            <div
                                class={classes!(
                                    "message-menu",
                                    menu_style.is_some().then_some("message-menu--anchored")
                                )}
                                ref={menu_ref.clone()}
                                style={(*menu_style).clone()}
                                onclick={Callback::from(|e: MouseEvent| e.stop_propagation())}
                            >
                                <button type="button" class="message-menu-item" onclick={start_edit}>{"Edit"}</button>
                                if show_regenerate {
                                    <button type="button" class="message-menu-item" onclick={regenerate}>{"Regenerate"}</button>
                                }
                                if show_recheck_variables {
                                    <button type="button" class="message-menu-item" onclick={recheck_variables}>{"Recheck variables"}</button>
                                }
                                <button
                                    type="button"
                                    class="message-menu-item message-menu-item--rewind"
                                    onclick={rewind_here}
                                    disabled={props.after_count == 0 || *acting}
                                >
                                    if props.after_count == 0 {
                                        {"Rewind here (nothing after)"}
                                    } else {
                                        { format!("Rewind here (delete {after} after)", after = props.after_count) }
                                    }
                                </button>
                            </div>
                        }
                    </div>
                }
            </div>
            if show_thought_block {
                <ThoughtBlock
                    thought_content={props.message.thought_content.clone()}
                    thought_duration_ms={props.message.thought_duration_ms}
                    thought_in_progress={props.message.thought_in_progress}
                />
            }
            if *mode == MessageBubbleMode::Edit {
                <textarea
                    class="message-edit-input"
                    value={(*edit_text).clone()}
                    oninput={Callback::from({
                        let edit_text = edit_text.clone();
                        move |e: InputEvent| {
                            let input: HtmlInputElement = e.target_unchecked_into();
                            edit_text.set(input.value());
                        }
                    })}
                    disabled={*acting}
                />
                <div class="message-edit-actions">
                    <button type="button" class="btn" onclick={save_edit} disabled={*acting || edit_text.trim().is_empty()}>
                        { if *acting { "Saving…" } else { "Save" } }
                    </button>
                    <button type="button" class="btn secondary" onclick={cancel_edit} disabled={*acting}>{"Cancel"}</button>
                </div>
            } else if props.display_content.is_empty() && queued {
                <span class="muted">{"Waiting in queue…"}</span>
            } else if props.display_content.is_empty() && streaming {
                { "…" }
            } else if failure_only {
                if let Some(error) = generation_error {
                    <div class="message-error" role="alert">
                        <strong>{"Generation failed"}</strong>
                        <span>{ error }</span>
                    </div>
                }
            } else if props.display_content.is_empty()
                && props.message.role == MessageRole::Assistant
                && !props.message.thought_content.is_empty()
            {
                <span class="muted">{"(No reply text — see thought block above)"}</span>
            } else if props.display_content.is_empty()
                && props.message.role == MessageRole::Assistant
                && !active
            {
                <span class="muted">{"(Empty response)"}</span>
            } else {
                if show_body {
                    { props.rendered_content.clone() }
                    if streaming {
                        <span class="streaming-cursor" aria-hidden="true">{"▍"}</span>
                        <div class="message-streaming-note muted">{"Still writing…"}</div>
                    }
                }
                if let Some(error) = generation_error {
                    <div class={classes!("message-error", show_body.then_some("message-error--partial"))} role="alert">
                        <strong>{ if show_body { "Generation stopped early" } else { "Generation failed" } }</strong>
                        <span>{ error }</span>
                    </div>
                }
            }
            if show_variable_updates {
                <VariableUpdatesBlock updates={props.message.variable_updates.clone()} />
            }
        </div>
    }
}

const SUMMARIZE_PLACEHOLDER: &str = "Summarizing earlier messages…";

#[derive(Clone, Copy, PartialEq)]
enum SummaryMarkerMode {
    View,
    Edit,
}

#[derive(Properties, PartialEq)]
struct SummaryMarkerProps {
    message: Message,
    chat_id: i64,
    chat_summary: String,
    summarize_busy: bool,
    on_changed: Callback<()>,
}

#[function_component(SummaryMarker)]
fn summary_marker(props: &SummaryMarkerProps) -> Html {
    let expanded = use_state(|| true);
    let mode = use_state(|| SummaryMarkerMode::View);
    let edit_text = use_state(String::new);
    let acting = use_state(|| false);
    let active = matches!(
        props.message.job_status,
        Some(JobStatus::Queued) | Some(JobStatus::Running)
    );
    let pending = active || props.message.content == SUMMARIZE_PLACEHOLDER;
    let summary_html = if props.chat_summary.is_empty() {
        html! { <span class="muted">{"(Empty summary)"}</span> }
    } else {
        markdown::render_message_content(&props.chat_summary)
    };

    let toggle = {
        let expanded = expanded.clone();
        Callback::from(move |_| expanded.set(!*expanded))
    };

    let start_edit = {
        let mode = mode.clone();
        let edit_text = edit_text.clone();
        let expanded = expanded.clone();
        let chat_summary = props.chat_summary.clone();
        Callback::from(move |_| {
            edit_text.set(chat_summary.clone());
            mode.set(SummaryMarkerMode::Edit);
            expanded.set(true);
        })
    };

    let cancel_edit = {
        let mode = mode.clone();
        Callback::from(move |_| mode.set(SummaryMarkerMode::View))
    };

    let can_manage = !pending && !props.summarize_busy && !*acting;
    let has_summary = !props.chat_summary.trim().is_empty();

    let regenerate_summary = {
        let acting = acting.clone();
        let on_changed = props.on_changed.clone();
        let chat_id = props.chat_id;
        let marker_id = props.message.id;
        Callback::from(move |_| {
            if *acting || !has_summary {
                return;
            }
            acting.set(true);
            let acting = acting.clone();
            let on_changed = on_changed.clone();
            wasm_bindgen_futures::spawn_local(async move {
                match api::regenerate_chat_summary(chat_id, marker_id).await {
                    Ok(_) => on_changed.emit(()),
                    Err(err) => {
                        if let Some(window) = web_sys::window() {
                            let _ = window.alert_with_message(&format!(
                                "Could not regenerate summary: {err}"
                            ));
                        }
                    }
                }
                acting.set(false);
            });
        })
    };

    let delete_summary = {
        let acting = acting.clone();
        let on_changed = props.on_changed.clone();
        let chat_id = props.chat_id;
        Callback::from(move |_| {
            if *acting || !has_summary {
                return;
            }
            if !confirm_delete_chat_summary() {
                return;
            }
            acting.set(true);
            let acting = acting.clone();
            let on_changed = on_changed.clone();
            wasm_bindgen_futures::spawn_local(async move {
                match api::delete_chat_summary(chat_id).await {
                    Ok(_) => on_changed.emit(()),
                    Err(err) => {
                        if let Some(window) = web_sys::window() {
                            let _ = window
                                .alert_with_message(&format!("Could not delete summary: {err}"));
                        }
                    }
                }
                acting.set(false);
            });
        })
    };

    let save_edit = {
        let mode = mode.clone();
        let edit_text = edit_text.clone();
        let acting = acting.clone();
        let on_changed = props.on_changed.clone();
        let chat_id = props.chat_id;
        Callback::from(move |_| {
            let summary = (*edit_text).trim().to_string();
            if summary.is_empty() || *acting {
                return;
            }
            acting.set(true);
            let mode = mode.clone();
            let acting = acting.clone();
            let on_changed = on_changed.clone();
            wasm_bindgen_futures::spawn_local(async move {
                let payload = ChatUpdate {
                    title: None,
                    character_id: None,
                    summary: Some(summary),
                };
                match api::update_chat(chat_id, &payload).await {
                    Ok(_) => {
                        mode.set(SummaryMarkerMode::View);
                        on_changed.emit(());
                    }
                    Err(err) => {
                        if let Some(window) = web_sys::window() {
                            let _ = window
                                .alert_with_message(&format!("Could not save summary: {err}"));
                        }
                    }
                }
                acting.set(false);
            });
        })
    };

    html! {
        <div class={classes!(
            "message-summary-marker",
            active.then_some("message-summary-marker--active")
        )}>
            <div class="message-summary-break" aria-hidden="true">
                <span class="message-summary-break-line"></span>
                <span class="message-summary-break-label">{
                    if pending { "Summarizing earlier messages" } else { "Earlier messages summarized" }
                }</span>
                <span class="message-summary-break-line"></span>
            </div>
            <div class="message-summary-body">
                if pending {
                    <p class="message-summary-pending muted">
                        <span class="settings-save-spinner" aria-hidden="true"></span>
                        {" Compressing chat history to fit your context window…"}
                    </p>
                } else {
                    <>
                        <div class="message-summary-actions">
                            <button type="button" class="message-summary-toggle" onclick={toggle}>
                                <span class="message-summary-chevron">{ if *expanded { "▾" } else { "▸" } }</span>
                                <span>{"View summary"}</span>
                            </button>
                            if *mode == SummaryMarkerMode::View && can_manage && has_summary {
                                <button type="button" class="message-summary-toggle" onclick={start_edit}>
                                    {"Edit summary"}
                                </button>
                                <button type="button" class="message-summary-toggle" onclick={regenerate_summary}>
                                    {"Regenerate"}
                                </button>
                                <button type="button" class="message-summary-toggle" onclick={delete_summary}>
                                    {"Delete summary"}
                                </button>
                            }
                        </div>
                        if *expanded {
                            if *mode == SummaryMarkerMode::Edit {
                                <div class="message-summary-editor">
                                    <textarea
                                        rows="8"
                                        value={(*edit_text).clone()}
                                        oninput={Callback::from({
                                            let edit_text = edit_text.clone();
                                            move |e: InputEvent| {
                                                let input: HtmlInputElement = e.target_unchecked_into();
                                                edit_text.set(input.value());
                                            }
                                        })}
                                    />
                                    <div class="message-summary-editor-actions">
                                        <button class="btn secondary" onclick={cancel_edit} disabled={*acting}>{"Cancel"}</button>
                                        <button class="btn" onclick={save_edit} disabled={*acting || edit_text.trim().is_empty()}>
                                            { if *acting { "Saving…" } else { "Save summary" } }
                                        </button>
                                    </div>
                                </div>
                            } else {
                                <div class="message-summary-content">{ summary_html }</div>
                            }
                        }
                    </>
                }
            </div>
        </div>
    }
}

#[derive(Properties, PartialEq)]
struct MessageListProps {
    chat_id: Option<i64>,
    chat_summary: String,
    summarize_busy: bool,
    messages: Vec<Message>,
    loading: bool,
    settings: Option<Settings>,
    character: Option<Character>,
    char_name: Option<String>,
    on_messages_change: Callback<()>,
}

#[function_component(MessageList)]
fn message_list(props: &MessageListProps) -> Html {
    let messages_ref = use_node_ref();

    use_effect_with((props.chat_id, props.loading, props.messages.len()), {
        let messages_ref = messages_ref.clone();
        move |(chat_id, loading, len)| {
            if chat_id.is_some() && !*loading && *len > 0 {
                let messages_ref = messages_ref.clone();
                Timeout::new(0, move || {
                    let el = messages_ref.cast::<HtmlElement>();
                    scroll_chat_view_to_bottom(el.as_ref());
                })
                .forget();
            }
            || ()
        }
    });

    let macro_ctx = props.settings.as_ref().map(|settings| {
        if let Some(character) = props.character.as_ref() {
            MacroContext::from_character_and_settings(
                Some(character),
                &settings.user_name,
                &settings.persona_description,
            )
        } else {
            MacroContext {
                char_name: props.char_name.as_deref().unwrap_or("Character"),
                user_name: settings.user_name.as_str(),
                persona: settings.persona_description.as_str(),
                description: "",
                personality: "",
                scenario: "",
                first_message: "",
            }
        }
    });
    let last_id = props.messages.last().map(|m| m.id);
    let show_thoughts = props
        .settings
        .as_ref()
        .is_some_and(|s| s.thought_blocks_enabled);
    let show_variables = props.settings.as_ref().is_some_and(|s| s.variables_enabled);
    html! {
        <div class="messages" ref={messages_ref}>
            if props.messages.is_empty() {
                <div class="empty-state muted">
                    if props.loading {
                        {"Loading messages…"}
                    } else {
                        {"Send a message to queue a reply. You can switch chats while it generates server-side."}
                    }
                </div>
            } else if let Some(chat_id) = props.chat_id {
                { for props.messages.iter().enumerate().map(|(idx, m)| {
                    let after_count = props.messages.len().saturating_sub(idx + 1);
                    let is_last = last_id == Some(m.id);
                    let streaming = matches!(m.job_status, Some(JobStatus::Running));
                    let display_content = if m.role == MessageRole::Assistant {
                        variables::strip_variables_for_display(&m.content, streaming)
                    } else {
                        m.content.clone()
                    };
                    let rendered_content = if display_content.is_empty() {
                        html! {}
                    } else if let Some(ctx) = macro_ctx.as_ref() {
                        markdown::render_message_content(&substitute_macros(&display_content, ctx))
                    } else {
                        markdown::render_message_content(&display_content)
                    };
                    if m.is_summary {
                        html! {
                            <SummaryMarker
                                key={m.id}
                                message={m.clone()}
                                chat_id={chat_id}
                                chat_summary={props.chat_summary.clone()}
                                summarize_busy={props.summarize_busy}
                                on_changed={props.on_messages_change.clone()}
                            />
                        }
                    } else {
                        html! {
                            <MessageBubble
                                key={m.id}
                                message={m.clone()}
                                chat_id={chat_id}
                                is_last={is_last}
                                after_count={after_count}
                                display_content={display_content}
                                rendered_content={rendered_content}
                                show_thoughts={show_thoughts}
                                show_variables={show_variables}
                                on_changed={props.on_messages_change.clone()}
                            />
                        }
                    }
                }) }
            }
        </div>
    }
}

#[derive(Properties, PartialEq)]
struct ComposerProps {
    disabled: bool,
    notice: Option<GenerationNotice>,
    on_send: Callback<String>,
}

#[function_component(Composer)]
fn composer(props: &ComposerProps) -> Html {
    let text = use_state(String::new);
    let sending = use_state(|| false);
    let disabled = props.disabled;

    let on_send = {
        let text = text.clone();
        let sending = sending.clone();
        let parent = props.on_send.clone();
        Callback::from(move |_| {
            let content = (*text).trim().to_string();
            if content.is_empty() || *sending || disabled {
                return;
            }
            sending.set(true);
            text.set(String::new());
            parent.emit(content);
            sending.set(false);
        })
    };

    let placeholder = props
        .notice
        .map(GenerationNotice::textarea_placeholder)
        .unwrap_or("Write your message…");

    html! {
        <>
            if let Some(notice) = props.notice {
                <GenerationStatusBar notice={notice} />
            }
            <div class="composer">
                <textarea
                    rows="2"
                    value={(*text).clone()}
                    oninput={Callback::from({
                        let text = text.clone();
                        move |e: InputEvent| {
                            let input: HtmlInputElement = e.target_unchecked_into();
                            text.set(input.value());
                        }
                    })}
                    placeholder={placeholder}
                    disabled={props.disabled || *sending}
                />
                <button class="btn" onclick={on_send} disabled={props.disabled || *sending || text.trim().is_empty()}>
                    { if *sending { "Queuing…" } else { "Send" } }
                </button>
            </div>
        </>
    }
}

#[derive(Properties, PartialEq)]
struct CharacterPickerModalProps {
    characters: Vec<Character>,
    on_close: Callback<()>,
    on_select: Callback<Character>,
}

#[function_component(CharacterPickerModal)]
fn character_picker_modal(props: &CharacterPickerModalProps) -> Html {
    html! {
        <>
            <div class="modal-backdrop" onclick={props.on_close.reform(|_| ())} />
            <div class="modal">
                <h2>{"Start a chat"}</h2>
                <p class="muted" style="margin:0 0 0.75rem;">{"Pick a character for this conversation."}</p>
                if props.characters.is_empty() {
                    <p class="muted">{"No characters yet. Create or import one from the Character menu first."}</p>
                } else {
                    <div class="modal-list">
                        { for props.characters.iter().map(|c| {
                            let character = c.clone();
                            html! {
                                <div class="modal-item" onclick={{
                                    let on_select = props.on_select.clone();
                                    let character = character.clone();
                                    Callback::from(move |_| on_select.emit(character.clone()))
                                }}>
                                    <span>{ &c.name }</span>
                                </div>
                            }
                        }) }
                    </div>
                }
                <button class="btn secondary" onclick={props.on_close.reform(|_| ())}>{"Cancel"}</button>
            </div>
        </>
    }
}

#[derive(Properties, PartialEq)]
struct CharacterPanelProps {
    selected_character_id: Option<i64>,
    chat_id: Option<i64>,
    on_character_change: Callback<(i64, i64)>,
    on_start_chat: Callback<(i64, String)>,
    on_chat_created: Callback<i64>,
    on_chats_changed: Callback<()>,
    on_characters_changed: Callback<()>,
}

#[function_component(CharacterPanel)]
fn character_panel(props: &CharacterPanelProps) -> Html {
    let characters = use_state(Vec::<Character>::new);
    let draft = use_state(CharacterDraft::default);
    let editing_id = use_state(|| None::<i64>);
    let file_input = use_node_ref();

    {
        let characters = characters.clone();
        use_effect_with((), move |_| {
            wasm_bindgen_futures::spawn_local(async move {
                if let Ok(list) = api::list_characters().await {
                    characters.set(list);
                }
            });
            || ()
        });
    }

    {
        let draft = draft.clone();
        let editing_id = editing_id.clone();
        let characters = characters.clone();
        let selected = props.selected_character_id;
        use_effect_with(selected, move |selected| {
            if let Some(id) = *selected {
                if let Some(c) = characters.iter().find(|c| c.id == id) {
                    editing_id.set(Some(c.id));
                    draft.set(CharacterDraft::from(c));
                }
            }
            || ()
        });
    }

    html! {
        <div>
            <div style="display:flex;gap:0.5rem;flex-wrap:wrap;margin-bottom:1rem;">
                <button class="btn" onclick={{
                    let draft = draft.clone();
                    let editing_id = editing_id.clone();
                    Callback::from(move |_| {
                        draft.set(CharacterDraft::default());
                        editing_id.set(None);
                    })
                }}>{"New"}</button>
                <button class="btn secondary" onclick={{
                    let file_input = file_input.clone();
                    Callback::from(move |_| {
                        if let Some(input) = file_input.cast::<HtmlInputElement>() {
                            input.click();
                        }
                    })
                }}>{"Import JSON/PNG"}</button>
                <input type="file" accept=".json,.png" ref={file_input} style="display:none;" onchange={{
                    let characters = characters.clone();
                    let draft = draft.clone();
                    let editing_id = editing_id.clone();
                    let on_character_change = props.on_character_change.clone();
                    let on_start_chat = props.on_start_chat.clone();
                    let on_characters_changed = props.on_characters_changed.clone();
                    let chat_id = props.chat_id;
                    Callback::from(move |e: Event| {
                        let input: HtmlInputElement = e.target_unchecked_into();
                        if let Some(file) = input.files().and_then(|f| f.get(0)) {
                            let characters = characters.clone();
                            let draft = draft.clone();
                            let editing_id = editing_id.clone();
                            let on_character_change = on_character_change.clone();
                            let on_start_chat = on_start_chat.clone();
                            let on_characters_changed = on_characters_changed.clone();
                            wasm_bindgen_futures::spawn_local(async move {
                                if let Ok(character) = api::import_character(&file).await {
                                    if let Ok(list) = api::list_characters().await {
                                        characters.set(list);
                                    }
                                    on_characters_changed.emit(());
                                    editing_id.set(Some(character.id));
                                    draft.set(CharacterDraft::from(&character));
                                    if let Some(chat_id) = chat_id {
                                        on_character_change.emit((chat_id, character.id));
                                    } else {
                                        on_start_chat.emit((character.id, character.name.clone()));
                                    }
                                }
                            });
                        }
                    })
                }} />
            </div>
            <div class="scroll-list">
                { for characters.iter().map(|c| {
                    let id = c.id;
                    let name = c.name.clone();
                    let delete_name = name.clone();
                    html! {
                        <div class="list-row"
                            onclick={{
                                let draft = draft.clone();
                                let editing_id = editing_id.clone();
                                let c = c.clone();
                                Callback::from(move |_| {
                                    editing_id.set(Some(id));
                                    draft.set(CharacterDraft::from(&c));
                                })
                            }}>
                            <span class="list-row-name">{ &c.name }</span>
                            <button class="btn secondary btn-compact" onclick={{
                                let on_start_chat = props.on_start_chat.clone();
                                let name = name.clone();
                                Callback::from(move |e: MouseEvent| {
                                    e.stop_propagation();
                                    on_start_chat.emit((id, name.clone()));
                                })
                            }}>{"Chat"}</button>
                            <button class="btn secondary btn-compact" onclick={{
                                let characters = characters.clone();
                                let draft = draft.clone();
                                let editing_id = editing_id.clone();
                                let on_characters_changed = props.on_characters_changed.clone();
                                let on_chats_changed = props.on_chats_changed.clone();
                                Callback::from(move |e: MouseEvent| {
                                    e.stop_propagation();
                                    if !confirm_character_delete(&delete_name) {
                                        return;
                                    }
                                    let characters = characters.clone();
                                    let draft = draft.clone();
                                    let editing_id = editing_id.clone();
                                    let on_characters_changed = on_characters_changed.clone();
                                    let on_chats_changed = on_chats_changed.clone();
                                    wasm_bindgen_futures::spawn_local(async move {
                                        match api::delete_character(id).await {
                                            Ok(()) => {
                                                if *editing_id == Some(id) {
                                                    editing_id.set(None);
                                                    draft.set(CharacterDraft::default());
                                                }
                                                if let Ok(list) = api::list_characters().await {
                                                    characters.set(list);
                                                }
                                                on_characters_changed.emit(());
                                                on_chats_changed.emit(());
                                            }
                                            Err(err) => {
                                                if let Some(window) = web_sys::window() {
                                                    let _ = window.alert_with_message(&format!(
                                                        "Could not delete character: {err}"
                                                    ));
                                                }
                                            }
                                        }
                                    });
                                })
                            }}>{"delete"}</button>
                        </div>
                    }
                }) }
            </div>
            { character_fields(&draft) }
            <button class="btn" style="margin-top:0.5rem;" onclick={{
                let draft = draft.clone();
                let editing_id = editing_id.clone();
                let characters = characters.clone();
                let on_character_change = props.on_character_change.clone();
                let on_start_chat = props.on_start_chat.clone();
                let on_characters_changed = props.on_characters_changed.clone();
                let chat_id = props.chat_id;
                Callback::from(move |_| {
                    let payload = draft.to_create();
                    let editing_id_val = *editing_id;
                    let characters = characters.clone();
                    let draft = draft.clone();
                    let editing_id = editing_id.clone();
                    let on_character_change = on_character_change.clone();
                    let on_start_chat = on_start_chat.clone();
                    let on_characters_changed = on_characters_changed.clone();
                    wasm_bindgen_futures::spawn_local(async move {
                        let character = if let Some(id) = editing_id_val {
                            api::update_character(id, &draft.to_update()).await
                        } else {
                            api::create_character(&payload).await
                        };
                        if let Ok(character) = character {
                            if let Ok(list) = api::list_characters().await {
                                characters.set(list);
                            }
                            on_characters_changed.emit(());
                            editing_id.set(Some(character.id));
                            draft.set(CharacterDraft::from(&character));
                            if let Some(chat_id) = chat_id {
                                on_character_change.emit((chat_id, character.id));
                            } else {
                                on_start_chat.emit((character.id, character.name.clone()));
                            }
                        }
                    });
                })
            }}>{"Save character"}</button>
        </div>
    }
}

#[derive(Clone, Default, PartialEq)]
struct CharacterDraft {
    name: String,
    description: String,
    personality: String,
    scenario: String,
    first_message: String,
    example_dialogue: String,
    system_prompt: String,
}

impl CharacterDraft {
    fn from(c: &Character) -> Self {
        Self {
            name: c.name.clone(),
            description: c.description.clone(),
            personality: c.personality.clone(),
            scenario: c.scenario.clone(),
            first_message: c.first_message.clone(),
            example_dialogue: c.example_dialogue.clone(),
            system_prompt: c.system_prompt.clone(),
        }
    }

    fn to_create(&self) -> CharacterCreate {
        CharacterCreate {
            name: self.name.clone(),
            description: self.description.clone(),
            personality: self.personality.clone(),
            scenario: self.scenario.clone(),
            first_message: self.first_message.clone(),
            example_dialogue: self.example_dialogue.clone(),
            system_prompt: self.system_prompt.clone(),
            avatar_url: None,
        }
    }

    fn to_update(&self) -> CharacterUpdate {
        CharacterUpdate {
            name: Some(self.name.clone()),
            description: Some(self.description.clone()),
            personality: Some(self.personality.clone()),
            scenario: Some(self.scenario.clone()),
            first_message: Some(self.first_message.clone()),
            example_dialogue: Some(self.example_dialogue.clone()),
            system_prompt: Some(self.system_prompt.clone()),
            avatar_url: None,
        }
    }
}

fn character_fields(draft: &UseStateHandle<CharacterDraft>) -> Html {
    let fields = [
        ("name", "Name", false),
        ("description", "Description", true),
        ("personality", "Personality", true),
        ("scenario", "Scenario", true),
        ("first_message", "First message", true),
        ("example_dialogue", "Example dialogue", true),
        ("system_prompt", "System prompt override", true),
    ];
    html! {
        <>
            { for fields.iter().map(|(key, label, multiline)| {
                let key = *key;
                let draft = draft.clone();
                html! {
                    <label class="field">
                        <span class="muted">{ *label }</span>
                        if *multiline {
                            <textarea value={draft_field(draft.clone(), key)} oninput={draft_oninput(draft, key, true)} />
                        } else {
                            <input type="text" value={draft_field(draft.clone(), key)} oninput={draft_oninput(draft, key, false)} />
                        }
                    </label>
                }
            }) }
        </>
    }
}

fn draft_field(draft: UseStateHandle<CharacterDraft>, key: &str) -> String {
    match key {
        "name" => draft.name.clone(),
        "description" => draft.description.clone(),
        "personality" => draft.personality.clone(),
        "scenario" => draft.scenario.clone(),
        "first_message" => draft.first_message.clone(),
        "example_dialogue" => draft.example_dialogue.clone(),
        "system_prompt" => draft.system_prompt.clone(),
        _ => String::new(),
    }
}

fn draft_oninput(
    draft: UseStateHandle<CharacterDraft>,
    key: &str,
    _multiline: bool,
) -> Callback<InputEvent> {
    let key = key.to_string();
    Callback::from(move |e: InputEvent| {
        let input: HtmlInputElement = e.target_unchecked_into();
        let value = input.value();
        let mut next = (*draft).clone();
        match key.as_str() {
            "name" => next.name = value,
            "description" => next.description = value,
            "personality" => next.personality = value,
            "scenario" => next.scenario = value,
            "first_message" => next.first_message = value,
            "example_dialogue" => next.example_dialogue = value,
            "system_prompt" => next.system_prompt = value,
            _ => {}
        }
        draft.set(next);
    })
}

#[derive(Clone, PartialEq)]
struct VariableRefreshSignal {
    chat_id: Option<i64>,
    message_signals: Vec<(i64, usize, Option<JobStatus>)>,
}

#[derive(Properties, PartialEq)]
struct VariablesPanelProps {
    chat_id: Option<i64>,
    messages: Vec<Message>,
}

#[function_component(VariablesPanel)]
fn variables_panel(props: &VariablesPanelProps) -> Html {
    let variables = use_state(Vec::<ChatVariable>::new);
    let key = use_state(String::new);
    let value = use_state(String::new);
    let editing_key = use_state(|| None::<String>);

    let refresh_signal = VariableRefreshSignal {
        chat_id: props.chat_id,
        message_signals: props
            .messages
            .iter()
            .map(|m| (m.id, m.variable_updates.len(), m.job_status))
            .collect(),
    };

    {
        let variables = variables.clone();
        use_effect_with(refresh_signal, move |signal| {
            if let Some(chat_id) = signal.chat_id {
                let variables = variables.clone();
                wasm_bindgen_futures::spawn_local(async move {
                    if let Ok(list) = api::get_variables(chat_id).await {
                        variables.set(list);
                    }
                });
            } else {
                variables.set(vec![]);
            }
            || ()
        });
    }

    let Some(chat_id) = props.chat_id else {
        return html! { <p class="muted">{"Select a chat to view variables."}</p> };
    };

    let clear_form = {
        let key = key.clone();
        let value = value.clone();
        let editing_key = editing_key.clone();
        Callback::from(move |_| {
            key.set(String::new());
            value.set(String::new());
            editing_key.set(None);
        })
    };

    html! {
        <div>
            <p class="muted">{"Chat variables are injected into the prompt. The model can update them with var tags."}</p>
            { for variables.iter().map(|v| {
                let variable_key = v.key.clone();
                let variable_value = v.value.clone();
                let chat_id_for_actions = chat_id;
                html! {
                    <div class="variable-card">
                        <div class="variable-card-header">
                            <strong>{ &v.key }</strong>
                            <div class="variable-card-actions">
                                <button class="btn secondary btn-compact" onclick={{
                                    let key = key.clone();
                                    let value = value.clone();
                                    let editing_key = editing_key.clone();
                                    let variable_key = variable_key.clone();
                                    let variable_value = variable_value.clone();
                                    Callback::from(move |_| {
                                        key.set(variable_key.clone());
                                        value.set(variable_value.clone());
                                        editing_key.set(Some(variable_key.clone()));
                                    })
                                }}>{"edit"}</button>
                                <button class="btn secondary btn-compact" onclick={{
                                    let variables = variables.clone();
                                    let variable_key = variable_key.clone();
                                    let key = key.clone();
                                    let value = value.clone();
                                    let editing_key = editing_key.clone();
                                    let chat_id = chat_id_for_actions;
                                    Callback::from(move |_| {
                                        let variables = variables.clone();
                                        let variable_key = variable_key.clone();
                                        let key = key.clone();
                                        let value = value.clone();
                                        let editing_key = editing_key.clone();
                                        wasm_bindgen_futures::spawn_local(async move {
                                            match api::delete_variable(chat_id, &variable_key).await {
                                                Ok(()) => {
                                                    if editing_key.as_ref() == Some(&variable_key) {
                                                        key.set(String::new());
                                                        value.set(String::new());
                                                        editing_key.set(None);
                                                    }
                                                    if let Ok(list) = api::get_variables(chat_id).await {
                                                        variables.set(list);
                                                    }
                                                }
                                                Err(err) => {
                                                    if let Some(window) = web_sys::window() {
                                                        let _ = window.alert_with_message(&format!(
                                                            "Could not delete variable: {err}"
                                                        ));
                                                    }
                                                }
                                            }
                                        });
                                    })
                                }}>{"delete"}</button>
                            </div>
                        </div>
                        <div class="variable-card-value">{ &v.value }</div>
                    </div>
                }
            }) }
            <label class="field">
                <span class="muted">{"Key"}</span>
                <input
                    value={(*key).clone()}
                    readonly={editing_key.is_some()}
                    oninput={input_callback(key.clone())}
                />
            </label>
            <label class="field"><span class="muted">{"Value"}</span><textarea value={(*value).clone()} oninput={input_callback(value.clone())} /></label>
            <div class="variable-form-actions">
                <button class="btn" onclick={{
                    let variables = variables.clone();
                    let key = key.clone();
                    let value = value.clone();
                    let editing_key = editing_key.clone();
                    Callback::from(move |_| {
                        if key.trim().is_empty() { return; }
                        let variables = variables.clone();
                        let k = (*key).clone();
                        let v = (*value).clone();
                        let key = key.clone();
                        let value = value.clone();
                        let editing_key = editing_key.clone();
                        wasm_bindgen_futures::spawn_local(async move {
                            match api::upsert_variable(chat_id, &k, &v).await {
                                Ok(_) => {
                                    key.set(String::new());
                                    value.set(String::new());
                                    editing_key.set(None);
                                    if let Ok(list) = api::get_variables(chat_id).await {
                                        variables.set(list);
                                    }
                                }
                                Err(err) => {
                                    if let Some(window) = web_sys::window() {
                                        let _ = window.alert_with_message(&format!(
                                            "Could not save variable: {err}"
                                        ));
                                    }
                                }
                            }
                        });
                    })
                }}>{ if editing_key.is_some() { "Update variable" } else { "Save variable" } }</button>
                if editing_key.is_some() {
                    <button class="btn secondary" onclick={clear_form.clone()}>{"Cancel"}</button>
                }
            </div>
        </div>
    }
}

fn input_callback(state: UseStateHandle<String>) -> Callback<InputEvent> {
    Callback::from(move |e: InputEvent| {
        let input: HtmlInputElement = e.target_unchecked_into();
        state.set(input.value());
    })
}

#[derive(Clone, PartialEq)]
enum SettingsSavePhase {
    Synced,
    Debouncing,
    Saving,
    Failed(String),
}

#[derive(Clone)]
struct SettingsSaveContext {
    draft: UseStateHandle<Option<Settings>>,
    draft_ref: Rc<RefCell<Option<Settings>>>,
    last_saved: UseStateHandle<Option<Settings>>,
    parent_settings: UseStateHandle<Option<Settings>>,
    phase: UseStateHandle<SettingsSavePhase>,
    save_timeout: Rc<RefCell<Option<Timeout>>>,
}

impl PartialEq for SettingsSaveContext {
    fn eq(&self, other: &Self) -> bool {
        self.draft == other.draft
            && self.last_saved == other.last_saved
            && self.parent_settings == other.parent_settings
            && self.phase == other.phase
    }
}

impl SettingsSaveContext {
    fn load_from(&self, settings: Settings) {
        *self.draft_ref.borrow_mut() = Some(settings.clone());
        self.draft.set(Some(settings.clone()));
        self.last_saved.set(Some(settings));
        self.phase.set(SettingsSavePhase::Synced);
    }

    fn ensure_loaded(&self) {
        if (*self.draft).is_none() {
            if let Some(settings) = (*self.parent_settings).clone() {
                self.load_from(settings);
            }
        }
    }

    fn is_dirty(&self) -> bool {
        self.draft_ref.borrow().as_ref() != (*self.last_saved).as_ref()
    }

    fn cancel_debounce(&self) {
        if let Some(timeout) = self.save_timeout.borrow_mut().take() {
            drop(timeout);
        }
    }

    fn mark_saved(&self, saved: Settings) {
        *self.draft_ref.borrow_mut() = Some(saved.clone());
        self.draft.set(Some(saved.clone()));
        self.last_saved.set(Some(saved.clone()));
        self.parent_settings.set(Some(saved));
        self.cancel_debounce();
        self.phase.set(SettingsSavePhase::Synced);
    }

    fn schedule_save(&self) {
        if !self.is_dirty() {
            self.cancel_debounce();
            if !matches!(*self.phase, SettingsSavePhase::Saving) {
                self.phase.set(SettingsSavePhase::Synced);
            }
            return;
        }

        self.cancel_debounce();
        self.phase.set(SettingsSavePhase::Debouncing);

        let ctx = self.clone();
        *self.save_timeout.borrow_mut() = Some(Timeout::new(400, move || {
            ctx.run_save();
        }));
    }

    fn run_save(&self) {
        let Some(sent) = self.draft_ref.borrow().clone() else {
            return;
        };
        if Some(&sent) == (*self.last_saved).as_ref() {
            self.phase.set(SettingsSavePhase::Synced);
            return;
        }

        self.phase.set(SettingsSavePhase::Saving);
        let ctx = self.clone();
        wasm_bindgen_futures::spawn_local(async move {
            let update = settings_to_update(&sent);
            match api::update_settings(&update).await {
                Ok(saved) if ctx.draft_ref.borrow().as_ref() == Some(&sent) => {
                    ctx.mark_saved(saved);
                }
                Ok(_) => {
                    ctx.phase.set(SettingsSavePhase::Synced);
                    ctx.schedule_save();
                }
                Err(err) if ctx.draft_ref.borrow().as_ref() == Some(&sent) => {
                    ctx.phase.set(SettingsSavePhase::Failed(err));
                }
                Err(_) => {
                    ctx.phase.set(SettingsSavePhase::Synced);
                    ctx.schedule_save();
                }
            }
        });
    }

    fn update_field<F>(&self, update: F)
    where
        F: FnOnce(&mut Settings),
    {
        let Some(mut current) = self.draft_ref.borrow().clone() else {
            return;
        };
        update(&mut current);
        let dirty = Some(&current) != (*self.last_saved).as_ref();
        *self.draft_ref.borrow_mut() = Some(current.clone());
        self.draft.set(Some(current));
        if matches!(*self.phase, SettingsSavePhase::Failed(_)) && dirty {
            self.phase.set(SettingsSavePhase::Debouncing);
        } else if matches!(*self.phase, SettingsSavePhase::Failed(_)) {
            self.phase.set(SettingsSavePhase::Synced);
        }
        if dirty {
            self.schedule_save();
        } else {
            self.cancel_debounce();
            self.phase.set(SettingsSavePhase::Synced);
        }
    }
}

fn settings_to_update(current: &Settings) -> SettingsUpdate {
    SettingsUpdate {
        inference_url: Some(current.inference_url.clone()),
        model: Some(current.model.clone()),
        temperature: Some(current.temperature),
        top_p: Some(current.top_p),
        max_tokens: Some(current.max_tokens),
        system_prompt_prefix: Some(current.system_prompt_prefix.clone()),
        system_prompt_suffix: Some(current.system_prompt_suffix.clone()),
        user_name: Some(current.user_name.clone()),
        persona_description: Some(current.persona_description.clone()),
        summarize_enabled: Some(current.summarize_enabled),
        summarize_adaptive: Some(current.summarize_adaptive),
        summarize_after_messages: Some(current.summarize_after_messages),
        summarize_keep_recent: Some(current.summarize_keep_recent),
        variables_enabled: Some(current.variables_enabled),
        thought_blocks_enabled: Some(current.thought_blocks_enabled),
        max_context_messages: Some(current.max_context_messages),
        context_tokens: Some(current.context_tokens),
        auto_context_on_model_change: Some(current.auto_context_on_model_change),
        max_concurrent_jobs: Some(current.max_concurrent_jobs),
    }
}

fn apply_detected_context(save_ctx: &SettingsSaveContext, caps: &ModelCapabilities) {
    let Some(ctx) = caps.context_length else {
        return;
    };
    save_ctx.update_field(|current| {
        if !current.auto_context_on_model_change {
            return;
        }
        current.context_tokens = ctx;
        if current.max_tokens >= ctx {
            current.max_tokens = suggested_response_tokens(ctx);
        }
    });
}

fn settings_save_status_html(phase: &SettingsSavePhase) -> Html {
    match phase {
        SettingsSavePhase::Saving => html! {
            <span class="settings-save-indicator settings-save-indicator--saving">
                <span class="settings-save-spinner" aria-hidden="true"></span>
                <span>{"Saving…"}</span>
            </span>
        },
        SettingsSavePhase::Failed(message) => html! {
            <span class="settings-save-indicator settings-save-indicator--error" title={message.clone()}>
                <span class="settings-save-icon" aria-hidden="true">{"✕"}</span>
                <span>{"Save failed"}</span>
            </span>
        },
        SettingsSavePhase::Debouncing => html! {
            <span class="settings-save-indicator settings-save-indicator--pending">
                <span class="settings-save-icon" aria-hidden="true">{"⏳"}</span>
            </span>
        },
        SettingsSavePhase::Synced => html! {
            <span class="settings-save-indicator settings-save-indicator--saved">
                <span class="settings-save-icon" aria-hidden="true">{"✓"}</span>
                <span>{"Saved"}</span>
            </span>
        },
    }
}

#[derive(Properties, PartialEq)]
struct SettingsPanelProps {
    save_ctx: SettingsSaveContext,
    install_kind: InstallKind,
    on_install_change: Callback<()>,
}

#[function_component(SettingsPanel)]
fn settings_panel(props: &SettingsPanelProps) -> Html {
    let save_ctx = props.save_ctx.clone();
    let draft = save_ctx.draft.clone();
    let phase = save_ctx.phase.clone();
    let models = use_state(Vec::<ModelInfo>::new);
    let model_error = use_state(|| None::<String>);
    let detected_caps = use_state(|| None::<ModelCapabilities>);
    let caps_busy = use_state(|| false);

    let Some(s) = (*draft).clone() else {
        return html! { <p class="muted">{"Loading settings…"}</p> };
    };

    let prompt_budget = prompt_token_budget(s.context_tokens, s.max_tokens);

    html! {
        <div>
            <div class="settings-status">
                { settings_save_status_html(&phase) }
            </div>
            <label class="field">
                <span class="muted">{"Inference server"}</span>
                <input value={s.inference_url.clone()} oninput={{
                    let save_ctx = save_ctx.clone();
                    Callback::from(move |e: InputEvent| {
                        let input: HtmlInputElement = e.target_unchecked_into();
                        save_ctx.update_field(|current| current.inference_url = input.value());
                    })
                }} />
            </label>
            <div class="settings-model-row">
                <label class="field">
                    <span class="muted">{"Model"}</span>
                    <select title={s.model.clone()} onchange={{
                        let save_ctx = save_ctx.clone();
                        let detected_caps = detected_caps.clone();
                        let caps_busy = caps_busy.clone();
                        Callback::from(move |e: Event| {
                            let input: HtmlInputElement = e.target_unchecked_into();
                            let model = input.value();
                            save_ctx.update_field(|current| current.model = model.clone());
                            if model.is_empty() {
                                detected_caps.set(None);
                                return;
                            }
                            let save_ctx = save_ctx.clone();
                            let detected_caps = detected_caps.clone();
                            let caps_busy = caps_busy.clone();
                            caps_busy.set(true);
                            wasm_bindgen_futures::spawn_local(async move {
                                match api::get_model_capabilities(&model).await {
                                    Ok(caps) => {
                                        apply_detected_context(&save_ctx, &caps);
                                        detected_caps.set(Some(caps));
                                    }
                                    Err(_) => detected_caps.set(None),
                                }
                                caps_busy.set(false);
                            });
                        })
                    }}>
                        <option value="">{"Select a model"}</option>
                        { for models.iter().map(|m| html! { <option value={m.id.clone()} selected={m.id == s.model}>{ m.name.clone().unwrap_or(m.id.clone()) }</option> }) }
                    </select>
                    if !s.model.is_empty() {
                        <p class="settings-model-name muted">{
                            models.iter()
                                .find(|m| m.id == s.model)
                                .map(|m| m.name.clone().unwrap_or(m.id.clone()))
                                .unwrap_or_else(|| s.model.clone())
                        }</p>
                    }
                </label>
                <button class="btn secondary" onclick={{
                    let models = models.clone();
                    let model_error = model_error.clone();
                    Callback::from(move |_| {
                        let models = models.clone();
                        let model_error = model_error.clone();
                        wasm_bindgen_futures::spawn_local(async move {
                            match api::list_models().await {
                                Ok(list) => {
                                    model_error.set(None);
                                    models.set(list);
                                }
                                Err(err) => model_error.set(Some(err)),
                            }
                        });
                    })
                }}>{"Refresh"}</button>
            </div>
            if let Some(err) = &*model_error {
                <p class="text-danger">{ err }</p>
            }
            if *caps_busy {
                <p class="muted">{"Detecting model context…"}</p>
            } else if let Some(caps) = &*detected_caps {
                if let Some(ctx) = caps.context_length {
                    <p class="muted">{
                        format!(
                            "Detected context: {} tokens ({})",
                            ctx,
                            caps.context_source.clone().unwrap_or_else(|| "unknown".into())
                        )
                    }</p>
                } else {
                    <p class="muted">{"Could not detect context for this backend — set it manually."}</p>
                }
            }
            <div class="settings-group">
                <strong>{"Context budget"}</strong>
                <p class="muted" style="margin:0.35rem 0 0.5rem;">
                    {"Total context is split between the prompt (history + character) and the response. Lower response length leaves more room for chat history."}
                </p>
                <label style="display:flex;gap:0.5rem;align-items:center;margin:0.5rem 0;">
                    <input type="checkbox" checked={s.auto_context_on_model_change} onclick={{
                        let save_ctx = save_ctx.clone();
                        Callback::from(move |_| {
                            save_ctx.update_field(|current| {
                                current.auto_context_on_model_change = !current.auto_context_on_model_change;
                            });
                        })
                    }} />
                    {"Auto-set context when model changes"}
                </label>
                <div class="settings-params-grid">
                    <label class="field">
                        <span class="muted">{"Context (tokens)"}</span>
                        <input type="number" value={s.context_tokens.to_string()} oninput={num_input(save_ctx.clone(), "context_tokens")} />
                    </label>
                    <label class="field">
                        <span class="muted">{"Response length (tokens)"}</span>
                        <input type="number" value={s.max_tokens.to_string()} oninput={num_input(save_ctx.clone(), "max_tokens")} />
                    </label>
                    <label class="field">
                        <span class="muted">{"Max history messages"}</span>
                        <input type="number" value={s.max_context_messages.to_string()} oninput={num_input(save_ctx.clone(), "max_context_messages")} />
                    </label>
                </div>
                if s.context_tokens > 0 {
                    <p class="muted" style="margin:0;">
                        { format!("Prompt budget: ~{prompt_budget} tokens (context − response)") }
                    </p>
                }
                <button class="btn secondary" style="margin-top:0.5rem;" disabled={s.model.is_empty() || *caps_busy} onclick={{
                    let save_ctx = save_ctx.clone();
                    let detected_caps = detected_caps.clone();
                    let caps_busy = caps_busy.clone();
                    let model = s.model.clone();
                    Callback::from(move |_| {
                        if model.is_empty() {
                            return;
                        }
                        let save_ctx = save_ctx.clone();
                        let detected_caps = detected_caps.clone();
                        let caps_busy = caps_busy.clone();
                        let model = model.clone();
                        caps_busy.set(true);
                        wasm_bindgen_futures::spawn_local(async move {
                            match api::get_model_capabilities(&model).await {
                                Ok(caps) => {
                                    apply_detected_context(&save_ctx, &caps);
                                    detected_caps.set(Some(caps));
                                }
                                Err(_) => detected_caps.set(None),
                            }
                            caps_busy.set(false);
                        });
                    })
                }}>{"Detect context from backend"}</button>
            </div>
            <div class="settings-params-grid">
                <label class="field"><span class="muted">{"Temperature"}</span><input type="number" step="0.05" value={s.temperature.to_string()} oninput={num_input(save_ctx.clone(), "temperature")} /></label>
                <label class="field"><span class="muted">{"Top P"}</span><input type="number" step="0.05" value={s.top_p.to_string()} oninput={num_input(save_ctx.clone(), "top_p")} /></label>
                <label class="field"><span class="muted">{"Max concurrent jobs"}</span><input type="number" value={s.max_concurrent_jobs.to_string()} oninput={num_input(save_ctx.clone(), "max_concurrent_jobs")} /></label>
            </div>
            <label class="field"><span class="muted">{"User name ({{user}})"}</span><input value={s.user_name.clone()} oninput={text_input(save_ctx.clone(), "user_name")} /></label>
            <label class="field"><span class="muted">{"Persona description ({{persona}})"}</span><textarea value={s.persona_description.clone()} rows="3" oninput={text_input(save_ctx.clone(), "persona_description")} /></label>
            <label class="field"><span class="muted">{"Main prompt (prefix)"}</span><textarea value={s.system_prompt_prefix.clone()} rows="3" oninput={text_input(save_ctx.clone(), "system_prompt_prefix")} /></label>
            <label class="field"><span class="muted">{"Post-history instructions (suffix)"}</span><textarea value={s.system_prompt_suffix.clone()} rows="3" oninput={text_input(save_ctx.clone(), "system_prompt_suffix")} /></label>
            <div class="settings-group">
                <strong>{"Auto summarize"}</strong>
                <p class="muted" style="margin:0.35rem 0 0.5rem;">
                    {"Folds older messages into a summary for the model context window. Your full chat history stays visible; summarized messages are omitted from future prompts. Summarization runs as a queued job and appears as a break in the chat."}
                </p>
                <label style="display:flex;gap:0.5rem;margin:0.5rem 0;">
                    <input type="checkbox" checked={s.summarize_enabled} onclick={{
                        let save_ctx = save_ctx.clone();
                        Callback::from(move |_| {
                            save_ctx.update_field(|current| current.summarize_enabled = !current.summarize_enabled);
                        })
                    }} />
                    {"Enable summarization"}
                </label>
                <label style="display:flex;gap:0.5rem;margin:0.5rem 0;">
                    <input type="checkbox" checked={s.summarize_adaptive} disabled={!s.summarize_enabled} onclick={{
                        let save_ctx = save_ctx.clone();
                        Callback::from(move |_| {
                            save_ctx.update_field(|current| current.summarize_adaptive = !current.summarize_adaptive);
                        })
                    }} />
                    {"Adapt to context window (uses context − response budget)"}
                </label>
                <label class="field"><span class="muted">{"Minimum messages before summarize"}</span><input type="number" value={s.summarize_after_messages.to_string()} oninput={num_input(save_ctx.clone(), "summarize_after_messages")} /></label>
                <label class="field"><span class="muted">{"Minimum recent messages to keep"}</span><input type="number" value={s.summarize_keep_recent.to_string()} oninput={num_input(save_ctx.clone(), "summarize_keep_recent")} /></label>
            </div>
            <label style="display:flex;gap:0.5rem;align-items:center;margin-bottom:0.75rem;">
                <input type="checkbox" checked={s.variables_enabled} onclick={{
                    let save_ctx = save_ctx.clone();
                    Callback::from(move |_| {
                        save_ctx.update_field(|current| current.variables_enabled = !current.variables_enabled);
                    })
                }} />
                {"Enable chat variables in prompts"}
            </label>
            <label style="display:flex;gap:0.5rem;align-items:center;margin-bottom:0.75rem;">
                <input type="checkbox" checked={s.thought_blocks_enabled} onclick={{
                    let save_ctx = save_ctx.clone();
                    Callback::from(move |_| {
                        save_ctx.update_field(|current| current.thought_blocks_enabled = !current.thought_blocks_enabled);
                    })
                }} />
                {"Extract reasoning into collapsible thought block"}
            </label>
            <InstallSettings
                kind={props.install_kind}
                on_change={props.on_install_change.clone()}
            />
            <NotificationSettings />
        </div>
    }
}

#[function_component(NotificationSettings)]
fn notification_settings() -> Html {
    let enabled = use_state(notifications::is_enabled);
    let permission = use_state(notifications::permission_label);
    let busy = use_state(|| false);
    let supported = notifications::is_supported();

    let toggle = {
        let enabled = enabled.clone();
        let permission = permission.clone();
        let busy = busy.clone();
        Callback::from(move |_| {
            if *busy || !supported {
                return;
            }
            let next = !*enabled;
            if next {
                busy.set(true);
                let enabled = enabled.clone();
                let permission = permission.clone();
                let busy = busy.clone();
                wasm_bindgen_futures::spawn_local(async move {
                    let granted = notifications::request_permission().await;
                    if granted {
                        notifications::set_enabled(true);
                        enabled.set(true);
                        notifications::show_test_notification();
                    }
                    permission.set(notifications::permission_label());
                    busy.set(false);
                });
            } else {
                notifications::set_enabled(false);
                enabled.set(false);
            }
        })
    };

    html! {
        <div class="settings-group">
            <strong>{"Browser notifications"}</strong>
            <p class="muted" style="margin:0.35rem 0 0.5rem;">
                {"Get notified when a queued generation finishes while you are in another tab or chat."}
            </p>
            if !supported {
                <p class="muted">{"This browser does not support notifications."}</p>
            } else {
                <label style="display:flex;gap:0.5rem;align-items:center;margin:0.5rem 0;">
                    <input
                        type="checkbox"
                        checked={*enabled}
                        disabled={*busy || notifications::permission_denied()}
                        onclick={toggle}
                    />
                    if *busy {
                        <span>{"Requesting permission…"}</span>
                    } else {
                        <span>{"Notify when generations complete"}</span>
                    }
                </label>
                <p class="muted" style="margin:0;">
                    { format!("Permission: {}", *permission) }
                </p>
                if *enabled && notifications::permission_granted() {
                    <button
                        class="btn secondary"
                        style="margin-top:0.5rem;"
                        onclick={Callback::from(|_| notifications::show_test_notification())}
                    >
                        {"Send test notification"}
                    </button>
                }
            }
        </div>
    }
}

fn num_input(save_ctx: SettingsSaveContext, field: &'static str) -> Callback<InputEvent> {
    Callback::from(move |e: InputEvent| {
        let input: HtmlInputElement = e.target_unchecked_into();
        if let Ok(v) = input.value().parse::<f64>() {
            save_ctx.update_field(move |current| match field {
                "temperature" => current.temperature = v,
                "top_p" => current.top_p = v,
                "max_tokens" => current.max_tokens = (v as i64).max(1),
                "context_tokens" => current.context_tokens = (v as i64).max(0),
                "max_context_messages" => current.max_context_messages = (v as i64).max(0),
                "max_concurrent_jobs" => current.max_concurrent_jobs = (v as i64).max(1),
                "summarize_after_messages" => current.summarize_after_messages = v as i64,
                "summarize_keep_recent" => current.summarize_keep_recent = v as i64,
                _ => {}
            });
        }
    })
}

fn text_input(save_ctx: SettingsSaveContext, field: &'static str) -> Callback<InputEvent> {
    Callback::from(move |e: InputEvent| {
        let input: HtmlInputElement = e.target_unchecked_into();
        let value = input.value();
        save_ctx.update_field(move |current| match field {
            "system_prompt_prefix" => current.system_prompt_prefix = value.clone(),
            "system_prompt_suffix" => current.system_prompt_suffix = value.clone(),
            "user_name" => current.user_name = value.clone(),
            "persona_description" => current.persona_description = value.clone(),
            _ => {}
        });
    })
}

#[cfg(test)]
mod chat_sort_tests {
    use super::*;

    fn sample_chat(id: i64, title: &str, updated_at: &str) -> Chat {
        let mut chat: Chat = serde_json::from_value(serde_json::json!({
            "id": id,
            "title": title,
            "character_id": 1,
            "character_name": "Test",
            "summary": "",
            "created_at": "2026-01-01T00:00:00Z",
            "updated_at": updated_at,
            "queued_jobs": 0,
        }))
        .expect("sample chat");
        chat.active_job = None;
        chat
    }

    fn archived_chat(id: i64, title: &str, archived_at: &str) -> Chat {
        let mut chat: Chat = serde_json::from_value(serde_json::json!({
            "id": id,
            "title": title,
            "character_id": 1,
            "character_name": "Test",
            "summary": "",
            "created_at": "2026-01-01T00:00:00Z",
            "updated_at": "2026-01-01T00:00:00Z",
            "archived_at": archived_at,
            "queued_jobs": 0,
        }))
        .expect("archived chat");
        chat.active_job = None;
        chat
    }

    #[test]
    fn sort_chats_orders_by_updated_at_then_title() {
        let chats = sort_chats(vec![
            sample_chat(1, "Bravo", "2026-01-02T00:00:00Z"),
            sample_chat(2, "Alpha", "2026-01-03T00:00:00Z"),
            sample_chat(3, "Charlie", "2026-01-02T00:00:00Z"),
        ]);
        assert_eq!(
            chats.iter().map(|chat| chat.id).collect::<Vec<_>>(),
            vec![2, 1, 3]
        );
    }

    #[test]
    fn sort_chats_uses_case_insensitive_title_tiebreaker() {
        let chats = sort_chats(vec![
            sample_chat(1, "bravo", "2026-01-02T00:00:00Z"),
            sample_chat(2, "Alpha", "2026-01-02T00:00:00Z"),
            sample_chat(3, "ALPHA", "2026-01-02T00:00:00Z"),
        ]);
        assert_eq!(
            chats.iter().map(|chat| chat.id).collect::<Vec<_>>(),
            vec![2, 3, 1]
        );
    }

    #[test]
    fn sort_archived_chats_orders_by_archived_at_then_title() {
        let chats = sort_archived_chats(vec![
            archived_chat(1, "Bravo", "2026-01-02T00:00:00Z"),
            archived_chat(2, "Alpha", "2026-01-03T00:00:00Z"),
            archived_chat(3, "Charlie", "2026-01-02T00:00:00Z"),
        ]);
        assert_eq!(
            chats.iter().map(|chat| chat.id).collect::<Vec<_>>(),
            vec![2, 1, 3]
        );
    }
}

#[cfg(test)]
mod generation_error_tests {
    use super::*;

    fn sample_message(content: &str) -> Message {
        let mut message: Message = serde_json::from_value(serde_json::json!({
            "id": 1,
            "chat_id": 1,
            "role": "assistant",
            "content": content,
            "is_summary": false,
            "in_summary": false,
            "created_at": "2026-01-01T00:00:00Z",
        }))
        .expect("sample message");
        message.generation_error = None;
        message
    }

    #[test]
    fn reads_generation_error_field() {
        let mut message = sample_message("Partial");
        message.generation_error = Some("connection reset".to_string());
        assert_eq!(
            message_generation_error(&message),
            Some("connection reset".to_string())
        );
    }

    #[test]
    fn parses_legacy_failure_placeholder() {
        let message = sample_message("[Generation failed: timeout]");
        assert_eq!(
            message_generation_error(&message),
            Some("timeout".to_string())
        );
        assert!(legacy_failure_only_message(&message));
    }
}

#[wasm_bindgen::prelude::wasm_bindgen(start)]
pub fn main() {
    APP.with(|app| {
        if let Some(handle) = app.borrow_mut().take() {
            handle.destroy();
        }
        *app.borrow_mut() = Some(yew::Renderer::<App>::new().render());
    });
}
