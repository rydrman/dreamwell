mod api;
mod markdown;
mod queue_ui;
mod stories_ui;

use std::cell::RefCell;
use std::rc::Rc;

use dreamwell_types::*;
use gloo_timers::callback::{Interval, Timeout};
use queue_ui::{AppMode, QueueBar, QueuePage};
use stories_ui::StoriesShell;
use web_sys::HtmlInputElement;
use yew::prelude::*;

#[derive(Clone, Copy, PartialEq)]
pub(crate) enum MobilePane {
    Main,
    Sidebar,
    Panel,
}

#[function_component(App)]
fn app() -> Html {
    let mode = use_state(|| AppMode::Chats);
    let return_mode = use_state(|| AppMode::Chats);
    let pending_story_id = use_state(|| None::<i64>);
    let chats = use_state(Vec::<Chat>::new);
    let characters = use_state(Vec::<Character>::new);
    let selected_chat_id = use_state(|| None::<i64>);
    let messages = use_state(Vec::<Message>::new);
    let messages_loading = use_state(|| false);
    let settings = use_state(|| None::<Settings>);
    let queue = use_state(|| None::<QueueStatus>);
    let loading = use_state(|| true);
    let picker_open = use_state(|| false);
    let mobile_pane = use_state(|| MobilePane::Main);

    {
        let chats = chats.clone();
        let characters = characters.clone();
        let selected_chat_id = selected_chat_id.clone();
        let settings = settings.clone();
        let loading = loading.clone();
        use_effect_with((), move |_| {
            wasm_bindgen_futures::spawn_local(async move {
                if let Ok(list) = api::list_chats().await {
                    if let Some(first) = list.first() {
                        selected_chat_id.set(Some(first.id));
                    }
                    chats.set(list);
                }
                if let Ok(list) = api::list_characters().await {
                    characters.set(list);
                }
                if let Ok(s) = api::get_settings().await {
                    settings.set(Some(s));
                }
                loading.set(false);
            });
            || ()
        });
    }

    {
        let messages = messages.clone();
        let messages_loading = messages_loading.clone();
        let chats = chats.clone();
        let selected_chat_id = *selected_chat_id;
        use_effect_with(selected_chat_id, move |chat_id| {
            let mut stream_holder = None::<api::ChatStream>;
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
                stream_holder = api::ChatStream::new(chat_id, move |payload| {
                    messages.set(payload.messages.clone());
                    messages_loading.set(false);
                    let current = (*chats).clone();
                    chats.set(
                        current
                            .into_iter()
                            .map(|c| {
                                if c.id == payload.chat.id {
                                    payload.chat.clone()
                                } else {
                                    c
                                }
                            })
                            .collect(),
                    );
                });
            } else {
                messages.set(vec![]);
                messages_loading.set(false);
            }
            move || {
                drop(stream_holder);
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
        let queue = queue.clone();
        let chats = chats.clone();
        use_effect_with((), move |_| {
            let queue = queue.clone();
            let chats = chats.clone();
            let handle = Interval::new(3000, move || {
                let queue = queue.clone();
                let chats = chats.clone();
                wasm_bindgen_futures::spawn_local(async move {
                    if let Ok(status) = api::get_queue().await {
                        queue.set(Some(status));
                    }
                    if let Ok(list) = api::list_chats().await {
                        chats.set(list);
                    }
                });
            });
            move || drop(handle)
        });
    }

    let open_queue = {
        let mode = mode.clone();
        let return_mode = return_mode.clone();
        Callback::from(move |_| {
            return_mode.set(*mode);
            mode.set(AppMode::Queue);
        })
    };

    if *loading && *mode == AppMode::Chats {
        return html! { <div class="loading-screen muted">{"Loading Dreamwell…"}</div> };
    }

    if *mode == AppMode::Queue {
        return html! {
            <>
                <ModeBar mode={AppMode::Chats} settings={settings.clone()} on_mode={Callback::from({
                    let mode = mode.clone();
                    move |m| mode.set(m)
                })} />
                <QueuePage
                    queue={(*queue).clone()}
                    on_back={Callback::from({
                        let mode = mode.clone();
                        let return_mode = return_mode.clone();
                        move |_| mode.set(*return_mode)
                    })}
                    on_open_chat={Callback::from({
                        let mode = mode.clone();
                        let selected_chat_id = selected_chat_id.clone();
                        let load_messages_for_chat = load_messages_for_chat.clone();
                        move |chat_id| {
                            mode.set(AppMode::Chats);
                            selected_chat_id.set(Some(chat_id));
                            load_messages_for_chat.emit(chat_id);
                        }
                    })}
                    on_open_story={Callback::from({
                        let mode = mode.clone();
                        let pending_story_id = pending_story_id.clone();
                        move |story_id| {
                            pending_story_id.set(Some(story_id));
                            mode.set(AppMode::Stories);
                        }
                    })}
                    on_queue_change={Callback::from({
                        let queue = queue.clone();
                        move |status| queue.set(Some(status))
                    })}
                />
            </>
        };
    }

    if *mode == AppMode::Stories {
        return html! {
            <>
                <ModeBar mode={*mode} settings={settings.clone()} on_mode={Callback::from({
                    let mode = mode.clone();
                    move |m| mode.set(m)
                })} />
                <StoriesShell
                    queue={(*queue).clone()}
                    on_open_queue={open_queue.clone()}
                    initial_story_id={*pending_story_id}
                />
            </>
        };
    }

    let selected = (*selected_chat_id).and_then(|id| chats.iter().find(|c| c.id == id).cloned());

    let start_chat = {
        let chats = chats.clone();
        let selected_chat_id = selected_chat_id.clone();
        let picker_open = picker_open.clone();
        let load_messages_for_chat = load_messages_for_chat.clone();
        Callback::from(move |(character_id, title): (i64, String)| {
            let chats = chats.clone();
            let selected_chat_id = selected_chat_id.clone();
            let picker_open = picker_open.clone();
            let load_messages_for_chat = load_messages_for_chat.clone();
            wasm_bindgen_futures::spawn_local(async move {
                match api::create_chat(&title, character_id).await {
                    Ok(chat) => {
                        if let Ok(list) = api::list_chats().await {
                            chats.set(list);
                        }
                        selected_chat_id.set(Some(chat.id));
                        load_messages_for_chat.emit(chat.id);
                        picker_open.set(false);
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

    html! {
        <>
            <ModeBar mode={*mode} settings={settings.clone()} on_mode={Callback::from({
                let mode = mode.clone();
                move |m| mode.set(m)
            })} />
            if *picker_open {
                <CharacterPickerModal
                    characters={(*characters).clone()}
                    on_close={Callback::from({
                        let picker_open = picker_open.clone();
                        move |_| picker_open.set(false)
                    })}
                    on_select={Callback::from({
                        let start_chat = start_chat.clone();
                        move |character: Character| {
                            start_chat.emit((character.id, character.name.clone()));
                        }
                    })}
                />
            }
            if *mobile_pane != MobilePane::Main {
                <div class="drawer-backdrop" onclick={Callback::from({
                    let mobile_pane = mobile_pane.clone();
                    move |_| mobile_pane.set(MobilePane::Main)
                })} />
            }
            <div class={classes!(
                "app-shell",
                (*mobile_pane == MobilePane::Sidebar).then_some("pane-sidebar"),
                (*mobile_pane == MobilePane::Panel).then_some("pane-panel"),
            )}>
            <ChatSidebar
                chats={(*chats).clone()}
                selected_id={*selected_chat_id}
                on_select={Callback::from({
                    let selected_chat_id = selected_chat_id.clone();
                    let mobile_pane = mobile_pane.clone();
                    move |id| {
                        selected_chat_id.set(Some(id));
                        mobile_pane.set(MobilePane::Main);
                    }
                })}
                on_new={Callback::from({
                    let picker_open = picker_open.clone();
                    move |_| picker_open.set(true)
                })}
                on_delete={Callback::from({
                    let chats = chats.clone();
                    let selected_chat_id = selected_chat_id.clone();
                    move |id| {
                        let chats = chats.clone();
                        let selected_chat_id = selected_chat_id.clone();
                        wasm_bindgen_futures::spawn_local(async move {
                            let _ = api::delete_chat(id).await;
                            if let Ok(list) = api::list_chats().await {
                                if *selected_chat_id == Some(id) {
                                    selected_chat_id.set(list.first().map(|c| c.id));
                                }
                                chats.set(list);
                            }
                        });
                    }
                })}
            />
            <main class="main">
                <QueueBar queue={(*queue).clone()} on_open={open_queue.clone()} />
                <header class="header">
                    <div class="mobile-toolbar">
                        <button class="btn secondary" onclick={Callback::from({
                            let mobile_pane = mobile_pane.clone();
                            move |_| mobile_pane.set(MobilePane::Sidebar)
                        })}>{"Chats"}</button>
                        <button class="btn secondary" onclick={Callback::from({
                            let mobile_pane = mobile_pane.clone();
                            move |_| mobile_pane.set(MobilePane::Panel)
                        })}>{"Character"}</button>
                    </div>
                    <h1 class="header-title">{selected.as_ref().map(|c| c.title.clone()).unwrap_or_else(|| "Select a chat".to_string())}</h1>
                    if let Some(chat) = selected.as_ref() {
                        <p class="header-subtitle muted">{ format!("With {}", chat.character_name) }</p>
                    } else if chats.is_empty() {
                        <p class="header-subtitle muted">{"Create a character in the panel, then start a chat."}</p>
                    } else {
                        <p class="header-subtitle muted">{"Responses stream on the server — switch chats freely while they generate."}</p>
                    }
                </header>
                if chats.is_empty() {
                    <div class="empty-state muted">
                        if characters.is_empty() {
                            <p>{"No characters yet. Use the Character panel to create or import one."}</p>
                        } else {
                            <p>{"No chats yet. Click New in the sidebar to pick a character."}</p>
                            <button class="btn" style="margin-top:0.75rem;" onclick={{
                                let picker_open = picker_open.clone();
                                Callback::from(move |_| picker_open.set(true))
                            }}>{"Start a chat"}</button>
                        }
                    </div>
                } else {
                    <MessageList
                        chat_id={*selected_chat_id}
                        messages={(*messages).clone()}
                        loading={*messages_loading}
                        settings={(*settings).clone()}
                        character={selected.as_ref().and_then(|chat| {
                            characters.iter().find(|c| c.id == chat.character_id).cloned()
                        })}
                        char_name={selected.as_ref().map(|c| c.character_name.clone())}
                        on_messages_change={Callback::from({
                            let selected_chat_id = selected_chat_id.clone();
                            let messages = messages.clone();
                            let queue = queue.clone();
                            move |_| {
                                let Some(chat_id) = *selected_chat_id else { return };
                                let messages = messages.clone();
                                let queue = queue.clone();
                                wasm_bindgen_futures::spawn_local(async move {
                                    if let Ok(msgs) = api::get_messages(chat_id).await {
                                        messages.set(msgs);
                                    }
                                    if let Ok(status) = api::get_queue().await {
                                        queue.set(Some(status));
                                    }
                                });
                            }
                        })}
                    />
                }
                <Composer
                    disabled={selected_chat_id.is_none()}
                    on_send={Callback::from({
                        let selected_chat_id = selected_chat_id.clone();
                        let messages = messages.clone();
                        let chats = chats.clone();
                        let queue = queue.clone();
                        move |content: String| {
                            let Some(chat_id) = *selected_chat_id else { return };
                            let messages = messages.clone();
                            let chats = chats.clone();
                            let queue = queue.clone();
                            wasm_bindgen_futures::spawn_local(async move {
                                let _ = api::send_message(chat_id, &content).await;
                                if let Ok(msgs) = api::get_messages(chat_id).await {
                                    messages.set(msgs);
                                }
                                if let Ok(list) = api::list_chats().await {
                                    chats.set(list);
                                }
                                if let Ok(status) = api::get_queue().await {
                                    queue.set(Some(status));
                                }
                            });
                        }
                    })}
                />
            </main>
            <RightPanel
                chat_id={*selected_chat_id}
                character_id={selected.as_ref().map(|c| c.character_id)}
                messages={(*messages).clone()}
                on_character_change={Callback::from({
                    let chats = chats.clone();
                    move |(chat_id, character_id)| {
                        let chats = chats.clone();
                        wasm_bindgen_futures::spawn_local(async move {
                            let _ = api::update_chat(chat_id, character_id).await;
                            if let Ok(list) = api::list_chats().await {
                                chats.set(list);
                            }
                        });
                    }
                })}
                on_start_chat={start_chat.clone()}
                on_chat_created={Callback::from({
                    let chats = chats.clone();
                    let selected_chat_id = selected_chat_id.clone();
                    let load_messages_for_chat = load_messages_for_chat.clone();
                    move |chat_id| {
                        let chats = chats.clone();
                        let selected_chat_id = selected_chat_id.clone();
                        let load_messages_for_chat = load_messages_for_chat.clone();
                        wasm_bindgen_futures::spawn_local(async move {
                            if let Ok(list) = api::list_chats().await {
                                chats.set(list);
                            }
                            selected_chat_id.set(Some(chat_id));
                            load_messages_for_chat.emit(chat_id);
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
        </div>
        </>
    }
}

#[derive(Properties, PartialEq)]
struct ModeBarProps {
    mode: AppMode,
    settings: UseStateHandle<Option<Settings>>,
    on_mode: Callback<AppMode>,
}

#[function_component(ModeBar)]
fn mode_bar(props: &ModeBarProps) -> Html {
    let settings_open = use_state(|| false);
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
        use_effect_with(*settings_open, move |open| {
            if *open {
                save_ctx.ensure_loaded();
                if (*save_ctx.draft).is_none() {
                    wasm_bindgen_futures::spawn_local(async move {
                        if let Ok(settings) = api::get_settings().await {
                            save_ctx.parent_settings.set(Some(settings.clone()));
                            save_ctx.load_from(settings);
                        }
                    });
                }
            }
            || ()
        });
    }

    html! {
        <>
            if *settings_open {
                <div class="settings-backdrop" onclick={{
                    let settings_open = settings_open.clone();
                    Callback::from(move |_| settings_open.set(false))
                }} />
            }
            <div class="mode-bar">
                <div class="mode-bar-tabs">
                    <button class={classes!("mode-btn", (props.mode == AppMode::Chats).then_some("active"))}
                        onclick={props.on_mode.reform(|_| AppMode::Chats)}>{"Chats"}</button>
                    <button class={classes!("mode-btn", (props.mode == AppMode::Stories).then_some("active"))}
                        onclick={props.on_mode.reform(|_| AppMode::Stories)}>{"Stories"}</button>
                </div>
                <button class={classes!("mode-btn", (*settings_open).then_some("active"))} onclick={{
                    let settings_open = settings_open.clone();
                    Callback::from(move |_| settings_open.set(!*settings_open))
                }}>{"Settings"}</button>
            </div>
            if *settings_open {
                <div class="settings-popover">
                    <div class="settings-header">
                        <h2>{"Settings"}</h2>
                        <button class="btn secondary btn-compact" onclick={{
                            let settings_open = settings_open.clone();
                            Callback::from(move |_| settings_open.set(false))
                        }}>{"Close"}</button>
                    </div>
                    <SettingsPanel save_ctx={save_ctx.clone()} />
                </div>
            }
        </>
    }
}

#[derive(Properties, PartialEq)]
struct ChatSidebarProps {
    chats: Vec<Chat>,
    selected_id: Option<i64>,
    on_select: Callback<i64>,
    on_new: Callback<()>,
    on_delete: Callback<i64>,
}

#[function_component(ChatSidebar)]
fn chat_sidebar(props: &ChatSidebarProps) -> Html {
    html! {
        <aside class="sidebar">
            <div class="header sidebar-header">
                <div>
                    <div class="muted" style="text-transform:uppercase;letter-spacing:0.2em;font-size:0.7rem;">{"Dreamwell"}</div>
                    <strong>{"Chats"}</strong>
                </div>
                <button class="btn" onclick={props.on_new.reform(|_| ())}>{"New"}</button>
            </div>
            <div class="sidebar-scroll">
                { for props.chats.iter().map(|chat| {
                    let id = chat.id;
                    let status = chat_status(chat);
                    let selected = props.selected_id == Some(chat.id);
                    html! {
                        <div class={classes!("chat-item", selected.then_some("selected"))}>
                            <div style="display:flex;gap:0.5rem;">
                                <div style="flex:1;min-width:0;" onclick={props.on_select.reform(move |_| id)}>
                                    <div style="overflow:hidden;text-overflow:ellipsis;white-space:nowrap;">{ &chat.title }</div>
                                    <div class="chat-character">{ &chat.character_name }</div>
                                    if let Some(label) = status {
                                        <span class="badge">{ label }</span>
                                    }
                                </div>
                                <button class="btn secondary btn-compact" onclick={props.on_delete.reform(move |_| id)}>{"✕"}</button>
                            </div>
                        </div>
                    }
                }) }
            </div>
        </aside>
    }
}

fn chat_status(chat: &Chat) -> Option<String> {
    let job = chat.active_job.as_ref()?;
    match job.status {
        JobStatus::Running => Some("writing…".to_string()),
        JobStatus::Queued => {
            if chat.queued_jobs > 1 {
                Some(format!("queued ({})", chat.queued_jobs))
            } else {
                Some("queued".to_string())
            }
        }
        _ => Some(format!("{:?}", job.status).to_lowercase()),
    }
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

fn format_variable_update_summary(update: &MessageVariableUpdate) -> String {
    if let Some(previous) = &update.previous_value {
        format!("{}: {} → {}", update.key, previous, update.value)
    } else {
        format!("{} → {}", update.key, update.value)
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
    let summary = props
        .updates
        .iter()
        .map(format_variable_update_summary)
        .collect::<Vec<_>>()
        .join(", ");

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
                    { for props.updates.iter().map(|update| {
                        html! {
                            <div class="message-variable-update-item">
                                { format_variable_update_summary(update) }
                            </div>
                        }
                    }) }
                </div>
            } else {
                <div class="message-variable-updates-summary muted">{ summary }</div>
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
    rendered_content: Html,
    show_thoughts: bool,
    show_variables: bool,
    on_changed: Callback<()>,
}

#[function_component(MessageBubble)]
fn message_bubble(props: &MessageBubbleProps) -> Html {
    let menu_open = use_state(|| false);
    let mode = use_state(|| MessageBubbleMode::View);
    let edit_text = use_state(String::new);
    let rewind = use_state(|| false);
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
    let show_thought_block = props.show_thoughts
        && props.message.role == MessageRole::Assistant
        && (props.message.thought_in_progress
            || !props.message.thought_content.is_empty()
            || props.message.thought_duration_ms.is_some());
    let show_variable_updates = props.show_variables
        && props.message.role == MessageRole::Assistant
        && !props.message.variable_updates.is_empty();

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
        let rewind = rewind.clone();
        let acting = acting.clone();
        let on_changed = props.on_changed.clone();
        let chat_id = props.chat_id;
        let message_id = props.message.id;
        let after_count = props.after_count;
        Callback::from(move |_| {
            let content = (*edit_text).trim().to_string();
            if content.is_empty() || *acting {
                return;
            }
            let rewind_enabled = *rewind;
            let delete_count = if rewind_enabled { after_count } else { 0 };
            if !confirm_delete_after(delete_count) {
                return;
            }
            acting.set(true);
            let mode = mode.clone();
            let acting = acting.clone();
            let on_changed = on_changed.clone();
            wasm_bindgen_futures::spawn_local(async move {
                match api::update_message(chat_id, message_id, &content, rewind_enabled).await {
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

    let regenerate = {
        let menu_open = menu_open.clone();
        let rewind = rewind.clone();
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
            let rewind_enabled = *rewind;
            let delete_count = if !is_last || rewind_enabled {
                after_count
            } else {
                0
            };
            if !confirm_delete_after(delete_count) {
                return;
            }
            menu_open.set(false);
            acting.set(true);
            let acting = acting.clone();
            let on_changed = on_changed.clone();
            wasm_bindgen_futures::spawn_local(async move {
                match api::regenerate_message(chat_id, message_id, rewind_enabled).await {
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

    html! {
        <div class={classes!("message", role)}>
            <div class="message-header">
                <div class="message-meta muted">
                    { role.to_string() }
                    if queued { <span>{" · queued on server"}</span> }
                    if streaming { <span>{" · streaming on server"}</span> }
                </div>
                if can_menu {
                    <div class="message-menu-wrap">
                        if *menu_open {
                            <div class="message-menu-backdrop" onclick={close_menu.clone()} />
                        }
                        <button
                            type="button"
                            class="message-menu-btn"
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
                            <div class="message-menu" onclick={Callback::from(|e: MouseEvent| e.stop_propagation())}>
                                <button type="button" class="message-menu-item" onclick={start_edit}>{"Edit"}</button>
                                if show_regenerate {
                                    <button type="button" class="message-menu-item" onclick={regenerate}>{"Regenerate"}</button>
                                }
                                <label class="message-menu-rewind">
                                    <input
                                        type="checkbox"
                                        checked={*rewind}
                                        disabled={props.after_count == 0}
                                        onclick={Callback::from({
                                            let rewind = rewind.clone();
                                            move |_| rewind.set(!*rewind)
                                        })}
                                    />
                                    if props.after_count == 0 {
                                        <span>{"Rewind (nothing after)"}</span>
                                    } else {
                                        <span>{ format!("Rewind (delete {after} after)", after = props.after_count) }</span>
                                    }
                                </label>
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
                    <label class="message-menu-rewind">
                        <input
                            type="checkbox"
                            checked={*rewind}
                            disabled={props.after_count == 0}
                            onclick={Callback::from({
                                let rewind = rewind.clone();
                                move |_| rewind.set(!*rewind)
                            })}
                        />
                        if props.after_count == 0 {
                            <span>{"Rewind (nothing after)"}</span>
                        } else {
                            <span>{ format!("Rewind (delete {after} after)", after = props.after_count) }</span>
                        }
                    </label>
                </div>
            } else if props.message.content.is_empty() && active {
                { "…" }
            } else {
                { props.rendered_content.clone() }
            }
            if show_variable_updates {
                <VariableUpdatesBlock updates={props.message.variable_updates.clone()} />
            }
        </div>
    }
}

#[derive(Properties, PartialEq)]
struct MessageListProps {
    chat_id: Option<i64>,
    messages: Vec<Message>,
    loading: bool,
    settings: Option<Settings>,
    character: Option<Character>,
    char_name: Option<String>,
    on_messages_change: Callback<()>,
}

#[function_component(MessageList)]
fn message_list(props: &MessageListProps) -> Html {
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
        <div class="messages">
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
                    let rendered_content = if m.content.is_empty() {
                        html! {}
                    } else if let Some(ctx) = macro_ctx.as_ref() {
                        markdown::render_message_content(&substitute_macros(&m.content, ctx))
                    } else {
                        markdown::render_message_content(&m.content)
                    };
                    html! {
                        <MessageBubble
                            message={m.clone()}
                            chat_id={chat_id}
                            is_last={is_last}
                            after_count={after_count}
                            rendered_content={rendered_content}
                            show_thoughts={show_thoughts}
                            show_variables={show_variables}
                            on_changed={props.on_messages_change.clone()}
                        />
                    }
                }) }
            }
        </div>
    }
}

#[derive(Properties, PartialEq)]
struct ComposerProps {
    disabled: bool,
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

    html! {
        <div class="composer">
            <textarea
                value={(*text).clone()}
                oninput={Callback::from({
                    let text = text.clone();
                    move |e: InputEvent| {
                        let input: HtmlInputElement = e.target_unchecked_into();
                        text.set(input.value());
                    }
                })}
                placeholder="Write your message…"
                disabled={props.disabled || *sending}
            />
            <button class="btn" onclick={on_send} disabled={props.disabled || *sending || text.trim().is_empty()}>
                { if *sending { "Queuing…" } else { "Send" } }
            </button>
        </div>
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
                    <p class="muted">{"No characters yet. Create or import one in the Character panel first."}</p>
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
struct RightPanelProps {
    chat_id: Option<i64>,
    character_id: Option<i64>,
    messages: Vec<Message>,
    on_character_change: Callback<(i64, i64)>,
    on_start_chat: Callback<(i64, String)>,
    on_chat_created: Callback<i64>,
    on_characters_changed: Callback<()>,
}

#[function_component(RightPanel)]
fn right_panel(props: &RightPanelProps) -> Html {
    let tab = use_state(|| 0u8);
    html! {
        <aside class="panel">
            <div class="tabs">
                <button class={classes!("tab", (*tab == 0).then_some("active"))} onclick={{
                    let tab = tab.clone();
                    Callback::from(move |_| tab.set(0))
                }}>{"Character"}</button>
                <button class={classes!("tab", (*tab == 1).then_some("active"))} onclick={{
                    let tab = tab.clone();
                    Callback::from(move |_| tab.set(1))
                }}>{"Variables"}</button>
            </div>
            <div class="panel-body">
                if *tab == 0 {
                    <CharacterPanel
                        selected_character_id={props.character_id}
                        on_character_change={props.on_character_change.clone()}
                        on_start_chat={props.on_start_chat.clone()}
                        on_chat_created={props.on_chat_created.clone()}
                        on_characters_changed={props.on_characters_changed.clone()}
                        chat_id={props.chat_id}
                    />
                } else {
                    <VariablesPanel chat_id={props.chat_id} messages={props.messages.clone()} />
                }
            </div>
        </aside>
    }
}

#[derive(Properties, PartialEq)]
struct CharacterPanelProps {
    selected_character_id: Option<i64>,
    chat_id: Option<i64>,
    on_character_change: Callback<(i64, i64)>,
    on_start_chat: Callback<(i64, String)>,
    on_chat_created: Callback<i64>,
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
                    let on_chat_created = props.on_chat_created.clone();
                    let on_characters_changed = props.on_characters_changed.clone();
                    let chat_id = props.chat_id;
                    Callback::from(move |e: Event| {
                        let input: HtmlInputElement = e.target_unchecked_into();
                        if let Some(file) = input.files().and_then(|f| f.get(0)) {
                            let characters = characters.clone();
                            let draft = draft.clone();
                            let editing_id = editing_id.clone();
                            let on_character_change = on_character_change.clone();
                            let on_chat_created = on_chat_created.clone();
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
                                    } else if let Ok(chat) = api::create_chat(&character.name, character.id).await {
                                        on_chat_created.emit(chat.id);
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
                                let on_characters_changed = props.on_characters_changed.clone();
                                Callback::from(move |e: MouseEvent| {
                                    e.stop_propagation();
                                    let characters = characters.clone();
                                    let on_characters_changed = on_characters_changed.clone();
                                    wasm_bindgen_futures::spawn_local(async move {
                                        let _ = api::delete_character(id).await;
                                        if let Ok(list) = api::list_characters().await {
                                            characters.set(list);
                                        }
                                        on_characters_changed.emit(());
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
                let on_chat_created = props.on_chat_created.clone();
                let on_characters_changed = props.on_characters_changed.clone();
                let chat_id = props.chat_id;
                Callback::from(move |_| {
                    let payload = draft.to_create();
                    let editing_id_val = *editing_id;
                    let characters = characters.clone();
                    let draft = draft.clone();
                    let editing_id = editing_id.clone();
                    let on_character_change = on_character_change.clone();
                    let on_chat_created = on_chat_created.clone();
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
                            } else if let Ok(chat) = api::create_chat(&character.name, character.id).await {
                                on_chat_created.emit(chat.id);
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

    html! {
        <div>
            <p class="muted">{"Chat variables are injected into the prompt. The model can update them with var tags."}</p>
            { for variables.iter().map(|v| {
                let variable_key = v.key.clone();
                let chat_id_for_delete = chat_id;
                html! {
                    <div class="variable-card">
                        <div style="display:flex;justify-content:space-between;">
                            <strong>{ &v.key }</strong>
                            <button class="btn secondary btn-compact" onclick={{
                                let variables = variables.clone();
                                let variable_key = variable_key.clone();
                                let chat_id = chat_id_for_delete;
                                Callback::from(move |_| {
                                    let variables = variables.clone();
                                    let key = variable_key.clone();
                                    wasm_bindgen_futures::spawn_local(async move {
                                        let _ = api::delete_variable(chat_id, &key).await;
                                        if let Ok(list) = api::get_variables(chat_id).await {
                                            variables.set(list);
                                        }
                                    });
                                })
                            }}>{"delete"}</button>
                        </div>
                        <div style="white-space:pre-wrap;">{ &v.value }</div>
                    </div>
                }
            }) }
            <label class="field"><span class="muted">{"Key"}</span><input value={(*key).clone()} oninput={input_callback(key.clone())} /></label>
            <label class="field"><span class="muted">{"Value"}</span><textarea value={(*value).clone()} oninput={input_callback(value.clone())} /></label>
            <button class="btn" onclick={{
                let variables = variables.clone();
                let key = key.clone();
                let value = value.clone();
                Callback::from(move |_| {
                    if key.trim().is_empty() { return; }
                    let variables = variables.clone();
                    let k = (*key).clone();
                    let v = (*value).clone();
                    let key = key.clone();
                    let value = value.clone();
                    wasm_bindgen_futures::spawn_local(async move {
                        let _ = api::upsert_variable(chat_id, &k, &v).await;
                        key.set(String::new());
                        value.set(String::new());
                        if let Ok(list) = api::get_variables(chat_id).await {
                            variables.set(list);
                        }
                    });
                })
            }}>{"Save variable"}</button>
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
        summarize_after_messages: Some(current.summarize_after_messages),
        summarize_keep_recent: Some(current.summarize_keep_recent),
        variables_enabled: Some(current.variables_enabled),
        thought_blocks_enabled: Some(current.thought_blocks_enabled),
        max_context_messages: Some(current.max_context_messages),
        max_concurrent_jobs: Some(current.max_concurrent_jobs),
    }
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
}

#[function_component(SettingsPanel)]
fn settings_panel(props: &SettingsPanelProps) -> Html {
    let save_ctx = props.save_ctx.clone();
    let draft = save_ctx.draft.clone();
    let phase = save_ctx.phase.clone();
    let models = use_state(Vec::<ModelInfo>::new);
    let model_error = use_state(|| None::<String>);

    let Some(s) = (*draft).clone() else {
        return html! { <p class="muted">{"Loading settings…"}</p> };
    };

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
                        Callback::from(move |e: Event| {
                            let input: HtmlInputElement = e.target_unchecked_into();
                            save_ctx.update_field(|current| current.model = input.value());
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
            <div class="settings-params-grid">
                <label class="field"><span class="muted">{"Temperature"}</span><input type="number" step="0.05" value={s.temperature.to_string()} oninput={num_input(save_ctx.clone(), "temperature")} /></label>
                <label class="field"><span class="muted">{"Top P"}</span><input type="number" step="0.05" value={s.top_p.to_string()} oninput={num_input(save_ctx.clone(), "top_p")} /></label>
                <label class="field"><span class="muted">{"Max tokens"}</span><input type="number" value={s.max_tokens.to_string()} oninput={num_input(save_ctx.clone(), "max_tokens")} /></label>
                <label class="field"><span class="muted">{"Max concurrent jobs"}</span><input type="number" value={s.max_concurrent_jobs.to_string()} oninput={num_input(save_ctx.clone(), "max_concurrent_jobs")} /></label>
            </div>
            <label class="field"><span class="muted">{"User name ({{user}})"}</span><input value={s.user_name.clone()} oninput={text_input(save_ctx.clone(), "user_name")} /></label>
            <label class="field"><span class="muted">{"Persona description ({{persona}})"}</span><textarea value={s.persona_description.clone()} rows="3" oninput={text_input(save_ctx.clone(), "persona_description")} /></label>
            <label class="field"><span class="muted">{"Main prompt (prefix)"}</span><textarea value={s.system_prompt_prefix.clone()} rows="3" oninput={text_input(save_ctx.clone(), "system_prompt_prefix")} /></label>
            <label class="field"><span class="muted">{"Post-history instructions (suffix)"}</span><textarea value={s.system_prompt_suffix.clone()} rows="3" oninput={text_input(save_ctx.clone(), "system_prompt_suffix")} /></label>
            <div class="settings-group">
                <strong>{"Auto summarize"}</strong>
                <label style="display:flex;gap:0.5rem;margin:0.5rem 0;">
                    <input type="checkbox" checked={s.summarize_enabled} onclick={{
                        let save_ctx = save_ctx.clone();
                        Callback::from(move |_| {
                            save_ctx.update_field(|current| current.summarize_enabled = !current.summarize_enabled);
                        })
                    }} />
                    {"Enable summarization"}
                </label>
                <label class="field"><span class="muted">{"Summarize after N messages"}</span><input type="number" value={s.summarize_after_messages.to_string()} oninput={num_input(save_ctx.clone(), "summarize_after_messages")} /></label>
                <label class="field"><span class="muted">{"Keep recent messages"}</span><input type="number" value={s.summarize_keep_recent.to_string()} oninput={num_input(save_ctx.clone(), "summarize_keep_recent")} /></label>
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
                "max_tokens" => current.max_tokens = v as i64,
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

#[wasm_bindgen::prelude::wasm_bindgen(start)]
pub fn main() {
    yew::Renderer::<App>::new().render();
}
