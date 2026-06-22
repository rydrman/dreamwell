mod api;
mod app_sync;
mod auth;
mod auto_grow;
mod chat_sync;
mod dice_ui;
mod game_create_ui;
mod game_presets_ui;
mod game_setup_ui;
mod game_sync;
mod game_ui;
mod generation_ui;
mod install;
mod item_list;
mod markdown;
mod message_menu;
mod notifications;
mod queue_ui;
mod resume_policy;
mod router;
mod scenario_ui;
mod sidebar;
mod sse_client;
mod state_ui;
mod stories_ui;
mod story_save;
mod story_sync;
mod summary_ui;
mod title_editor;
mod variables;
mod variables_ui;
mod view_scroll;

use std::cell::RefCell;
use std::rc::Rc;

thread_local! {
    static APP: RefCell<Option<yew::AppHandle<App>>> = const { RefCell::new(None) };
}

use wasm_bindgen::closure::Closure;
use wasm_bindgen::JsCast;

use chat_sync::{message_generation_live, messages_stale_vs_chat, should_apply_messages_from_sse};
use dreamwell_types::*;
use game_create_ui::GameCreateModal;
use game_ui::GameShell;
use generation_ui::{
    composer_notice, generation_error_message, GenerationNotice, GenerationStatusBar,
};
use gloo_timers::callback::Timeout;
use install::InstallSettings;
use item_list::ChatList;
use message_menu::MessageOptionsMenu;
use queue_ui::{AppMode, QueueBar, QueuePage, TopBarQueueButton};
use router::{use_router, AppRoute, Overlay, StoryNav};
use scenario_ui::{default_game_title, game_create_from_scenario, ScenariosPage};
use sidebar::AppSidebar;
use state_ui::{PhaseSection, PlanBeatsList, StateChangesList, StateEntriesPanel, StateEntryRow};
use stories_ui::StoriesShell;
use story_save::{AutoSaveField, AutoSavePhase};
use story_sync::{FetchGeneration, AUTOSAVE_DEBOUNCE_MS};
use summary_ui::{
    chat_summarize_in_progress, is_chat_summarize_pending, SummaryBreak, SummaryKind, SummaryView,
    CHAT_SUMMARIZE_PLACEHOLDER,
};
use title_editor::TitleEditor;
use variables_ui::{
    chat_scope_options, chat_variable_row, make_chat_variable_handlers, VariableList,
    VariableRowModel, MANUAL_MESSAGE_SOURCE,
};
use view_scroll::{
    mobile_scroll_chrome_active, scroll_content_view_to_bottom, update_mobile_sidebar_inset,
    window_scroll_y,
};
use web_sys::{HtmlElement, HtmlInputElement};
use yew::prelude::*;

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

fn game_id_from_route(route: &AppRoute) -> Option<i64> {
    match route {
        AppRoute::Games { game_id, .. } => *game_id,
        _ => None,
    }
}

fn scenarios_route_context(route: &AppRoute) -> (Option<i64>, Option<i64>) {
    match route {
        AppRoute::Scenarios {
            scenario_id,
            game_id,
            ..
        } => (*scenario_id, *game_id),
        _ => (None, None),
    }
}

fn characters_route_context(route: &AppRoute) -> (Option<i64>, Option<i64>) {
    match route {
        AppRoute::Characters {
            character_id,
            chat_id,
            ..
        } => (*character_id, *chat_id),
        _ => (None, None),
    }
}

fn resolve_characters_selected_id(
    character_id: Option<i64>,
    chat_id: Option<i64>,
    chats: &[Chat],
) -> Option<i64> {
    if let Some(id) = character_id.filter(|id| *id != 0) {
        return Some(id);
    }
    chat_id.and_then(|chat_id| {
        chats
            .iter()
            .find(|chat| chat.id == chat_id)
            .map(|chat| chat.character_id)
            .filter(|id| *id != 0)
    })
}

fn sidebar_open_from_route(route: &AppRoute) -> bool {
    matches!(
        route,
        AppRoute::Chats { sidebar: true, .. }
            | AppRoute::Stories { sidebar: true, .. }
            | AppRoute::Games { sidebar: true, .. }
            | AppRoute::Queue { sidebar: true }
            | AppRoute::Settings { sidebar: true }
            | AppRoute::Characters { sidebar: true, .. }
            | AppRoute::Scenarios { sidebar: true, .. }
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

fn finalize_chat_list(current: &[Chat], next: Vec<Chat>, keep_chat_id: Option<i64>) -> Vec<Chat> {
    let mut next = sort_chats(next);
    if let Some(id) = keep_chat_id {
        if !next.iter().any(|chat| chat.id == id) {
            if let Some(chat) = current.iter().find(|chat| chat.id == id) {
                next.push(chat.clone());
                next = sort_chats(next);
            }
        }
    }
    next
}

fn publish_chats(chats: &UseStateHandle<Vec<Chat>>, next: Vec<Chat>, keep_chat_id: Option<i64>) {
    let current = (**chats).clone();
    let next = finalize_chat_list(&current, next, keep_chat_id);
    if current != next {
        cache_chat_list(CHATS_LIST_CACHE_KEY, &next);
        chats.set(next);
    }
}

fn merge_chat_into_list(mut chats: Vec<Chat>, updated: Chat) -> Vec<Chat> {
    if let Some(chat) = chats.iter_mut().find(|chat| chat.id == updated.id) {
        *chat = updated;
    } else {
        chats.push(updated);
    }
    chats
}

/// Merge one chat into the sidebar list without re-sorting.
///
/// Stream updates touch `updated_at` frequently; re-sorting on every payload
/// makes the list jump even when the visible order should stay put.
fn update_chat_in_list(chats: &UseStateHandle<Vec<Chat>>, updated: Chat) {
    cache_chat_snapshot(&updated);
    let current = (**chats).clone();
    let next = merge_chat_into_list(current.clone(), updated);
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

fn publish_archived_stories(archived_stories: &UseStateHandle<Vec<Story>>, next: Vec<Story>) {
    let next = sort_archived_stories(next);
    if **archived_stories != next {
        archived_stories.set(next);
    }
}

fn publish_archived_games(archived_games: &UseStateHandle<Vec<Game>>, next: Vec<Game>) {
    let next = sort_archived_games(next);
    if **archived_games != next {
        archived_games.set(next);
    }
}

fn spawn_gated_messages_fetch(
    chat_id: i64,
    messages: &UseStateHandle<Vec<Message>>,
    messages_loading: &UseStateHandle<bool>,
    messages_fetch_gen: &UseStateHandle<u64>,
    show_loading: bool,
) {
    let generation = FetchGeneration::from_raw(**messages_fetch_gen).bump();
    messages_fetch_gen.set(generation.raw());
    if show_loading {
        messages_loading.set(true);
    }
    let messages = messages.clone();
    let messages_loading = messages_loading.clone();
    let messages_fetch_gen = messages_fetch_gen.clone();
    wasm_bindgen_futures::spawn_local(async move {
        let result = api::get_messages(chat_id).await;
        let latest = FetchGeneration::from_raw(*messages_fetch_gen);
        if generation.is_current(latest) {
            if let Ok(msgs) = result {
                messages.set(msgs);
            }
            messages_loading.set(false);
        }
    });
}

#[function_component(App)]
fn app() -> Html {
    let router = use_router();
    let route = router.route();
    let mode = route.mode();
    let selected_chat_id = chat_id_from_route(&route);
    let _selected_story_id = story_id_from_route(&route);
    let chats = use_state(Vec::<Chat>::new);
    let stories = use_state(Vec::<Story>::new);
    let games = use_state(Vec::<Game>::new);
    let setup_scenario = use_state(|| None::<Scenario>);
    let archived_chats = use_state(Vec::<Chat>::new);
    let archived_stories = use_state(Vec::<Story>::new);
    let archived_games = use_state(Vec::<Game>::new);
    let characters = use_state(Vec::<Character>::new);
    let messages = use_state(Vec::<Message>::new);
    let messages_loading = use_state(|| false);
    let messages_fetch_gen = use_state(|| 0u64);
    let settings = use_state(|| None::<Settings>);
    let queue = use_state(|| None::<QueueStatus>);
    let loading = use_state(|| true);
    let auth_expired = use_state(|| false);
    let refresh_generation = use_state(|| 0u32);
    let chat_stream_nudge = use_mut_ref(|| None::<api::StreamNudge>);
    let summarize_watch = use_mut_ref(|| None::<i64>);
    let job_tracker = use_mut_ref(notifications::JobCompletionTracker::new);
    let installed = use_state(install::is_installed);

    {
        let installed = installed.clone();
        use_effect_with((), move |_| {
            install::init(Callback::from(move |_| {
                installed.set(install::is_installed());
            }));
            || ()
        });
    }

    let _installed = *installed;

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
        let archived_stories = archived_stories.clone();
        let archived_games = archived_games.clone();
        let characters = characters.clone();
        let settings = settings.clone();
        let stories = stories.clone();
        let games = games.clone();
        let loading = loading.clone();
        let auth_expired = auth_expired.clone();
        let router = router.clone();
        use_effect_with((), move |_| {
            wasm_bindgen_futures::spawn_local(async move {
                let cached_chats = load_cached_chats();
                let cached_archived = load_cached_archived_chats();

                let mut chat_list = Vec::<Chat>::new();
                match api::list_chats().await {
                    Ok(list) => {
                        chat_list = sort_chats(list);
                        publish_chats(
                            &chats,
                            chat_list.clone(),
                            chat_id_from_route(&router.route()),
                        );
                    }
                    Err(ref err) if api::is_auth_expired(err) => {
                        auth_expired.set(true);
                        loading.set(false);
                        return;
                    }
                    Err(_) if !cached_chats.is_empty() => {
                        chat_list = cached_chats.clone();
                        publish_chats(
                            &chats,
                            chat_list.clone(),
                            chat_id_from_route(&router.route()),
                        );
                    }
                    Err(_) => {}
                }
                match api::list_archived_chats().await {
                    Ok(list) => publish_archived_chats(&archived_chats, list),
                    Err(ref err) if api::is_auth_expired(err) => {
                        auth_expired.set(true);
                        loading.set(false);
                        return;
                    }
                    Err(_) if !cached_archived.is_empty() => {
                        publish_archived_chats(&archived_chats, cached_archived);
                    }
                    Err(_) => {}
                }
                match api::list_characters().await {
                    Ok(list) => characters.set(list),
                    Err(ref err) if api::is_auth_expired(err) => {
                        auth_expired.set(true);
                        loading.set(false);
                        return;
                    }
                    Err(_) => {}
                }
                match api::get_settings().await {
                    Ok(s) => settings.set(Some(s)),
                    Err(ref err) if api::is_auth_expired(err) => {
                        auth_expired.set(true);
                        loading.set(false);
                        return;
                    }
                    Err(_) => {}
                }
                match api::list_stories().await {
                    Ok(list) => stories.set(list),
                    Err(ref err) if api::is_auth_expired(err) => {
                        auth_expired.set(true);
                        loading.set(false);
                        return;
                    }
                    Err(_) => {}
                }
                match api::list_archived_stories().await {
                    Ok(list) => publish_archived_stories(&archived_stories, list),
                    Err(ref err) if api::is_auth_expired(err) => {
                        auth_expired.set(true);
                        loading.set(false);
                        return;
                    }
                    Err(_) => {}
                }
                match api::list_games().await {
                    Ok(list) => games.set(list),
                    Err(ref err) if api::is_auth_expired(err) => {
                        auth_expired.set(true);
                        loading.set(false);
                        return;
                    }
                    Err(_) => {}
                }
                match api::list_archived_games().await {
                    Ok(list) => publish_archived_games(&archived_games, list),
                    Err(ref err) if api::is_auth_expired(err) => {
                        auth_expired.set(true);
                        loading.set(false);
                        return;
                    }
                    Err(_) => {}
                }
                loading.set(false);

                if !chat_list.is_empty() {
                    match router.route() {
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
        let messages_fetch_gen = messages_fetch_gen.clone();
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
                    spawn_gated_messages_fetch(
                        chat_id,
                        &messages,
                        &messages_loading,
                        &messages_fetch_gen,
                        true,
                    );
                    let messages = messages.clone();
                    let messages_fetch_gen = messages_fetch_gen.clone();
                    let messages_loading = messages_loading.clone();
                    let chats = chats.clone();
                    let summarize_watch = summarize_watch.clone();
                    let had_active_job = Rc::new(RefCell::new(false));
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
                        let was_active = *had_active_job.borrow();
                        let now_active = payload.active_job.is_some();
                        if now_active {
                            *had_active_job.borrow_mut() = true;
                        }
                        let job_just_finished = was_active && !now_active;
                        update_chat_in_list(&chats, payload.chat.clone());
                        if job_just_finished {
                            if should_apply_messages_from_sse(&payload.messages, &payload.chat) {
                                messages.set(payload.messages.clone());
                                messages_loading.set(false);
                            }
                            spawn_gated_messages_fetch(
                                chat_id,
                                &messages,
                                &messages_loading,
                                &messages_fetch_gen,
                                false,
                            );
                            *had_active_job.borrow_mut() = false;
                        } else if should_apply_messages_from_sse(&payload.messages, &payload.chat) {
                            messages.set(payload.messages.clone());
                            messages_loading.set(false);
                        }
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
        let messages = messages.clone();
        let messages_loading = messages_loading.clone();
        let messages_fetch_gen = messages_fetch_gen.clone();
        let chats = chats.clone();
        use_effect_with(
            (selected_chat_id, (*chats).clone(), (*messages).clone()),
            move |(chat_id, chats, message_list)| {
                if let Some(chat_id) = *chat_id {
                    if let Some(chat) = chats.iter().find(|chat| chat.id == chat_id) {
                        if messages_stale_vs_chat(message_list, chat) {
                            spawn_gated_messages_fetch(
                                chat_id,
                                &messages,
                                &messages_loading,
                                &messages_fetch_gen,
                                false,
                            );
                        }
                    }
                }
                || ()
            },
        );
    }

    {
        let chat_stream_nudge = chat_stream_nudge.clone();
        let chats = chats.clone();
        let archived_chats = archived_chats.clone();
        let queue = queue.clone();
        let stories = stories.clone();
        let router = router.clone();
        let messages = messages.clone();
        let messages_loading = messages_loading.clone();
        let messages_fetch_gen = messages_fetch_gen.clone();
        use_effect_with((), move |_| {
            let guard = app_sync::register_scope(
                {
                    let chats = chats.clone();
                    let archived_chats = archived_chats.clone();
                    let queue = queue.clone();
                    let stories = stories.clone();
                    let router = router.clone();
                    let chat_stream_nudge = chat_stream_nudge.clone();
                    let messages = messages.clone();
                    let messages_loading = messages_loading.clone();
                    let messages_fetch_gen = messages_fetch_gen.clone();
                    move |ctx| {
                        if let Some(nudge) = chat_stream_nudge.borrow().clone() {
                            if ctx.force() {
                                nudge.reconnect();
                            } else {
                                nudge.resume();
                            }
                        }
                        if let Some(chat_id) = chat_id_from_route(&router.route()) {
                            spawn_gated_messages_fetch(
                                chat_id,
                                &messages,
                                &messages_loading,
                                &messages_fetch_gen,
                                false,
                            );
                        }
                        let queue = queue.clone();
                        let chats = chats.clone();
                        let archived_chats = archived_chats.clone();
                        let stories = stories.clone();
                        let keep_chat_id = chat_id_from_route(&router.route());
                        wasm_bindgen_futures::spawn_local(async move {
                            if let Ok(status) = api::get_queue().await {
                                queue.set(Some(status));
                            }
                            if let Ok(list) = api::list_chats().await {
                                publish_chats(&chats, list, keep_chat_id);
                            }
                            if let Ok(list) = api::list_archived_chats().await {
                                publish_archived_chats(&archived_chats, list);
                            }
                            if let Ok(list) = api::list_stories().await {
                                stories.set(list);
                            }
                        });
                    }
                },
                {
                    let chat_stream_nudge = chat_stream_nudge.clone();
                    move || {
                        if let Some(nudge) = chat_stream_nudge.borrow().clone() {
                            nudge.pause();
                        }
                    }
                },
            );
            move || drop(guard)
        });
    }

    {
        use_effect_with((), move |_| app_sync::install_lifecycle());
    }

    let load_messages_for_chat = {
        let messages = messages.clone();
        let messages_loading = messages_loading.clone();
        let messages_fetch_gen = messages_fetch_gen.clone();
        Callback::from(move |chat_id: i64| {
            spawn_gated_messages_fetch(
                chat_id,
                &messages,
                &messages_loading,
                &messages_fetch_gen,
                true,
            );
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
        let messages = messages.clone();
        let messages_loading = messages_loading.clone();
        let messages_fetch_gen = messages_fetch_gen.clone();
        use_effect_with((), move |_| {
            let queue = queue.clone();
            let chats = chats.clone();
            let archived_chats = archived_chats.clone();
            let stories = stories.clone();
            let router = router.clone();
            let job_tracker = job_tracker.clone();
            let messages = messages.clone();
            let messages_loading = messages_loading.clone();
            let messages_fetch_gen = messages_fetch_gen.clone();
            let poll_ms = if notifications::is_enabled() {
                1500
            } else {
                3000
            };
            let tick = Rc::new(move || {
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
                let messages = messages.clone();
                let messages_loading = messages_loading.clone();
                let messages_fetch_gen = messages_fetch_gen.clone();
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
                        if let Some(chat_id) = view.selected_chat_id {
                            if let Some(chat) = list.iter().find(|chat| chat.id == chat_id) {
                                if messages_stale_vs_chat(&messages, chat) {
                                    spawn_gated_messages_fetch(
                                        chat_id,
                                        &messages,
                                        &messages_loading,
                                        &messages_fetch_gen,
                                        false,
                                    );
                                }
                            }
                        }
                        publish_chats(&chats, list, view.selected_chat_id);
                    }
                    if let Some(list) = archived_list {
                        publish_archived_chats(&archived_chats, list);
                    }
                    if let Some(list) = story_list {
                        stories.set(list);
                    }
                });
            });
            app_sync::start_poll(poll_ms, tick)
        });
    }

    let open_queue = {
        let navigate = navigate.clone();
        Callback::from(move |_| navigate.emit((AppRoute::Queue { sidebar: false }, true)))
    };

    let open_settings = {
        let navigate = navigate.clone();
        Callback::from(move |_| navigate.emit((AppRoute::Settings { sidebar: false }, true)))
    };

    let open_characters = {
        let navigate = navigate.clone();
        Callback::from(move |_| {
            navigate.emit((
                AppRoute::Characters {
                    character_id: None,
                    chat_id: None,
                    sidebar: false,
                },
                true,
            ))
        })
    };

    let open_characters_for_chat = {
        let navigate = navigate.clone();
        Callback::from(move |(chat_id, character_id): (i64, Option<i64>)| {
            navigate.emit((
                AppRoute::Characters {
                    character_id,
                    chat_id: Some(chat_id),
                    sidebar: false,
                },
                true,
            ))
        })
    };

    let open_scenarios = {
        let navigate = navigate.clone();
        Callback::from(move |_| {
            navigate.emit((
                AppRoute::Scenarios {
                    scenario_id: None,
                    game_id: None,
                    sidebar: false,
                },
                true,
            ))
        })
    };

    let _open_scenarios_for_game = {
        let navigate = navigate.clone();
        Callback::from(move |(game_id, scenario_id): (i64, Option<i64>)| {
            navigate.emit((
                AppRoute::Scenarios {
                    scenario_id,
                    game_id: Some(game_id),
                    sidebar: false,
                },
                true,
            ))
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

    if *loading {
        return html! { <div class="loading-screen muted">{"Loading Dreamwell…"}</div> };
    }

    if *auth_expired {
        let on_sign_in = Callback::from(|_| {
            auth::handle_auth_expiry();
        });
        return html! {
            <div class="session-expired-screen">
                <h1>{"Session expired"}</h1>
                <p class="muted">{"Your sign-in session ended. Sign in again to continue using Dreamwell."}</p>
                <button type="button" class="primary" onclick={on_sign_in}>{"Sign in again"}</button>
            </div>
        };
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
                AppMode::Game => AppRoute::Games {
                    game_id: game_id_from_route(&route),
                    overlay: None,
                    sidebar: false,
                },
                AppMode::Queue => AppRoute::Queue { sidebar: false },
                AppMode::Settings => AppRoute::Settings { sidebar: false },
                AppMode::Characters => AppRoute::Characters {
                    character_id: None,
                    chat_id: None,
                    sidebar: false,
                },
                AppMode::Scenarios => AppRoute::Scenarios {
                    scenario_id: None,
                    game_id: None,
                    sidebar: false,
                },
            };
            navigate.emit((next, true));
        })
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
                            publish_chats(&chats, list, Some(chat.id));
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

    let start_game = {
        let games = games.clone();
        let navigate = navigate.clone();
        let setup_scenario = setup_scenario.clone();
        Callback::from(move |scenario: Scenario| {
            if game_setup_ui::scenario_needs_setup(&scenario) {
                setup_scenario.set(Some(scenario));
                return;
            }
            let title = default_game_title(&scenario.title, scenario.id, &games);
            let payload = game_create_from_scenario(&scenario, title);
            let games = games.clone();
            let navigate = navigate.clone();
            wasm_bindgen_futures::spawn_local(async move {
                match api::create_game(&payload).await {
                    Ok(detail) => {
                        if let Ok(list) = api::list_games().await {
                            games.set(list);
                        }
                        navigate.emit((
                            AppRoute::Games {
                                game_id: Some(detail.game.id),
                                overlay: None,
                                sidebar: false,
                            },
                            true,
                        ));
                    }
                    Err(err) => {
                        if let Some(window) = web_sys::window() {
                            let _ =
                                window.alert_with_message(&format!("Could not start game: {err}"));
                        }
                    }
                }
            });
        })
    };

    let create_game = {
        let games = games.clone();
        let navigate = navigate.clone();
        Callback::from(move |payload: GameCreate| {
            let games = games.clone();
            let navigate = navigate.clone();
            wasm_bindgen_futures::spawn_local(async move {
                match api::create_game(&payload).await {
                    Ok(detail) => {
                        if let Ok(list) = api::list_games().await {
                            games.set(list);
                        }
                        navigate.emit((
                            AppRoute::Games {
                                game_id: Some(detail.game.id),
                                overlay: None,
                                sidebar: false,
                            },
                            true,
                        ));
                    }
                    Err(err) => {
                        if let Some(window) = web_sys::window() {
                            let _ =
                                window.alert_with_message(&format!("Could not start game: {err}"));
                        }
                    }
                }
            });
        })
    };

    let selected = selected_chat_id.and_then(|id| chats.iter().find(|c| c.id == id).cloned());
    let active_header = active_chat_header(&selected, selected_chat_id);
    let selected_story_id = story_id_from_route(&route);
    let selected_game_id = game_id_from_route(&route);
    let scroll_chrome_active =
        mobile_scroll_chrome_active(mode, selected_chat_id, selected_story_id, selected_game_id);
    let app_layout_ref = use_node_ref();
    let mobile_chrome_visible = use_state(|| {
        window_scroll_y() <= 0.0
            && mobile_scroll_chrome_active(
                mode,
                selected_chat_id,
                selected_story_id,
                selected_game_id,
            )
    });

    {
        let mobile_chrome_visible = mobile_chrome_visible.clone();
        let app_layout_ref = app_layout_ref.clone();
        use_effect_with(
            (
                mode,
                selected_chat_id,
                selected_story_id,
                selected_game_id,
                active_header.clone(),
            ),
            move |(mode, chat_id, story_id, game_id, _header)| {
                let mode = *mode;
                let chat_id = *chat_id;
                let story_id = *story_id;
                let game_id = *game_id;
                let scroll_chrome = mobile_scroll_chrome_active(mode, chat_id, story_id, game_id);
                let at_top = window_scroll_y() <= 0.0;
                mobile_chrome_visible.set(at_top && scroll_chrome);
                let mobile_chrome_visible = mobile_chrome_visible.clone();
                let app_layout_ref = app_layout_ref.clone();
                let last_scroll_y = Rc::new(RefCell::new(window_scroll_y()));

                let sync_sidebar_inset = {
                    let app_layout_ref = app_layout_ref.clone();
                    let mobile_chrome_visible = mobile_chrome_visible.clone();
                    Rc::new(move || {
                        if let Some(layout) = app_layout_ref.cast::<HtmlElement>() {
                            update_mobile_sidebar_inset(
                                &layout,
                                mode,
                                scroll_chrome,
                                *mobile_chrome_visible,
                                window_scroll_y(),
                            );
                        }
                    })
                };

                let scroll_callback = Closure::wrap(Box::new({
                    let sync_sidebar_inset = sync_sidebar_inset.clone();
                    let mobile_chrome_visible = mobile_chrome_visible.clone();
                    let last_scroll_y = last_scroll_y.clone();
                    move |_event: web_sys::Event| {
                        if !view_scroll::is_mobile_viewport() {
                            return;
                        }
                        let current = window_scroll_y();
                        if scroll_chrome {
                            let mut last = last_scroll_y.borrow_mut();
                            if current <= 0.0 || current < *last {
                                mobile_chrome_visible.set(true);
                            } else if current > *last {
                                mobile_chrome_visible.set(false);
                            }
                            *last = current;
                        }
                        sync_sidebar_inset();
                    }
                }) as Box<dyn FnMut(_)>);

                let resize_callback = Closure::wrap(Box::new({
                    let sync_sidebar_inset = sync_sidebar_inset.clone();
                    move |_event: web_sys::Event| sync_sidebar_inset()
                }) as Box<dyn FnMut(_)>);

                let window = web_sys::window();
                if let Some(window) = window.as_ref() {
                    let _ = window.add_event_listener_with_callback(
                        "scroll",
                        scroll_callback.as_ref().unchecked_ref(),
                    );
                    let _ = window.add_event_listener_with_callback(
                        "resize",
                        resize_callback.as_ref().unchecked_ref(),
                    );
                }

                let sync = sync_sidebar_inset.clone();
                Timeout::new(0, move || sync()).forget();

                move || {
                    if let Some(window) = window.as_ref() {
                        let _ = window.remove_event_listener_with_callback(
                            "scroll",
                            scroll_callback.as_ref().unchecked_ref(),
                        );
                        let _ = window.remove_event_listener_with_callback(
                            "resize",
                            resize_callback.as_ref().unchecked_ref(),
                        );
                    }
                }
            },
        );
    }

    {
        let app_layout_ref = app_layout_ref.clone();
        use_effect_with(
            (
                mode,
                scroll_chrome_active,
                *mobile_chrome_visible,
                selected_chat_id,
                selected_story_id,
                selected_game_id,
                active_header.as_ref().map(|header| header.title.clone()),
            ),
            move |(mode, scroll_chrome, chrome_visible, ..)| {
                let mode = *mode;
                let scroll_chrome = *scroll_chrome;
                let chrome_visible = *chrome_visible;
                if let Some(layout) = app_layout_ref.cast::<HtmlElement>() {
                    update_mobile_sidebar_inset(
                        &layout,
                        mode,
                        scroll_chrome,
                        chrome_visible,
                        window_scroll_y(),
                    );
                }
                let app_layout_ref = app_layout_ref.clone();
                Timeout::new(0, move || {
                    if let Some(layout) = app_layout_ref.cast::<HtmlElement>() {
                        update_mobile_sidebar_inset(
                            &layout,
                            mode,
                            scroll_chrome,
                            chrome_visible,
                            window_scroll_y(),
                        );
                    }
                })
                .forget();
                || ()
            },
        );
    }

    let sidebar_open = sidebar_open_from_route(&route);
    let overlay = route.overlay();
    let picker_open = overlay == Some(Overlay::NewChat);
    let game_create_open = overlay == Some(Overlay::NewGame);

    let (characters_character_id, characters_chat_id) = characters_route_context(&route);
    let (scenarios_scenario_id, scenarios_game_id) = scenarios_route_context(&route);
    let selected_character_id =
        resolve_characters_selected_id(characters_character_id, characters_chat_id, &chats);

    let bump_stream = {
        let refresh_generation = refresh_generation.clone();
        Callback::from(move |_| refresh_generation.set(*refresh_generation + 1))
    };

    let open_overlay = {
        let navigate = navigate.clone();
        let route = route.clone();
        Callback::from(move |overlay: Overlay| {
            navigate.emit((route.clone().with_overlay(overlay), true));
        })
    };

    html! {
        <div
            ref={app_layout_ref}
            class={classes!(
                "app-layout",
                scroll_chrome_active.then_some("app-layout--chat-scroll-chrome"),
                (*mobile_chrome_visible).then_some("mobile-chrome-visible"),
            )}
        >
            <ModeBar
                mode={mode}
                queue={(*queue).clone()}
                on_toggle_sidebar={toggle_sidebar.clone()}
                show_sidebar_toggle={true}
                sidebar_open={sidebar_open}
                on_open_settings={open_settings.clone()}
                on_open_queue={open_queue.clone()}
                on_open_characters={open_characters.clone()}
                on_open_scenarios={open_scenarios.clone()}
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
            if game_create_open {
                <GameCreateModal
                    characters={(*characters).clone()}
                    on_close={close_overlay.clone()}
                    on_create={create_game.clone()}
                />
            }
            if let Some(scenario) = (*setup_scenario).clone() {
                <game_setup_ui::GameSetupWizard
                    scenario={scenario}
                    games={(*games).clone()}
                    on_close={Callback::from({
                        let setup_scenario = setup_scenario.clone();
                        move |_| setup_scenario.set(None)
                    })}
                    on_create={create_game.clone()}
                />
            }
            if mode == AppMode::Chats && overlay == Some(Overlay::Variables) {
                <ChatPanelOverlay
                    chat_id={selected_chat_id}
                    messages={(*messages).clone()}
                    on_close={close_overlay.clone()}
                    on_messages_changed={load_messages_for_chat.clone()}
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
                archived_stories={(*archived_stories).clone()}
                games={(*games).clone()}
                archived_games={(*archived_games).clone()}
                selected_chat_id={selected_chat_id}
                selected_story_id={story_id_from_route(&route)}
                selected_game_id={game_id_from_route(&route)}
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
                                publish_chats(&chats, list, None);
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
                                    publish_chats(&chats, list, Some(id));
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
                on_open_characters={open_characters.clone()}
                on_archive_story={Callback::from({
                    let stories = stories.clone();
                    let archived_stories = archived_stories.clone();
                    let navigate = navigate.clone();
                    let route = route.clone();
                    move |id| {
                        let stories = stories.clone();
                        let archived_stories = archived_stories.clone();
                        let navigate = navigate.clone();
                        let route = route.clone();
                        wasm_bindgen_futures::spawn_local(async move {
                            let _ = api::archive_story(id).await;
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
                            if let Ok(list) = api::list_archived_stories().await {
                                publish_archived_stories(&archived_stories, list);
                            }
                        });
                    }
                })}
                on_restore_story={Callback::from({
                    let stories = stories.clone();
                    let archived_stories = archived_stories.clone();
                    let navigate = navigate.clone();
                    move |id| {
                        let stories = stories.clone();
                        let archived_stories = archived_stories.clone();
                        let navigate = navigate.clone();
                        wasm_bindgen_futures::spawn_local(async move {
                            if api::restore_story(id).await.is_ok() {
                                if let Ok(list) = api::list_stories().await {
                                    stories.set(list);
                                }
                                if let Ok(list) = api::list_archived_stories().await {
                                    publish_archived_stories(&archived_stories, list);
                                }
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
                        });
                    }
                })}
                on_permanent_delete_story={Callback::from({
                    let archived_stories = archived_stories.clone();
                    move |id| {
                        let archived_stories = archived_stories.clone();
                        wasm_bindgen_futures::spawn_local(async move {
                            if !confirm_permanent_story_delete() {
                                return;
                            }
                            let _ = api::permanently_delete_story(id).await;
                            if let Ok(list) = api::list_archived_stories().await {
                                publish_archived_stories(&archived_stories, list);
                            }
                        });
                    }
                })}
                on_select_game={Callback::from({
                    let navigate = navigate.clone();
                    move |id| {
                        navigate.emit((
                            AppRoute::Games {
                                game_id: Some(id),
                                overlay: None,
                                sidebar: false,
                            },
                            true,
                        ));
                    }
                })}
                on_open_scenarios={open_scenarios.clone()}
                on_new_game={Callback::from({
                    let navigate = navigate.clone();
                    let route = route.clone();
                    move |_| {
                        let next = AppRoute::Games {
                            game_id: game_id_from_route(&route),
                            overlay: Some(Overlay::NewGame),
                            sidebar: false,
                        };
                        navigate.emit((next, true));
                    }
                })}
                on_archive_game={Callback::from({
                    let games = games.clone();
                    let archived_games = archived_games.clone();
                    let navigate = navigate.clone();
                    let route = route.clone();
                    move |id| {
                        let games = games.clone();
                        let archived_games = archived_games.clone();
                        let navigate = navigate.clone();
                        let route = route.clone();
                        wasm_bindgen_futures::spawn_local(async move {
                            let _ = api::archive_game(id).await;
                            if let Ok(list) = api::list_games().await {
                                if game_id_from_route(&route) == Some(id) {
                                    navigate.emit((
                                        AppRoute::Games {
                                            game_id: list.first().map(|g| g.id),
                                            overlay: None,
                                            sidebar: false,
                                        },
                                        false,
                                    ));
                                }
                                games.set(list);
                            }
                            if let Ok(list) = api::list_archived_games().await {
                                publish_archived_games(&archived_games, list);
                            }
                        });
                    }
                })}
                on_restore_game={Callback::from({
                    let games = games.clone();
                    let archived_games = archived_games.clone();
                    let navigate = navigate.clone();
                    move |id| {
                        let games = games.clone();
                        let archived_games = archived_games.clone();
                        let navigate = navigate.clone();
                        wasm_bindgen_futures::spawn_local(async move {
                            if api::restore_game(id).await.is_ok() {
                                if let Ok(list) = api::list_games().await {
                                    games.set(list);
                                }
                                if let Ok(list) = api::list_archived_games().await {
                                    publish_archived_games(&archived_games, list);
                                }
                                navigate.emit((
                                    AppRoute::Games {
                                        game_id: Some(id),
                                        overlay: None,
                                        sidebar: false,
                                    },
                                    true,
                                ));
                            }
                        });
                    }
                })}
                on_permanent_delete_game={Callback::from({
                    let archived_games = archived_games.clone();
                    move |id| {
                        let archived_games = archived_games.clone();
                        wasm_bindgen_futures::spawn_local(async move {
                            if !confirm_permanent_game_delete() {
                                return;
                            }
                            let _ = api::permanently_delete_game(id).await;
                            if let Ok(list) = api::list_archived_games().await {
                                publish_archived_games(&archived_games, list);
                            }
                        });
                    }
                })}
            />
            <main class="main">
                <QueueBar queue={(*queue).clone()} on_open={open_queue.clone()} />
                if mode == AppMode::Queue {
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
                } else if mode == AppMode::Settings {
                    <SettingsPage
                        settings={settings.clone()}
                        on_back={Callback::from({
                            let router = router.clone();
                            move |_| router.back()
                        })}
                    />
                } else if mode == AppMode::Characters {
                    <CharactersPage
                        selected_character_id={selected_character_id}
                        chat_id={characters_chat_id}
                        on_back={Callback::from({
                            let router = router.clone();
                            move |_| router.back()
                        })}
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
                                        publish_chats(&chats, list, Some(chat_id));
                                    }
                                });
                            }
                        })}
                        on_start_chat={start_chat.clone()}
                        on_create_scenario={Callback::from({
                            let navigate = navigate.clone();
                            move |scenario: Scenario| {
                                navigate.emit((
                                    AppRoute::Scenarios {
                                        scenario_id: Some(scenario.id),
                                        game_id: None,
                                        sidebar: false,
                                    },
                                    true,
                                ));
                            }
                        })}
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
                                        publish_chats(&chats, list, Some(chat_id));
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
                            move |_| {
                                let chats = chats.clone();
                                let archived_chats = archived_chats.clone();
                                wasm_bindgen_futures::spawn_local(async move {
                                    if let Ok(list) = api::list_chats().await {
                                        publish_chats(&chats, list, None);
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
                } else if mode == AppMode::Scenarios {
                    <ScenariosPage
                        selected_scenario_id={scenarios_scenario_id}
                        game_id={scenarios_game_id}
                        on_back={Callback::from({
                            let router = router.clone();
                            move |_| router.back()
                        })}
                        on_scenario_change={Callback::from(|_| ())}
                        on_start_game={start_game.clone()}
                        on_scenarios_changed={Callback::from(|_| ())}
                    />
                } else if mode == AppMode::Stories {
                    <StoriesShell
                        route={route.clone()}
                        on_navigate={navigate.clone()}
                        variables_enabled={settings.as_ref().is_some_and(|s| s.variables_enabled)}
                    />
                } else if mode == AppMode::Game {
                    <GameShell
                        route={route.clone()}
                        on_navigate={navigate.clone()}
                        settings={(*settings).clone()}
                        games={(*games).clone()}
                        on_select_game={Callback::from({
                            let navigate = navigate.clone();
                            move |id| {
                                navigate.emit((
                                    AppRoute::Games {
                                        game_id: Some(id),
                                        overlay: None,
                                        sidebar: false,
                                    },
                                    true,
                                ));
                            }
                        })}
                        on_new_game={Callback::from({
                            let navigate = navigate.clone();
                            let route = route.clone();
                            move |_| {
                                navigate.emit((
                                    AppRoute::Games {
                                        game_id: game_id_from_route(&route),
                                        overlay: Some(Overlay::NewGame),
                                        sidebar: false,
                                    },
                                    true,
                                ));
                            }
                        })}
                    />
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
                                            if let Ok(updated) =
                                                api::update_chat(chat_id, &payload).await
                                            {
                                                update_chat_in_list(&chats, updated);
                                            }
                                        });
                                    }
                                })}
                            />
                            <div class="header-actions">
                                <button
                                    class="btn secondary btn-compact header-icon-btn"
                                    title="Character"
                                    onclick={{
                                        let open_characters_for_chat = open_characters_for_chat.clone();
                                        let chat_id = header.id;
                                        let character_id = (header.character_id != 0)
                                            .then_some(header.character_id);
                                        Callback::from(move |_| {
                                            open_characters_for_chat.emit((chat_id, character_id));
                                        })
                                    }}
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
                                        let messages_loading = messages_loading.clone();
                                        let messages_fetch_gen = messages_fetch_gen.clone();
                                        let chats = chats.clone();
                                        let queue = queue.clone();
                                        let bump_stream = bump_stream.clone();
                                        Callback::from(move |_| {
                                            let messages = messages.clone();
                                            let messages_loading = messages_loading.clone();
                                            let messages_fetch_gen = messages_fetch_gen.clone();
                                            let chats = chats.clone();
                                            let queue = queue.clone();
                                            let bump_stream = bump_stream.clone();
                                            wasm_bindgen_futures::spawn_local(async move {
                                                match api::summarize_chat(chat_id).await {
                                                    Ok(_) => {
                                                        bump_stream.emit(());
                                                        spawn_gated_messages_fetch(
                                                            chat_id,
                                                            &messages,
                                                            &messages_loading,
                                                            &messages_fetch_gen,
                                                            false,
                                                        );
                                                        if let Ok(list) = api::list_chats().await {
                                                            publish_chats(&chats, list, Some(chat_id));
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
                            <p class="header-subtitle muted">{"Pick a chat below to continue."}</p>
                        }
                        <div class="header-actions">
                            <button
                                class="btn secondary btn-compact"
                                title="Characters"
                                onclick={open_characters.reform(|_| ())}
                            >
                                {"Characters"}
                            </button>
                        </div>
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
                        generation_live={selected.as_ref().is_none_or(|chat| {
                            message_generation_live(chat, &messages)
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
                            let messages_loading = messages_loading.clone();
                            let messages_fetch_gen = messages_fetch_gen.clone();
                            let chats = chats.clone();
                            let queue = queue.clone();
                            let bump_stream = bump_stream.clone();
                            move |_| {
                                let Some(chat_id) = selected_chat_id else { return };
                                let messages = messages.clone();
                                let messages_loading = messages_loading.clone();
                                let messages_fetch_gen = messages_fetch_gen.clone();
                                let chats = chats.clone();
                                let queue = queue.clone();
                                bump_stream.emit(());
                                spawn_gated_messages_fetch(
                                    chat_id,
                                    &messages,
                                    &messages_loading,
                                    &messages_fetch_gen,
                                    false,
                                );
                                wasm_bindgen_futures::spawn_local(async move {
                                    if let Ok(list) = api::list_chats().await {
                                        publish_chats(&chats, list, Some(chat_id));
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
                            <p>{"No characters yet. Create or import one to get started."}</p>
                            <button class="btn" style="margin-top:0.75rem;" onclick={open_characters.reform(|_| ())}>
                                {"Create character"}
                            </button>
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
                    <ChatList
                        chats={(*chats).clone()}
                        on_select={Callback::from({
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
                    />
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
                        let messages_loading = messages_loading.clone();
                        let messages_fetch_gen = messages_fetch_gen.clone();
                        let chats = chats.clone();
                        let queue = queue.clone();
                        let bump_stream = bump_stream.clone();
                        move |content: String| {
                            let Some(chat_id) = selected_chat_id else { return };
                            let messages = messages.clone();
                            let messages_loading = messages_loading.clone();
                            let messages_fetch_gen = messages_fetch_gen.clone();
                            let chats = chats.clone();
                            let queue = queue.clone();
                            bump_stream.emit(());
                            wasm_bindgen_futures::spawn_local(async move {
                                let _ = api::send_message(chat_id, &content).await;
                                spawn_gated_messages_fetch(
                                    chat_id,
                                    &messages,
                                    &messages_loading,
                                    &messages_fetch_gen,
                                    false,
                                );
                                if let Ok(list) = api::list_chats().await {
                                    publish_chats(&chats, list, Some(chat_id));
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
    show_sidebar_toggle: bool,
    sidebar_open: bool,
    on_toggle_sidebar: Callback<()>,
    on_open_settings: Callback<()>,
    on_open_queue: Callback<()>,
    on_open_characters: Callback<()>,
    on_open_scenarios: Callback<()>,
}

#[function_component(ModeBar)]
fn mode_bar(props: &ModeBarProps) -> Html {
    html! {
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
                <div class="mode-bar-brand">
                    <span class="mode-bar-title">{"Dreamwell"}</span>
                    <BuildInfo />
                </div>
            </div>
            <div class="mode-bar-actions">
                <TopBarQueueButton
                    queue={props.queue.clone()}
                    active={props.mode == AppMode::Queue}
                    on_open={props.on_open_queue.clone()}
                />
                <button
                    type="button"
                    class={classes!("mode-btn", "mode-btn-icon", (props.mode == AppMode::Characters).then_some("active"))}
                    title="Characters"
                    aria-label="Characters"
                    onclick={props.on_open_characters.reform(|_| ())}
                >
                    <span class="mode-btn-icon-glyph">{"🎭"}</span>
                </button>
                <button
                    type="button"
                    class={classes!("mode-btn", "mode-btn-icon", (props.mode == AppMode::Scenarios).then_some("active"))}
                    title="Scenarios"
                    aria-label="Scenarios"
                    onclick={props.on_open_scenarios.reform(|_| ())}
                >
                    <span class="mode-btn-icon-glyph">{"🗺"}</span>
                </button>
                <button
                    type="button"
                    class={classes!("mode-btn", "mode-btn-icon", (props.mode == AppMode::Settings).then_some("active"))}
                    title="Settings"
                    aria-label="Settings"
                    onclick={props.on_open_settings.reform(|_| ())}
                >
                    <span class="mode-btn-icon-glyph">{"⚙"}</span>
                </button>
            </div>
        </div>
    }
}

#[derive(Properties, PartialEq)]
struct CharactersPageProps {
    selected_character_id: Option<i64>,
    chat_id: Option<i64>,
    on_back: Callback<()>,
    on_character_change: Callback<(i64, i64)>,
    on_start_chat: Callback<(i64, String)>,
    on_create_scenario: Callback<Scenario>,
    on_chat_created: Callback<i64>,
    on_chats_changed: Callback<()>,
    on_characters_changed: Callback<()>,
}

#[function_component(CharactersPage)]
fn characters_page(props: &CharactersPageProps) -> Html {
    html! {
        <main class="main characters-page">
            <header class="header">
                <button class="btn secondary" onclick={props.on_back.reform(|_| ())}>{"← Back"}</button>
                <h1 class="header-title">{"Characters"}</h1>
                <p class="header-subtitle muted">{"Create, edit, and import character cards for chats, scenarios, and games."}</p>
            </header>
            <div class="characters-page-body">
                <CharacterPanel
                    selected_character_id={props.selected_character_id}
                    chat_id={props.chat_id}
                    on_character_change={props.on_character_change.clone()}
                    on_start_chat={props.on_start_chat.clone()}
                    on_create_scenario={props.on_create_scenario.clone()}
                    on_chat_created={props.on_chat_created.clone()}
                    on_chats_changed={props.on_chats_changed.clone()}
                    on_characters_changed={props.on_characters_changed.clone()}
                />
            </div>
        </main>
    }
}

#[derive(Properties, PartialEq)]
struct SettingsPageProps {
    settings: UseStateHandle<Option<Settings>>,
    on_back: Callback<()>,
}

#[function_component(SettingsPage)]
fn settings_page(props: &SettingsPageProps) -> Html {
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
        let parent_settings = props.settings.clone();
        use_effect_with((), move |_| {
            let load_ctx = save_ctx.clone();
            let flush_ctx = save_ctx.clone();
            wasm_bindgen_futures::spawn_local(async move {
                if let Ok(settings) = api::get_settings().await {
                    parent_settings.set(Some(settings.clone()));
                    load_ctx.load_from(settings);
                }
            });
            move || {
                flush_ctx.flush_pending();
            }
        });
    }

    {
        let save_ctx = save_ctx.clone();
        use_effect_with((), move |_| {
            let guard = story_save::register_autosave_tab_flush(move || save_ctx.flush_pending());
            move || drop(guard)
        });
    }

    html! {
        <main class="main settings-page">
            <header class="header">
                <button class="btn secondary" onclick={props.on_back.reform(|_| ())}>{"← Back"}</button>
                <h1 class="header-title">{"Settings"}</h1>
                <p class="header-subtitle muted">{"Inference server, model, and generation defaults."}</p>
            </header>
            <div class="settings-page-body">
                <SettingsPanel save_ctx={save_ctx} />
            </div>
        </main>
    }
}

#[derive(Properties, PartialEq)]
struct ChatPanelOverlayProps {
    chat_id: Option<i64>,
    messages: Vec<Message>,
    on_close: Callback<()>,
    on_messages_changed: Callback<i64>,
}

#[function_component(ChatPanelOverlay)]
fn chat_panel_overlay(props: &ChatPanelOverlayProps) -> Html {
    html! {
        <div
            id="variables-panel"
            class="settings-popover panel-overlay"
        >
            <div class="settings-header">
                <h2>{"Variables"}</h2>
                <button class="btn secondary btn-compact" onclick={props.on_close.reform(|_| ())}>{"Close"}</button>
            </div>
            <div class="panel-overlay-body">
                <VariablesPanel
                    chat_id={props.chat_id}
                    messages={props.messages.clone()}
                    on_messages_changed={props.on_messages_changed.clone()}
                />
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

fn sort_archived_stories(mut stories: Vec<Story>) -> Vec<Story> {
    stories.sort_by(|a, b| {
        b.archived_at
            .cmp(&a.archived_at)
            .then_with(|| a.title.cmp(&b.title))
    });
    stories
}

fn sort_archived_games(mut games: Vec<Game>) -> Vec<Game> {
    games.sort_by(|a, b| {
        b.archived_at
            .cmp(&a.archived_at)
            .then_with(|| a.title.cmp(&b.title))
    });
    games
}

fn confirm_permanent_chat_delete() -> bool {
    web_sys::window()
        .and_then(|w| {
            w.confirm_with_message("Permanently delete this archived chat? This cannot be undone.")
                .ok()
        })
        .unwrap_or(false)
}

fn confirm_permanent_story_delete() -> bool {
    web_sys::window()
        .and_then(|w| {
            w.confirm_with_message("Permanently delete this archived story? This cannot be undone.")
                .ok()
        })
        .unwrap_or(false)
}

fn confirm_permanent_game_delete() -> bool {
    web_sys::window()
        .and_then(|w| {
            w.confirm_with_message("Permanently delete this archived game? This cannot be undone.")
                .ok()
        })
        .unwrap_or(false)
}

fn summarize_placeholder_id(messages: &[Message]) -> Option<i64> {
    messages
        .iter()
        .find(|message| is_chat_summarize_pending(message))
        .map(|message| message.id)
}

fn summarize_in_progress(chat: &Chat, messages: &[Message]) -> bool {
    chat_summarize_in_progress(chat, messages)
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
    #[prop_or(true)]
    generation_live: bool,
    on_changed: Callback<()>,
}

#[function_component(MessageBubble)]
fn message_bubble(props: &MessageBubbleProps) -> Html {
    let mode = use_state(|| MessageBubbleMode::View);
    let edit_text = use_state(String::new);
    let acting = use_state(|| false);

    let role = match props.message.role {
        MessageRole::User => "user",
        MessageRole::Assistant => "assistant",
        MessageRole::System => "system",
    };
    let queued =
        props.generation_live && matches!(props.message.job_status, Some(JobStatus::Queued));
    let streaming =
        props.generation_live && matches!(props.message.job_status, Some(JobStatus::Running));
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
    let align_menu_end = props.message.role == MessageRole::User;

    let start_edit = {
        let mode = mode.clone();
        let edit_text = edit_text.clone();
        let content = props.message.content.clone();
        Callback::from(move |_| {
            edit_text.set(content.clone());
            mode.set(MessageBubbleMode::Edit);
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
        let acting = acting.clone();
        let on_changed = props.on_changed.clone();
        let chat_id = props.chat_id;
        let message_id = props.message.id;
        Callback::from(move |_| {
            if *acting {
                return;
            }
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
        <div
            id={format!("message-{}", props.message.id)}
            class={classes!(
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
                    <MessageOptionsMenu align_end={align_menu_end} disabled={*acting}>
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
                    </MessageOptionsMenu>
                }
            </div>
            if show_thought_block {
                <ThoughtBlock
                    thought_content={props.message.thought_content.clone()}
                    thought_duration_ms={props.message.thought_duration_ms}
                    thought_in_progress={props.message.thought_in_progress}
                />
            }
            if props.message.role == MessageRole::Assistant && !props.message.state_changes.is_empty() {
                <PhaseSection label={"State changes".to_string()} default_expanded={false}>
                    <StateChangesList changes={props.message.state_changes.clone()} />
                </PhaseSection>
            }
            if props.message.role == MessageRole::Assistant && !props.message.reply_beats.is_empty() {
                <PhaseSection
                    label={"Plan".to_string()}
                    default_expanded={active || !show_body}
                >
                    <PlanBeatsList beats={props.message.reply_beats.clone()} inline={true} />
                </PhaseSection>
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
        </div>
    }
}

const SUMMARIZE_PLACEHOLDER: &str = CHAT_SUMMARIZE_PLACEHOLDER;

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
    let mode = use_state(|| SummaryMarkerMode::View);
    let edit_text = use_state(String::new);
    let acting = use_state(|| false);
    let active = matches!(
        props.message.job_status,
        Some(JobStatus::Queued) | Some(JobStatus::Running)
    );
    let pending = active || props.message.content == SUMMARIZE_PLACEHOLDER;
    let can_manage = !pending && !props.summarize_busy && !*acting;
    let has_summary = !props.chat_summary.trim().is_empty();

    let start_edit = {
        let mode = mode.clone();
        let edit_text = edit_text.clone();
        let chat_summary = props.chat_summary.clone();
        Callback::from(move |_| {
            edit_text.set(chat_summary.clone());
            mode.set(SummaryMarkerMode::Edit);
        })
    };

    let cancel_edit = {
        let mode = mode.clone();
        Callback::from(move |_| mode.set(SummaryMarkerMode::View))
    };

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

    let extra_actions = if *mode == SummaryMarkerMode::View && can_manage && has_summary {
        html! {
            <>
                <button type="button" class="summary-toggle" onclick={start_edit}>
                    {"Edit summary"}
                </button>
                <button type="button" class="summary-toggle" onclick={regenerate_summary}>
                    {"Regenerate"}
                </button>
                <button type="button" class="summary-toggle" onclick={delete_summary}>
                    {"Delete summary"}
                </button>
            </>
        }
    } else {
        html! {}
    };

    html! {
        <div class={classes!(
            "message-summary-marker",
            active.then_some("message-summary-marker--active")
        )}>
            <SummaryBreak kind={SummaryKind::ChatHistory} pending={pending} />
            <div class="message-summary-body">
                if pending {
                    <SummaryView
                        text={String::new()}
                        pending={true}
                        kind={SummaryKind::ChatHistory}
                        default_expanded={true}
                    />
                } else if *mode == SummaryMarkerMode::Edit {
                    <div class="summary-editor">
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
                        <div class="summary-editor-actions">
                            <button class="btn secondary" onclick={cancel_edit} disabled={*acting}>{"Cancel"}</button>
                            <button class="btn" onclick={save_edit} disabled={*acting || edit_text.trim().is_empty()}>
                                { if *acting { "Saving…" } else { "Save summary" } }
                            </button>
                        </div>
                    </div>
                } else {
                    <SummaryView
                        text={props.chat_summary.clone()}
                        pending={false}
                        kind={SummaryKind::ChatHistory}
                        extra_actions={extra_actions}
                    />
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
    #[prop_or(true)]
    generation_live: bool,
    on_messages_change: Callback<()>,
}

#[function_component(MessageList)]
fn message_list(props: &MessageListProps) -> Html {
    let messages_ref = use_node_ref();

    use_effect_with((props.chat_id, props.loading), {
        let messages_ref = messages_ref.clone();
        let len = props.messages.len();
        move |(chat_id, loading)| {
            if chat_id.is_some() && !*loading && len > 0 {
                let messages_ref = messages_ref.clone();
                Timeout::new(0, move || {
                    let el = messages_ref.cast::<HtmlElement>();
                    scroll_content_view_to_bottom(el.as_ref());
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
                setup_vars: dreamwell_types::empty_setup_vars(),
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
                    let streaming = props.generation_live
                        && matches!(m.job_status, Some(JobStatus::Running));
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
                                generation_live={props.generation_live}
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
    on_create_scenario: Callback<Scenario>,
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
                                let character = c.clone();
                                let on_create_scenario = props.on_create_scenario.clone();
                                Callback::from(move |e: MouseEvent| {
                                    e.stop_propagation();
                                    let character = character.clone();
                                    let on_create_scenario = on_create_scenario.clone();
                                    wasm_bindgen_futures::spawn_local(async move {
                                        let payload =
                                            scenario_create_from_character_record(&character);
                                        match api::create_scenario(&payload).await {
                                            Ok(scenario) => on_create_scenario.emit(scenario),
                                            Err(err) => {
                                                if let Some(window) = web_sys::window() {
                                                    let _ = window.alert_with_message(&format!(
                                                        "Could not create scenario: {err}"
                                                    ));
                                                }
                                            }
                                        }
                                    });
                                })
                            }}>{"Scenario"}</button>
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
    on_messages_changed: Callback<i64>,
}

#[function_component(VariablesPanel)]
fn variables_panel(props: &VariablesPanelProps) -> Html {
    let variables = use_state(Vec::<ChatVariable>::new);

    let chat_state = use_state(Vec::<ChatStateEntry>::new);

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
        let chat_state = chat_state.clone();
        use_effect_with(refresh_signal, move |signal| {
            if let Some(chat_id) = signal.chat_id {
                let variables = variables.clone();
                let chat_state = chat_state.clone();
                wasm_bindgen_futures::spawn_local(async move {
                    if let Ok(list) = api::get_variables(chat_id).await {
                        variables.set(list);
                    }
                    if let Ok(detail) = api::get_chat_detail(chat_id).await {
                        chat_state.set(detail.state);
                    }
                });
            } else {
                variables.set(vec![]);
                chat_state.set(vec![]);
            }
            || ()
        });
    }

    let Some(chat_id) = props.chat_id else {
        return html! { <p class="muted">{"Select a chat to view variables."}</p> };
    };

    let scope_options = chat_scope_options(&props.messages);
    let rows: Vec<VariableRowModel> = variables
        .iter()
        .map(|variable| chat_variable_row(variable, &props.messages, true))
        .collect();

    let on_messages_changed = props.on_messages_changed.clone();
    let (on_save, on_delete) = make_chat_variable_handlers(
        chat_id,
        variables,
        Some(Callback::from(move |_| on_messages_changed.emit(chat_id))),
    );

    html! {
        <>
            <StateEntriesPanel entries={chat_state.iter().map(StateEntryRow::from).collect::<Vec<_>>()} />
            <VariableList
            rows={rows}
            scope_options={scope_options}
            new_scope_value={MANUAL_MESSAGE_SOURCE.to_string()}
            description={"Chat variables are injected into the prompt. The model can update them with var tags. Scope controls when a manual value applies.".to_string()}
            on_save={on_save}
            on_delete={on_delete}
        />
        </>
    }
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
    fn flush_pending(&self) {
        if matches!(*self.phase, SettingsSavePhase::Debouncing) && self.is_dirty() {
            self.cancel_debounce();
            self.run_save();
        }
    }

    fn load_from(&self, settings: Settings) {
        *self.draft_ref.borrow_mut() = Some(settings.clone());
        self.draft.set(Some(settings.clone()));
        self.last_saved.set(Some(settings));
        self.phase.set(SettingsSavePhase::Synced);
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
        *self.save_timeout.borrow_mut() = Some(Timeout::new(AUTOSAVE_DEBOUNCE_MS, move || {
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
        active_connection_id: current.active_connection_id,
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

fn probe_model_capabilities_for_settings(
    save_ctx: &SettingsSaveContext,
    detected_caps: &UseStateHandle<Option<ModelCapabilities>>,
    caps_busy: &UseStateHandle<bool>,
    model: String,
) {
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
}

fn settings_autosave_field_props(phase: &SettingsSavePhase) -> (AutoSavePhase, Option<String>) {
    match phase {
        SettingsSavePhase::Synced => (AutoSavePhase::Synced, None),
        SettingsSavePhase::Debouncing => (AutoSavePhase::Debouncing, None),
        SettingsSavePhase::Saving => (AutoSavePhase::Saving, None),
        SettingsSavePhase::Failed(message) => (AutoSavePhase::Failed, Some(message.clone())),
    }
}

#[derive(Properties, PartialEq)]
struct SettingsPanelProps {
    save_ctx: SettingsSaveContext,
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
    let (autosave_phase, autosave_error) = settings_autosave_field_props(&phase);

    let active_connection = s
        .active_connection_id
        .and_then(|id| s.connections.iter().find(|c| c.id == id).cloned());
    let connection_name = active_connection
        .as_ref()
        .map(|c| c.name.clone())
        .unwrap_or_default();
    let api_key_set = active_connection.as_ref().is_some_and(|c| c.api_key_set);
    let json_format_strategy = active_connection
        .as_ref()
        .map(|c| c.json_format_strategy)
        .unwrap_or(JsonFormatStrategy::Auto);

    html! {
        <div>
            <div class="settings-group">
                <strong>{"Inference connection"}</strong>
                <p class="muted" style="margin:0.35rem 0 0.5rem;">
                    {"Save multiple API endpoints (Ollama, Featherlight.ai, etc.) and switch between them. API keys are stored on the server and sent as Bearer tokens."}
                </p>
                <div class="settings-connection-row">
                    <label class="field">
                        <span class="muted">{"Active connection"}</span>
                        <select onchange={{
                            let save_ctx = save_ctx.clone();
                            let models = models.clone();
                            let model_error = model_error.clone();
                            Callback::from(move |e: Event| {
                                let input: HtmlInputElement = e.target_unchecked_into();
                                let id: i64 = input.value().parse().unwrap_or(0);
                                if id <= 0 {
                                    return;
                                }
                                let save_ctx = save_ctx.clone();
                                let models = models.clone();
                                let model_error = model_error.clone();
                                save_ctx.update_field(|current| {
                                    current.active_connection_id = Some(id);
                                    if let Some(conn) = current.connections.iter().find(|c| c.id == id) {
                                        current.inference_url = conn.inference_url.clone();
                                    }
                                });
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
                        }}>
                            { for s.connections.iter().map(|c| html! {
                                <option value={c.id.to_string()} selected={Some(c.id) == s.active_connection_id}>
                                    { c.name.clone() }
                                </option>
                            }) }
                        </select>
                    </label>
                    <button class="btn secondary" type="button" onclick={{
                        let save_ctx = save_ctx.clone();
                        Callback::from(move |_| {
                            let save_ctx = save_ctx.clone();
                            wasm_bindgen_futures::spawn_local(async move {
                                let next_index = (*save_ctx.draft)
                                    .as_ref()
                                    .map(|s| s.connections.len())
                                    .unwrap_or(0)
                                    + 1;
                                let payload = InferenceConnectionCreate {
                                    name: format!("Connection {next_index}"),
                                    inference_url: "https://api.featherlight.ai/v1".into(),
                                    api_key: None,
                                };
                                match api::create_inference_connection(&payload).await {
                                    Ok(conn) => {
                                        save_ctx.update_field(|current| {
                                            current.connections.push(conn.clone());
                                            current.active_connection_id = Some(conn.id);
                                            current.inference_url = conn.inference_url;
                                        });
                                    }
                                    Err(err) => save_ctx.phase.set(SettingsSavePhase::Failed(err)),
                                }
                            });
                        })
                    }}>{"Add"}</button>
                    <button class="btn secondary" type="button" disabled={s.connections.len() <= 1} onclick={{
                        let save_ctx = save_ctx.clone();
                        let active_id = s.active_connection_id;
                        Callback::from(move |_| {
                            let Some(id) = active_id else { return };
                            let save_ctx = save_ctx.clone();
                            wasm_bindgen_futures::spawn_local(async move {
                                match api::delete_inference_connection(id).await {
                                    Ok(()) => {
                                        if let Ok(settings) = api::get_settings().await {
                                            save_ctx.mark_saved(settings);
                                        }
                                    }
                                    Err(err) => save_ctx.phase.set(SettingsSavePhase::Failed(err)),
                                }
                            });
                        })
                    }}>{"Delete"}</button>
                </div>
                if let Some(active_id) = s.active_connection_id {
                    <label class="field">
                        <span class="muted">{"Connection name"}</span>
                        <input value={connection_name} onchange={{
                            let save_ctx = save_ctx.clone();
                            Callback::from(move |e: Event| {
                                let input: HtmlInputElement = e.target_unchecked_into();
                                let name = input.value();
                                let save_ctx = save_ctx.clone();
                                save_ctx.update_field(|current| {
                                    if let Some(conn) = current.connections.iter_mut().find(|c| c.id == active_id) {
                                        conn.name = name.clone();
                                    }
                                });
                                wasm_bindgen_futures::spawn_local(async move {
                                    let payload = InferenceConnectionUpdate {
                                        name: Some(name),
                                        ..Default::default()
                                    };
                                    if let Err(err) = api::update_inference_connection(active_id, &payload).await {
                                        save_ctx.phase.set(SettingsSavePhase::Failed(err));
                                    }
                                });
                            })
                        }} />
                    </label>
                }
            </div>
            <label class="field">
                <span class="muted">{"API base URL"}</span>
                <AutoSaveField phase={autosave_phase} error={autosave_error.clone()}>
                    <input value={s.inference_url.clone()} placeholder="https://api.featherlight.ai/v1" oninput={{
                        let save_ctx = save_ctx.clone();
                        Callback::from(move |e: InputEvent| {
                            let input: HtmlInputElement = e.target_unchecked_into();
                            let url = input.value();
                            save_ctx.update_field(|current| {
                                current.inference_url = url.clone();
                                if let Some(active_id) = current.active_connection_id {
                                    if let Some(conn) = current.connections.iter_mut().find(|c| c.id == active_id) {
                                        conn.inference_url = url;
                                    }
                                }
                            });
                        })
                    }} />
                </AutoSaveField>
            </label>
            if let Some(active_id) = s.active_connection_id {
                <label class="field">
                    <span class="muted">{"API key"}</span>
                    <input type="password" autocomplete="off" placeholder={if api_key_set { "Leave blank to keep current key" } else { "sk-..." }} onchange={{
                        let save_ctx = save_ctx.clone();
                        Callback::from(move |e: Event| {
                            let input: HtmlInputElement = e.target_unchecked_into();
                            let key = input.value();
                            if key.is_empty() {
                                return;
                            }
                            let save_ctx = save_ctx.clone();
                            wasm_bindgen_futures::spawn_local(async move {
                                let payload = InferenceConnectionUpdate {
                                    api_key: Some(key),
                                    ..Default::default()
                                };
                                match api::update_inference_connection(active_id, &payload).await {
                                    Ok(conn) => {
                                        save_ctx.update_field(|current| {
                                            if let Some(existing) = current.connections.iter_mut().find(|c| c.id == active_id) {
                                                *existing = conn;
                                            }
                                        });
                                    }
                                    Err(err) => save_ctx.phase.set(SettingsSavePhase::Failed(err)),
                                }
                            });
                        })
                    }} />
                </label>
                <label class="field">
                    <span class="muted">{"Structured JSON format"}</span>
                    <select onchange={{
                        let save_ctx = save_ctx.clone();
                        Callback::from(move |e: Event| {
                            let input: HtmlInputElement = e.target_unchecked_into();
                            let strategy = match input.value().as_str() {
                                "response_json_schema" => JsonFormatStrategy::ResponseJsonSchema,
                                "guided_json" => JsonFormatStrategy::GuidedJson,
                                "json_object" => JsonFormatStrategy::JsonObject,
                                _ => JsonFormatStrategy::Auto,
                            };
                            let save_ctx = save_ctx.clone();
                            save_ctx.update_field(|current| {
                                if let Some(conn) = current.connections.iter_mut().find(|c| c.id == active_id) {
                                    conn.json_format_strategy = strategy;
                                }
                            });
                            wasm_bindgen_futures::spawn_local(async move {
                                let payload = InferenceConnectionUpdate {
                                    json_format_strategy: Some(strategy),
                                    ..Default::default()
                                };
                                if let Err(err) = api::update_inference_connection(active_id, &payload).await {
                                    save_ctx.phase.set(SettingsSavePhase::Failed(err));
                                }
                            });
                        })
                    }}>
                        <option value="auto" selected={json_format_strategy == JsonFormatStrategy::Auto}>
                            { JsonFormatStrategy::Auto.label() }
                        </option>
                        <option value="response_json_schema" selected={json_format_strategy == JsonFormatStrategy::ResponseJsonSchema}>
                            { JsonFormatStrategy::ResponseJsonSchema.label() }
                        </option>
                        <option value="guided_json" selected={json_format_strategy == JsonFormatStrategy::GuidedJson}>
                            { JsonFormatStrategy::GuidedJson.label() }
                        </option>
                        <option value="json_object" selected={json_format_strategy == JsonFormatStrategy::JsonObject}>
                            { JsonFormatStrategy::JsonObject.label() }
                        </option>
                    </select>
                    <p class="muted" style="margin:0.35rem 0 0;">
                        {"Game and story structured phases need schema-constrained JSON. Auto tries formats once, then caches the winner for this connection."}
                    </p>
                </label>
            }
            <div class="settings-model-row">
                <label class="field">
                    <span class="muted">{"Model"}</span>
                    <AutoSaveField phase={autosave_phase} error={autosave_error.clone()}>
                        <input
                            type="text"
                            list="dreamwell-model-list"
                            value={s.model.clone()}
                            placeholder="Type model id (e.g. from your provider)"
                            oninput={{
                                let save_ctx = save_ctx.clone();
                                Callback::from(move |e: InputEvent| {
                                    let input: HtmlInputElement = e.target_unchecked_into();
                                    save_ctx.update_field(|current| current.model = input.value());
                                })
                            }}
                            onchange={{
                                let save_ctx = save_ctx.clone();
                                let detected_caps = detected_caps.clone();
                                let caps_busy = caps_busy.clone();
                                Callback::from(move |e: Event| {
                                    let input: HtmlInputElement = e.target_unchecked_into();
                                    probe_model_capabilities_for_settings(
                                        &save_ctx,
                                        &detected_caps,
                                        &caps_busy,
                                        input.value(),
                                    );
                                })
                            }}
                        />
                        <datalist id="dreamwell-model-list">
                            { for models.iter().map(|m| html! {
                                <option value={m.id.clone()} label={m.name.clone().unwrap_or(m.id.clone())} />
                            }) }
                        </datalist>
                    </AutoSaveField>
                    <p class="muted" style="margin:0.35rem 0 0;">
                        {"Refresh loads models from the backend when supported; otherwise type the model id manually."}
                    </p>
                </label>
                <button class="btn secondary" type="button" onclick={{
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
                        <AutoSaveField phase={autosave_phase} error={autosave_error.clone()}>
                            <input type="number" value={s.context_tokens.to_string()} oninput={num_input(save_ctx.clone(), "context_tokens")} />
                        </AutoSaveField>
                    </label>
                    <label class="field">
                        <span class="muted">{"Response length (tokens)"}</span>
                        <AutoSaveField phase={autosave_phase} error={autosave_error.clone()}>
                            <input type="number" value={s.max_tokens.to_string()} oninput={num_input(save_ctx.clone(), "max_tokens")} />
                        </AutoSaveField>
                    </label>
                    <label class="field">
                        <span class="muted">{"Max history messages"}</span>
                        <AutoSaveField phase={autosave_phase} error={autosave_error.clone()}>
                            <input type="number" value={s.max_context_messages.to_string()} oninput={num_input(save_ctx.clone(), "max_context_messages")} />
                        </AutoSaveField>
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
                <label class="field"><span class="muted">{"Temperature"}</span>
                    <AutoSaveField phase={autosave_phase} error={autosave_error.clone()}>
                        <input type="number" step="0.05" value={s.temperature.to_string()} oninput={num_input(save_ctx.clone(), "temperature")} />
                    </AutoSaveField>
                </label>
                <label class="field"><span class="muted">{"Top P"}</span>
                    <AutoSaveField phase={autosave_phase} error={autosave_error.clone()}>
                        <input type="number" step="0.05" value={s.top_p.to_string()} oninput={num_input(save_ctx.clone(), "top_p")} />
                    </AutoSaveField>
                </label>
                <label class="field"><span class="muted">{"Max concurrent jobs"}</span>
                    <AutoSaveField phase={autosave_phase} error={autosave_error.clone()}>
                        <input type="number" value={s.max_concurrent_jobs.to_string()} oninput={num_input(save_ctx.clone(), "max_concurrent_jobs")} />
                    </AutoSaveField>
                </label>
            </div>
            <label class="field"><span class="muted">{"User name ({{user}})"}</span>
                <AutoSaveField phase={autosave_phase} error={autosave_error.clone()}>
                    <input value={s.user_name.clone()} oninput={text_input(save_ctx.clone(), "user_name")} />
                </AutoSaveField>
            </label>
            <label class="field"><span class="muted">{"Persona description ({{persona}})"}</span>
                <AutoSaveField phase={autosave_phase} error={autosave_error.clone()}>
                    <textarea value={s.persona_description.clone()} rows="3" oninput={text_input(save_ctx.clone(), "persona_description")} />
                </AutoSaveField>
            </label>
            <label class="field"><span class="muted">{"Main prompt (prefix)"}</span>
                <AutoSaveField phase={autosave_phase} error={autosave_error.clone()}>
                    <textarea value={s.system_prompt_prefix.clone()} rows="3" oninput={text_input(save_ctx.clone(), "system_prompt_prefix")} />
                </AutoSaveField>
            </label>
            <label class="field"><span class="muted">{"Post-history instructions (suffix)"}</span>
                <AutoSaveField phase={autosave_phase} error={autosave_error.clone()}>
                    <textarea value={s.system_prompt_suffix.clone()} rows="3" oninput={text_input(save_ctx.clone(), "system_prompt_suffix")} />
                </AutoSaveField>
            </label>
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
                <label class="field"><span class="muted">{"Minimum messages before summarize"}</span>
                    <AutoSaveField phase={autosave_phase} error={autosave_error.clone()}>
                        <input type="number" value={s.summarize_after_messages.to_string()} oninput={num_input(save_ctx.clone(), "summarize_after_messages")} />
                    </AutoSaveField>
                </label>
                <label class="field"><span class="muted">{"Minimum recent messages to keep"}</span>
                    <AutoSaveField phase={autosave_phase} error={autosave_error.clone()}>
                        <input type="number" value={s.summarize_keep_recent.to_string()} oninput={num_input(save_ctx.clone(), "summarize_keep_recent")} />
                    </AutoSaveField>
                </label>
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
            <InstallSettings />
            <NotificationSettings />
        </div>
    }
}

#[function_component(BuildInfo)]
fn build_info() -> Html {
    let sha = use_state(|| None::<String>);

    {
        let sha = sha.clone();
        use_effect_with((), move |_| {
            wasm_bindgen_futures::spawn_local(async move {
                if let Ok(health) = api::get_health().await {
                    sha.set(health.git_sha);
                }
            });
            || ()
        });
    }

    match (*sha).clone() {
        Some(sha) => html! { <span class="mode-bar-version muted">{ sha }</span> },
        None => html! {},
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
mod sidebar_tests {
    use super::*;

    #[test]
    fn sidebar_open_from_route_matches_all_modes() {
        assert!(!sidebar_open_from_route(&AppRoute::Chats {
            chat_id: None,
            overlay: None,
            sidebar: false,
        }));
        assert!(sidebar_open_from_route(&AppRoute::Chats {
            chat_id: None,
            overlay: None,
            sidebar: true,
        }));
        assert!(!sidebar_open_from_route(&AppRoute::Stories {
            story_id: None,
            nav: StoryNav::None,
            overlay: None,
            sidebar: false,
        }));
        assert!(sidebar_open_from_route(&AppRoute::Stories {
            story_id: None,
            nav: StoryNav::None,
            overlay: None,
            sidebar: true,
        }));
        assert!(!sidebar_open_from_route(&AppRoute::Games {
            game_id: None,
            overlay: None,
            sidebar: false,
        }));
        assert!(sidebar_open_from_route(&AppRoute::Games {
            game_id: None,
            overlay: None,
            sidebar: true,
        }));
    }
}

#[cfg(test)]
mod chat_list_tests {
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

    #[test]
    fn merge_chat_into_list_updates_existing_chat() {
        let chats = vec![
            sample_chat(1, "Alpha", "2026-01-01T00:00:00Z"),
            sample_chat(2, "Beta", "2026-01-02T00:00:00Z"),
        ];
        let updated = sample_chat(1, "Renamed", "2026-01-03T00:00:00Z");
        let next = merge_chat_into_list(chats, updated);
        assert_eq!(next.len(), 2);
        assert_eq!(next[0].title, "Renamed");
        assert_eq!(next[1].id, 2);
    }

    #[test]
    fn merge_chat_into_list_inserts_missing_chat() {
        let chats = vec![sample_chat(2, "Beta", "2026-01-02T00:00:00Z")];
        let updated = sample_chat(1, "Renamed", "2026-01-03T00:00:00Z");
        let next = merge_chat_into_list(chats, updated);
        assert_eq!(next.len(), 2);
        assert!(next
            .iter()
            .any(|chat| chat.id == 1 && chat.title == "Renamed"));
    }

    #[test]
    fn finalize_chat_list_keeps_selected_chat_when_server_list_omits_it() {
        let current = vec![
            sample_chat(1, "Renamed", "2026-01-03T00:00:00Z"),
            sample_chat(2, "Beta", "2026-01-02T00:00:00Z"),
        ];
        let next = finalize_chat_list(
            &current,
            vec![sample_chat(2, "Beta", "2026-01-02T00:00:00Z")],
            Some(1),
        );
        assert_eq!(next.len(), 2);
        assert!(next
            .iter()
            .any(|chat| chat.id == 1 && chat.title == "Renamed"));
    }
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
