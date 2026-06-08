mod api;
mod stories_ui;

use dreamwell_types::*;
use gloo_timers::callback::Interval;
use stories_ui::{QueueBar, StoriesShell};
use web_sys::HtmlInputElement;
use yew::prelude::*;

#[derive(Clone, Copy, PartialEq)]
enum AppMode {
    Chats,
    Stories,
}

#[function_component(App)]
fn app() -> Html {
    let mode = use_state(|| AppMode::Chats);
    let chats = use_state(Vec::<Chat>::new);
    let selected_chat_id = use_state(|| None::<i64>);
    let messages = use_state(Vec::<Message>::new);
    let queue = use_state(|| None::<QueueStatus>);
    let loading = use_state(|| true);

    {
        let chats = chats.clone();
        let selected_chat_id = selected_chat_id.clone();
        let loading = loading.clone();
        use_effect_with((), move |_| {
            wasm_bindgen_futures::spawn_local(async move {
                if let Ok(list) = api::list_chats().await {
                    if let Some(first) = list.first() {
                        selected_chat_id.set(Some(first.id));
                    }
                    chats.set(list);
                }
                loading.set(false);
            });
            || ()
        });
    }

    {
        let messages = messages.clone();
        let chats = chats.clone();
        let selected_chat_id = *selected_chat_id;
        use_effect_with(selected_chat_id, move |chat_id| {
            let mut stream_holder = None::<api::ChatStream>;
            if let Some(chat_id) = *chat_id {
                let messages_for_fetch = messages.clone();
                let chats = chats.clone();
                wasm_bindgen_futures::spawn_local(async move {
                    if let Ok(msgs) = api::get_messages(chat_id).await {
                        messages_for_fetch.set(msgs);
                    }
                });
                stream_holder = Some(api::ChatStream::new(chat_id, move |payload| {
                    messages.set(payload.messages.clone());
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
                }));
            } else {
                messages.set(vec![]);
            }
            move || {
                drop(stream_holder);
            }
        });
    }

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

    if *loading && *mode == AppMode::Chats {
        return html! { <div class="muted" style="padding:2rem;">{"Loading Dreamwell…"}</div> };
    }

    if *mode == AppMode::Stories {
        return html! {
            <>
                <ModeBar mode={*mode} on_mode={Callback::from({
                    let mode = mode.clone();
                    move |m| mode.set(m)
                })} />
                <StoriesShell queue={(*queue).clone()} />
            </>
        };
    }

    let selected = (*selected_chat_id).and_then(|id| chats.iter().find(|c| c.id == id).cloned());

    html! {
        <>
            <ModeBar mode={*mode} on_mode={Callback::from({
                let mode = mode.clone();
                move |m| mode.set(m)
            })} />
            <div class="app-shell">
            <ChatSidebar
                chats={(*chats).clone()}
                selected_id={*selected_chat_id}
                on_select={Callback::from({
                    let selected_chat_id = selected_chat_id.clone();
                    move |id| selected_chat_id.set(Some(id))
                })}
                on_new={Callback::from({
                    let chats = chats.clone();
                    let selected_chat_id = selected_chat_id.clone();
                    let character_id = selected.as_ref().and_then(|c| c.character_id);
                    move |_| {
                        let chats = chats.clone();
                        let selected_chat_id = selected_chat_id.clone();
                        let character_id = character_id;
                        wasm_bindgen_futures::spawn_local(async move {
                            let title = format!("Chat {}", chats.len() + 1);
                            if let Ok(chat) = api::create_chat(&title, character_id).await {
                                if let Ok(list) = api::list_chats().await {
                                    chats.set(list);
                                }
                                selected_chat_id.set(Some(chat.id));
                            }
                        });
                    }
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
                <QueueBar queue={(*queue).clone()} />
                <header class="header">
                    <h1 style="margin:0;font-size:1.1rem;">{selected.as_ref().map(|c| c.title.clone()).unwrap_or_else(|| "Select a chat".to_string())}</h1>
                    <p class="muted" style="margin:0.25rem 0 0;">{"Responses stream on the server — switch chats freely while they generate."}</p>
                </header>
                <MessageList messages={(*messages).clone()} />
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
                character_id={selected.as_ref().and_then(|c| c.character_id)}
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
            />
        </div>
        </>
    }
}

#[derive(Properties, PartialEq)]
struct ModeBarProps {
    mode: AppMode,
    on_mode: Callback<AppMode>,
}

#[function_component(ModeBar)]
fn mode_bar(props: &ModeBarProps) -> Html {
    html! {
        <div class="mode-bar">
            <button class={classes!("mode-btn", (props.mode == AppMode::Chats).then_some("active"))}
                onclick={props.on_mode.reform(|_| AppMode::Chats)}>{"Chats"}</button>
            <button class={classes!("mode-btn", (props.mode == AppMode::Stories).then_some("active"))}
                onclick={props.on_mode.reform(|_| AppMode::Stories)}>{"Stories"}</button>
        </div>
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
            <div class="header" style="display:flex;justify-content:space-between;align-items:center;">
                <div>
                    <div class="muted" style="text-transform:uppercase;letter-spacing:0.2em;font-size:0.7rem;">{"Dreamwell"}</div>
                    <strong>{"Chats"}</strong>
                </div>
                <button class="btn" onclick={props.on_new.reform(|_| ())}>{"New"}</button>
            </div>
            <div style="flex:1;overflow-y:auto;padding:0.5rem;">
                { for props.chats.iter().map(|chat| {
                    let id = chat.id;
                    let status = chat_status(chat);
                    let selected = props.selected_id == Some(chat.id);
                    html! {
                        <div class={classes!("chat-item", selected.then_some("selected"))}>
                            <div style="display:flex;gap:0.5rem;">
                                <div style="flex:1;min-width:0;" onclick={props.on_select.reform(move |_| id)}>
                                    <div style="overflow:hidden;text-overflow:ellipsis;white-space:nowrap;">{ &chat.title }</div>
                                    if let Some(label) = status {
                                        <span class="badge">{ label }</span>
                                    }
                                </div>
                                <button class="btn secondary" style="padding:0.2rem 0.5rem;font-size:0.75rem;" onclick={props.on_delete.reform(move |_| id)}>{"✕"}</button>
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

#[derive(Properties, PartialEq)]
struct MessageListProps {
    messages: Vec<Message>,
}

#[function_component(MessageList)]
fn message_list(props: &MessageListProps) -> Html {
    html! {
        <div class="messages">
            if props.messages.is_empty() {
                <div class="muted" style="text-align:center;padding:2rem;border:1px dashed rgba(88,28,135,0.5);border-radius:1rem;">
                    {"Send a message to queue a reply. You can switch chats while it generates server-side."}
                </div>
            } else {
                { for props.messages.iter().map(|m| {
                    let role = match m.role {
                        MessageRole::User => "user",
                        MessageRole::Assistant => "assistant",
                        MessageRole::System => "system",
                    };
                    let streaming = matches!(m.job_status, Some(JobStatus::Running) | Some(JobStatus::Queued));
                    html! {
                        <div class={classes!("message", role)}>
                            <div class="muted" style="font-size:0.75rem;text-transform:uppercase;">
                                { role.to_string() }
                                if streaming { <span>{" · streaming on server"}</span> }
                            </div>
                            { if m.content.is_empty() && streaming { "…".to_string() } else { m.content.clone() } }
                        </div>
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
struct RightPanelProps {
    chat_id: Option<i64>,
    character_id: Option<i64>,
    on_character_change: Callback<(i64, Option<i64>)>,
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
                }}>{"Facts"}</button>
                <button class={classes!("tab", (*tab == 2).then_some("active"))} onclick={{
                    let tab = tab.clone();
                    Callback::from(move |_| tab.set(2))
                }}>{"Settings"}</button>
            </div>
            <div class="panel-body">
                if *tab == 0 {
                    <CharacterPanel
                        selected_character_id={props.character_id}
                        on_character_change={props.on_character_change.clone()}
                        chat_id={props.chat_id}
                    />
                } else if *tab == 1 {
                    <FactsPanel chat_id={props.chat_id} />
                } else {
                    <SettingsPanel />
                }
            </div>
        </aside>
    }
}

#[derive(Properties, PartialEq)]
struct CharacterPanelProps {
    selected_character_id: Option<i64>,
    chat_id: Option<i64>,
    on_character_change: Callback<(i64, Option<i64>)>,
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
                    let chat_id = props.chat_id;
                    Callback::from(move |e: Event| {
                        let input: HtmlInputElement = e.target_unchecked_into();
                        if let Some(file) = input.files().and_then(|f| f.get(0)) {
                            let characters = characters.clone();
                            let draft = draft.clone();
                            let editing_id = editing_id.clone();
                            let on_character_change = on_character_change.clone();
                            wasm_bindgen_futures::spawn_local(async move {
                                if let Ok(character) = api::import_character(&file).await {
                                    if let Ok(list) = api::list_characters().await {
                                        characters.set(list);
                                    }
                                    editing_id.set(Some(character.id));
                                    draft.set(CharacterDraft::from(&character));
                                    if let Some(chat_id) = chat_id {
                                        on_character_change.emit((chat_id, Some(character.id)));
                                    }
                                }
                            });
                        }
                    })
                }} />
            </div>
            <div style="max-height:10rem;overflow-y:auto;border:1px solid rgba(88,28,135,0.3);border-radius:0.5rem;margin-bottom:1rem;">
                { for characters.iter().map(|c| {
                    let id = c.id;
                    html! {
                        <div style="display:flex;justify-content:space-between;padding:0.5rem;cursor:pointer;"
                            onclick={{
                                let draft = draft.clone();
                                let editing_id = editing_id.clone();
                                let c = c.clone();
                                Callback::from(move |_| {
                                    editing_id.set(Some(id));
                                    draft.set(CharacterDraft::from(&c));
                                })
                            }}>
                            <span>{ &c.name }</span>
                            <button class="btn secondary" style="padding:0.1rem 0.4rem;font-size:0.75rem;" onclick={{
                                let characters = characters.clone();
                                Callback::from(move |e: MouseEvent| {
                                    e.stop_propagation();
                                    let characters = characters.clone();
                                    wasm_bindgen_futures::spawn_local(async move {
                                        let _ = api::delete_character(id).await;
                                        if let Ok(list) = api::list_characters().await {
                                            characters.set(list);
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
                let chat_id = props.chat_id;
                Callback::from(move |_| {
                    let payload = draft.to_create();
                    let editing_id_val = *editing_id;
                    let characters = characters.clone();
                    let draft = draft.clone();
                    let editing_id = editing_id.clone();
                    let on_character_change = on_character_change.clone();
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
                            editing_id.set(Some(character.id));
                            draft.set(CharacterDraft::from(&character));
                            if let Some(chat_id) = chat_id {
                                on_character_change.emit((chat_id, Some(character.id)));
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

#[derive(Properties, PartialEq)]
struct FactsPanelProps {
    chat_id: Option<i64>,
}

#[function_component(FactsPanel)]
fn facts_panel(props: &FactsPanelProps) -> Html {
    let facts = use_state(Vec::<Fact>::new);
    let key = use_state(String::new);
    let value = use_state(String::new);

    {
        let facts = facts.clone();
        let chat_id = props.chat_id;
        use_effect_with(chat_id, move |chat_id| {
            if let Some(chat_id) = *chat_id {
                let facts = facts.clone();
                wasm_bindgen_futures::spawn_local(async move {
                    if let Ok(list) = api::get_facts(chat_id).await {
                        facts.set(list);
                    }
                });
            } else {
                facts.set(vec![]);
            }
            || ()
        });
    }

    let Some(chat_id) = props.chat_id else {
        return html! { <p class="muted">{"Select a chat to view facts."}</p> };
    };

    html! {
        <div>
            <p class="muted">{"Facts are injected into the prompt. The model can update them with fact tags."}</p>
            { for facts.iter().map(|f| {
                let fact_key = f.key.clone();
                let chat_id_for_delete = chat_id;
                html! {
                    <div style="border:1px solid rgba(88,28,135,0.4);border-radius:0.5rem;padding:0.75rem;margin-bottom:0.5rem;">
                        <div style="display:flex;justify-content:space-between;">
                            <strong>{ &f.key }</strong>
                            <button class="btn secondary" style="padding:0.1rem 0.4rem;font-size:0.75rem;" onclick={{
                                let facts = facts.clone();
                                let fact_key = fact_key.clone();
                                let chat_id = chat_id_for_delete;
                                Callback::from(move |_| {
                                    let facts = facts.clone();
                                    let key = fact_key.clone();
                                    wasm_bindgen_futures::spawn_local(async move {
                                        let _ = api::delete_fact(chat_id, &key).await;
                                        if let Ok(list) = api::get_facts(chat_id).await {
                                            facts.set(list);
                                        }
                                    });
                                })
                            }}>{"delete"}</button>
                        </div>
                        <div style="white-space:pre-wrap;">{ &f.value }</div>
                    </div>
                }
            }) }
            <label class="field"><span class="muted">{"Key"}</span><input value={(*key).clone()} oninput={input_callback(key.clone())} /></label>
            <label class="field"><span class="muted">{"Value"}</span><textarea value={(*value).clone()} oninput={input_callback(value.clone())} /></label>
            <button class="btn" onclick={{
                let facts = facts.clone();
                let key = key.clone();
                let value = value.clone();
                Callback::from(move |_| {
                    if key.trim().is_empty() { return; }
                    let facts = facts.clone();
                    let k = (*key).clone();
                    let v = (*value).clone();
                    let key = key.clone();
                    let value = value.clone();
                    wasm_bindgen_futures::spawn_local(async move {
                        let _ = api::upsert_fact(chat_id, &k, &v).await;
                        key.set(String::new());
                        value.set(String::new());
                        if let Ok(list) = api::get_facts(chat_id).await {
                            facts.set(list);
                        }
                    });
                })
            }}>{"Save fact"}</button>
        </div>
    }
}

fn input_callback(state: UseStateHandle<String>) -> Callback<InputEvent> {
    Callback::from(move |e: InputEvent| {
        let input: HtmlInputElement = e.target_unchecked_into();
        state.set(input.value());
    })
}

#[function_component(SettingsPanel)]
fn settings_panel() -> Html {
    let settings = use_state(|| None::<Settings>);
    let models = use_state(Vec::<ModelInfo>::new);
    let model_error = use_state(|| None::<String>);
    let saving = use_state(|| false);

    {
        let settings = settings.clone();
        use_effect_with((), move |_| {
            wasm_bindgen_futures::spawn_local(async move {
                if let Ok(s) = api::get_settings().await {
                    settings.set(Some(s));
                }
            });
            || ()
        });
    }

    let Some(s) = (*settings).clone() else {
        return html! { <p class="muted">{"Loading settings…"}</p> };
    };

    html! {
        <div>
            <label class="field">
                <span class="muted">{"Inference server"}</span>
                <input value={s.inference_url.clone()} oninput={{
                    let settings = settings.clone();
                    Callback::from(move |e: InputEvent| {
                        let input: HtmlInputElement = e.target_unchecked_into();
                        if let Some(mut current) = (*settings).clone() {
                            current.inference_url = input.value();
                            settings.set(Some(current));
                        }
                    })
                }} />
            </label>
            <div style="display:flex;gap:0.5rem;align-items:end;">
                <label class="field" style="flex:1;">
                    <span class="muted">{"Model"}</span>
                    <select onchange={{
                        let settings = settings.clone();
                        Callback::from(move |e: Event| {
                            let input: HtmlInputElement = e.target_unchecked_into();
                            if let Some(mut current) = (*settings).clone() {
                                current.model = input.value();
                                settings.set(Some(current));
                            }
                        })
                    }}>
                        <option value="">{"Select a model"}</option>
                        { for models.iter().map(|m| html! { <option value={m.id.clone()} selected={m.id == s.model}>{ m.name.clone().unwrap_or(m.id.clone()) }</option> }) }
                    </select>
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
                <p style="color:#fca5a5;">{ err }</p>
            }
            <div style="display:grid;grid-template-columns:1fr 1fr;gap:0.75rem;">
                <label class="field"><span class="muted">{"Temperature"}</span><input type="number" step="0.05" value={s.temperature.to_string()} oninput={num_input(settings.clone(), "temperature")} /></label>
                <label class="field"><span class="muted">{"Top P"}</span><input type="number" step="0.05" value={s.top_p.to_string()} oninput={num_input(settings.clone(), "top_p")} /></label>
                <label class="field"><span class="muted">{"Max tokens"}</span><input type="number" value={s.max_tokens.to_string()} oninput={num_input(settings.clone(), "max_tokens")} /></label>
                <label class="field"><span class="muted">{"Max concurrent jobs"}</span><input type="number" value={s.max_concurrent_jobs.to_string()} oninput={num_input(settings.clone(), "max_concurrent_jobs")} /></label>
            </div>
            <label class="field"><span class="muted">{"System prompt prefix"}</span><textarea value={s.system_prompt_prefix.clone()} rows="3" oninput={text_input(settings.clone(), "system_prompt_prefix")} /></label>
            <label class="field"><span class="muted">{"System prompt suffix"}</span><textarea value={s.system_prompt_suffix.clone()} rows="3" oninput={text_input(settings.clone(), "system_prompt_suffix")} /></label>
            <div style="border:1px solid rgba(88,28,135,0.3);border-radius:0.75rem;padding:0.75rem;margin-bottom:0.75rem;">
                <strong>{"Auto summarize"}</strong>
                <label style="display:flex;gap:0.5rem;margin:0.5rem 0;">
                    <input type="checkbox" checked={s.summarize_enabled} onclick={{
                        let settings = settings.clone();
                        Callback::from(move |_| {
                            if let Some(mut current) = (*settings).clone() {
                                current.summarize_enabled = !current.summarize_enabled;
                                settings.set(Some(current));
                            }
                        })
                    }} />
                    {"Enable summarization"}
                </label>
                <label class="field"><span class="muted">{"Summarize after N messages"}</span><input type="number" value={s.summarize_after_messages.to_string()} oninput={num_input(settings.clone(), "summarize_after_messages")} /></label>
                <label class="field"><span class="muted">{"Keep recent messages"}</span><input type="number" value={s.summarize_keep_recent.to_string()} oninput={num_input(settings.clone(), "summarize_keep_recent")} /></label>
            </div>
            <label style="display:flex;gap:0.5rem;align-items:center;margin-bottom:0.75rem;">
                <input type="checkbox" checked={s.facts_enabled} onclick={{
                    let settings = settings.clone();
                    Callback::from(move |_| {
                        if let Some(mut current) = (*settings).clone() {
                            current.facts_enabled = !current.facts_enabled;
                            settings.set(Some(current));
                        }
                    })
                }} />
                {"Enable KV facts in prompts"}
            </label>
            <button class="btn" disabled={*saving} onclick={{
                let settings = settings.clone();
                let saving = saving.clone();
                Callback::from(move |_| {
                    let Some(current) = (*settings).clone() else { return };
                    let settings = settings.clone();
                    let saving = saving.clone();
                    saving.set(true);
                    wasm_bindgen_futures::spawn_local(async move {
                        let update = SettingsUpdate {
                            inference_url: Some(current.inference_url),
                            model: Some(current.model),
                            temperature: Some(current.temperature),
                            top_p: Some(current.top_p),
                            max_tokens: Some(current.max_tokens),
                            system_prompt_prefix: Some(current.system_prompt_prefix),
                            system_prompt_suffix: Some(current.system_prompt_suffix),
                            summarize_enabled: Some(current.summarize_enabled),
                            summarize_after_messages: Some(current.summarize_after_messages),
                            summarize_keep_recent: Some(current.summarize_keep_recent),
                            facts_enabled: Some(current.facts_enabled),
                            max_context_messages: Some(current.max_context_messages),
                            max_concurrent_jobs: Some(current.max_concurrent_jobs),
                        };
                        if let Ok(updated) = api::update_settings(&update).await {
                            settings.set(Some(updated));
                        }
                        saving.set(false);
                    });
                })
            }}>{ if *saving { "Saving…" } else { "Save settings" } }</button>
        </div>
    }
}

fn num_input(
    settings: UseStateHandle<Option<Settings>>,
    field: &'static str,
) -> Callback<InputEvent> {
    Callback::from(move |e: InputEvent| {
        let input: HtmlInputElement = e.target_unchecked_into();
        if let Ok(v) = input.value().parse::<f64>() {
            if let Some(mut current) = (*settings).clone() {
                match field {
                    "temperature" => current.temperature = v,
                    "top_p" => current.top_p = v,
                    "max_tokens" => current.max_tokens = v as i64,
                    "max_concurrent_jobs" => current.max_concurrent_jobs = v as i64,
                    "summarize_after_messages" => current.summarize_after_messages = v as i64,
                    "summarize_keep_recent" => current.summarize_keep_recent = v as i64,
                    _ => {}
                }
                settings.set(Some(current));
            }
        }
    })
}

fn text_input(
    settings: UseStateHandle<Option<Settings>>,
    field: &'static str,
) -> Callback<InputEvent> {
    Callback::from(move |e: InputEvent| {
        let input: HtmlInputElement = e.target_unchecked_into();
        if let Some(mut current) = (*settings).clone() {
            match field {
                "system_prompt_prefix" => current.system_prompt_prefix = input.value(),
                "system_prompt_suffix" => current.system_prompt_suffix = input.value(),
                _ => {}
            }
            settings.set(Some(current));
        }
    })
}

#[wasm_bindgen::prelude::wasm_bindgen(start)]
pub fn main() {
    yew::Renderer::<App>::new().render();
}
