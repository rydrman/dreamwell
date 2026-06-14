use dreamwell_types::*;
use wasm_bindgen::JsCast;
use web_sys::{HtmlDocument, HtmlInputElement, HtmlSelectElement, HtmlTextAreaElement};
use yew::prelude::*;

use crate::api;
use crate::story_save::{
    draft_is_dirty, fail_auto_save, finish_auto_save, AutoSaveController, AutoSaveField,
    AutoSavePhase,
};

pub const MANUAL_MESSAGE_SOURCE: i64 = -1;
pub const MANUAL_STORY_SOURCE: i64 = -1;

#[derive(Clone, PartialEq)]
pub struct ScopeOption {
    pub value: String,
    pub label: String,
}

#[derive(Clone, PartialEq)]
pub struct VariableRowModel {
    pub id: Option<i64>,
    pub key: String,
    pub value: String,
    pub scope_label: String,
    pub scope_value: String,
    pub key_readonly: bool,
}

#[derive(Clone, PartialEq)]
pub struct VariableSavePayload {
    pub id: Option<i64>,
    pub key: String,
    pub value: String,
    pub scope_value: String,
}

pub struct VariableSaveAction {
    pub payload: VariableSavePayload,
    pub on_complete: Callback<Result<(), String>>,
}

#[derive(Clone, PartialEq)]
struct RowFields {
    key: String,
    value: String,
    scope_value: String,
}

impl RowFields {
    fn from_model(model: &VariableRowModel) -> Self {
        Self {
            key: model.key.clone(),
            value: model.value.clone(),
            scope_value: model.scope_value.clone(),
        }
    }

    fn snapshot(key: &str, value: &str, scope_value: &str) -> Self {
        Self {
            key: key.trim().to_string(),
            value: value.to_string(),
            scope_value: scope_value.to_string(),
        }
    }
}

fn text_input(state: UseStateHandle<String>) -> Callback<InputEvent> {
    Callback::from(move |e: InputEvent| {
        if let Some(target) = e.target() {
            if let Ok(input) = target.clone().dyn_into::<HtmlInputElement>() {
                state.set(input.value());
            } else if let Ok(textarea) = target.dyn_into::<HtmlTextAreaElement>() {
                state.set(textarea.value());
            }
        }
    })
}

fn copy_to_clipboard(text: &str) {
    if let Some(window) = web_sys::window() {
        if let Some(document) = window.document() {
            if let Ok(textarea) = document.create_element("textarea") {
                textarea.set_text_content(Some(text));
                if let Some(body) = document.body() {
                    let _ = body.append_child(&textarea);
                    let input: HtmlTextAreaElement = textarea.unchecked_into();
                    input.select();
                    if let Ok(html_document) = document.dyn_into::<HtmlDocument>() {
                        let _ = html_document.exec_command("copy");
                    }
                    let _ = body.remove_child(&input);
                }
            }
        }
    }
}

#[derive(Properties, PartialEq)]
pub struct VariableRowProps {
    pub model: VariableRowModel,
    #[prop_or_default]
    pub scope_options: Vec<ScopeOption>,
    #[prop_or(false)]
    pub scope_readonly: bool,
    #[prop_or(false)]
    pub fixed_scope: bool,
    #[prop_or(false)]
    pub compact: bool,
    pub on_save: Callback<VariableSaveAction>,
    pub on_delete: Callback<Option<i64>>,
    #[prop_or_default]
    pub on_cancel: Option<Callback<()>>,
}

#[function_component(VariableRow)]
pub fn variable_row(props: &VariableRowProps) -> Html {
    let key = use_state(|| props.model.key.clone());
    let value = use_state(|| props.model.value.clone());
    let scope_value = use_state(|| props.model.scope_value.clone());
    let last_saved = use_state(|| RowFields::from_model(&props.model));
    let save_phase = use_state(|| AutoSavePhase::Synced);
    let save_error = use_state(|| None::<String>);
    let save_controller = AutoSaveController::new(save_phase.clone(), save_error.clone());

    {
        let key = key.clone();
        let value = value.clone();
        let scope_value = scope_value.clone();
        let last_saved = last_saved.clone();
        let model = props.model.clone();
        use_effect_with(model, move |model| {
            let current = RowFields::snapshot(&key, &value, &scope_value);
            if !draft_is_dirty(&current, &*last_saved) {
                key.set(model.key.clone());
                value.set(model.value.clone());
                scope_value.set(model.scope_value.clone());
                last_saved.set(RowFields::from_model(model));
            }
            || ()
        });
    }

    let schedule_save = {
        let key = key.clone();
        let value = value.clone();
        let scope_value = scope_value.clone();
        let last_saved = last_saved.clone();
        let save_controller = save_controller.clone();
        let on_save = props.on_save.clone();
        let id = props.model.id;
        Callback::from(move |_| {
            let snapshot = RowFields::snapshot(&key, &value, &scope_value);
            if snapshot.key.is_empty() || !draft_is_dirty(&snapshot, &*last_saved) {
                return;
            }
            let key = key.clone();
            let value = value.clone();
            let scope_value = scope_value.clone();
            let last_saved = last_saved.clone();
            let save_controller = save_controller.clone();
            let on_save = on_save.clone();
            let controller_for_save = save_controller.clone();
            save_controller.schedule(move || {
                let payload = VariableSavePayload {
                    id,
                    key: snapshot.key.clone(),
                    value: snapshot.value.clone(),
                    scope_value: snapshot.scope_value.clone(),
                };
                let key = key.clone();
                let value = value.clone();
                let scope_value = scope_value.clone();
                let last_saved = last_saved.clone();
                let save_controller = controller_for_save.clone();
                on_save.emit(VariableSaveAction {
                    payload,
                    on_complete: Callback::from(move |result| match result {
                        Ok(()) => {
                            let current = RowFields::snapshot(&key, &value, &scope_value);
                            let _ = finish_auto_save(
                                &save_controller,
                                &current,
                                &snapshot,
                                &last_saved,
                            );
                        }
                        Err(message) => {
                            let current = RowFields::snapshot(&key, &value, &scope_value);
                            let _ = fail_auto_save(&save_controller, &current, &snapshot, message);
                        }
                    }),
                });
            });
        })
    };

    let scope_select = if props.fixed_scope {
        html! {
            <span class="variable-row-scope variable-row-scope--locked" title="Scope for this entry">
                { &props.model.scope_label }
            </span>
        }
    } else if props.scope_options.is_empty() {
        html! { <span class="muted variable-row-scope">{ &props.model.scope_label }</span> }
    } else {
        html! {
            <select
                class="variable-row-scope-select"
                value={(*scope_value).clone()}
                disabled={props.scope_readonly}
                onchange={{
                    let scope_value = scope_value.clone();
                    let schedule_save = schedule_save.clone();
                    Callback::from(move |e: Event| {
                        if let Some(select) = e.target_dyn_into::<HtmlSelectElement>() {
                            scope_value.set(select.value());
                            schedule_save.emit(());
                        }
                    })
                }}
            >
                { for props.scope_options.iter().map(|option| html! {
                    <option value={option.value.clone()}>{ &option.label }</option>
                }) }
            </select>
        }
    };

    let key_disabled = props.model.key_readonly || props.model.id.is_some();

    let key_input = {
        let key = key.clone();
        let schedule_save = schedule_save.clone();
        Callback::from(move |e: InputEvent| {
            text_input(key.clone()).emit(e);
            schedule_save.emit(());
        })
    };

    let value_input = {
        let value = value.clone();
        let schedule_save = schedule_save.clone();
        Callback::from(move |e: InputEvent| {
            text_input(value.clone()).emit(e);
            schedule_save.emit(());
        })
    };

    let actions = html! {
        <>
            <button
                class="btn secondary btn-compact variable-row-icon-btn"
                title="Copy value"
                onclick={{
                    let value = value.clone();
                    Callback::from(move |_| copy_to_clipboard(&value))
                }}
            >
                {"⧉"}
            </button>
            if let Some(id) = props.model.id {
                <button
                    class="btn secondary btn-compact variable-row-icon-btn text-danger"
                    title="Delete"
                    onclick={props.on_delete.reform(move |_| Some(id))}
                >
                    {"✕"}
                </button>
            }
        </>
    };

    if props.compact {
        let key_cell = if key_disabled {
            html! {
                <div class="variable-table-key-cell" role="cell">
                    <span class="variable-table-key-label">{ &*key }</span>
                </div>
            }
        } else {
            html! {
                <div class="variable-table-key-cell" role="cell">
                    <input
                        class="variable-table-key"
                        type="text"
                        placeholder="Key"
                        value={(*key).clone()}
                        oninput={key_input}
                    />
                </div>
            }
        };

        return html! {
            <div class="variable-table-row" role="row">
                { key_cell }
                <div class="variable-table-value-cell" role="cell">
                    <AutoSaveField phase={*save_phase} error={(*save_error).clone()}>
                        <input
                            class="variable-table-value"
                            type="text"
                            placeholder="Value"
                            value={(*value).clone()}
                            oninput={value_input}
                        />
                    </AutoSaveField>
                </div>
                <div class="variable-table-actions" role="cell">
                    { actions }
                </div>
            </div>
        };
    }

    html! {
        <div class="variable-row">
            <div class="variable-row-header">
                <input
                    class="variable-row-key"
                    type="text"
                    placeholder="Key"
                    value={(*key).clone()}
                    disabled={key_disabled}
                    oninput={key_input}
                />
                { scope_select }
                <div class="variable-row-actions">
                    { actions }
                </div>
            </div>
            <AutoSaveField phase={*save_phase} error={(*save_error).clone()}>
                <textarea
                    class="variable-row-value"
                    placeholder="Value"
                    rows="2"
                    value={(*value).clone()}
                    oninput={value_input}
                />
            </AutoSaveField>
            if props.on_cancel.is_some() && props.model.id.is_none() {
                <div class="variable-row-footer">
                    <button
                        class="btn secondary btn-compact"
                        onclick={props.on_cancel.clone().unwrap().reform(|_| ())}
                    >
                        {"Clear"}
                    </button>
                </div>
            }
        </div>
    }
}

pub fn chat_scope_options(messages: &[Message]) -> Vec<ScopeOption> {
    let mut options = vec![ScopeOption {
        value: MANUAL_MESSAGE_SOURCE.to_string(),
        label: "Session-wide (manual)".to_string(),
    }];
    for (index, message) in messages
        .iter()
        .filter(|message| !message.is_summary)
        .enumerate()
    {
        let preview = message
            .content
            .lines()
            .next()
            .unwrap_or("…")
            .chars()
            .take(40)
            .collect::<String>();
        options.push(ScopeOption {
            value: message.id.to_string(),
            label: format!("Message {} · {}", index + 1, preview),
        });
    }
    options
}

pub fn chat_scope_label(message_id: i64, messages: &[Message]) -> String {
    if message_id == MANUAL_MESSAGE_SOURCE {
        return "Session-wide".to_string();
    }
    messages
        .iter()
        .position(|message| message.id == message_id)
        .map(|index| format!("Message {}", index + 1))
        .unwrap_or_else(|| format!("Message {message_id}"))
}

pub fn story_scope_options(detail: &StoryDetail) -> Vec<ScopeOption> {
    let mut options = vec![ScopeOption {
        value: "manual".to_string(),
        label: "Story-wide (manual)".to_string(),
    }];
    for chapter in &detail.chapters {
        for beat in &chapter.beats {
            let chapter_num = chapter.sort_order + 1;
            let beat_num = beat.sort_order + 1;
            let beat_title = if beat.title.is_empty() {
                format!("Beat {beat_num}")
            } else {
                beat.title.clone()
            };
            options.push(ScopeOption {
                value: format!("{}:{}", chapter.sort_order, beat.sort_order),
                label: format!("Ch {chapter_num} · {beat_title}"),
            });
        }
    }
    options
}

pub fn story_scope_label(chapter_order: i64, beat_order: i64, detail: &StoryDetail) -> String {
    if chapter_order == MANUAL_STORY_SOURCE {
        return "Story-wide".to_string();
    }
    let chapter_num = chapter_order + 1;
    let beat_num = beat_order + 1;
    if let Some(chapter) = detail
        .chapters
        .iter()
        .find(|chapter| chapter.sort_order == chapter_order)
    {
        let beat_title = chapter
            .beats
            .iter()
            .find(|beat| beat.sort_order == beat_order)
            .map(|beat| {
                if beat.title.is_empty() {
                    format!("Beat {beat_num}")
                } else {
                    beat.title.clone()
                }
            })
            .unwrap_or_else(|| format!("Beat {beat_num}"));
        format!("Ch {chapter_num} · {beat_title}")
    } else {
        format!("Ch {chapter_num} · Beat {beat_num}")
    }
}

pub fn story_scope_value(chapter_order: i64, beat_order: i64) -> String {
    if chapter_order == MANUAL_STORY_SOURCE {
        "manual".to_string()
    } else {
        format!("{chapter_order}:{beat_order}")
    }
}

pub fn story_scope_from_value(value: &str) -> (i64, i64) {
    if value == "manual" {
        return (MANUAL_STORY_SOURCE, MANUAL_STORY_SOURCE);
    }
    let Some((chapter_order, beat_order)) = value.split_once(':') else {
        return (MANUAL_STORY_SOURCE, MANUAL_STORY_SOURCE);
    };
    (
        chapter_order.parse().unwrap_or(MANUAL_STORY_SOURCE),
        beat_order.parse().unwrap_or(MANUAL_STORY_SOURCE),
    )
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
pub struct VariableUpdatesAuditProps {
    pub updates: Vec<MessageVariableUpdate>,
}

#[function_component(VariableUpdatesAudit)]
pub fn variable_updates_audit(props: &VariableUpdatesAuditProps) -> Html {
    if props.updates.is_empty() {
        return html! {};
    }

    html! {
        <>
            <div class="message-variable-updates-subsection-title">{"Model changes"}</div>
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
        </>
    }
}

#[derive(Properties, PartialEq)]
pub struct CollapsibleVariablesSectionProps {
    pub title: String,
    #[prop_or(false)]
    pub default_expanded: bool,
    pub children: Children,
}

#[function_component(CollapsibleVariablesSection)]
pub fn collapsible_variables_section(props: &CollapsibleVariablesSectionProps) -> Html {
    let expanded = use_state(|| props.default_expanded);

    let toggle = {
        let expanded = expanded.clone();
        Callback::from(move |_| expanded.set(!*expanded))
    };

    html! {
        <div class="message-variable-updates">
            <button type="button" class="message-variable-updates-toggle" onclick={toggle}>
                <span class="message-variable-updates-label">{ &props.title }</span>
                <span class="message-variable-updates-chevron" aria-hidden="true">
                    { if *expanded { "▾" } else { "▸" } }
                </span>
            </button>
            if *expanded {
                <div class="message-variable-updates-body">
                    { for props.children.iter() }
                </div>
            }
        </div>
    }
}

pub fn chat_variable_row(
    variable: &ChatVariable,
    messages: &[Message],
    key_readonly: bool,
) -> VariableRowModel {
    VariableRowModel {
        id: Some(variable.id),
        key: variable.key.clone(),
        value: variable.value.clone(),
        scope_label: chat_scope_label(variable.source_message_id, messages),
        scope_value: variable.source_message_id.to_string(),
        key_readonly,
    }
}

pub fn story_variable_row(
    variable: &StoryVariable,
    detail: &StoryDetail,
    key_readonly: bool,
) -> VariableRowModel {
    let scope_value = story_scope_value(variable.source_chapter_order, variable.source_beat_order);
    VariableRowModel {
        id: Some(variable.id),
        key: variable.key.clone(),
        value: variable.value.clone(),
        scope_label: story_scope_label(
            variable.source_chapter_order,
            variable.source_beat_order,
            detail,
        ),
        scope_value,
        key_readonly,
    }
}

pub fn make_chat_variable_handlers(
    chat_id: i64,
    variables: UseStateHandle<Vec<ChatVariable>>,
    on_changed: Option<Callback<()>>,
) -> (Callback<VariableSaveAction>, Callback<Option<i64>>) {
    let on_save = {
        let variables = variables.clone();
        Callback::from(move |action: VariableSaveAction| {
            let payload = action.payload;
            let on_complete = action.on_complete;
            let variables = variables.clone();
            let source_message_id = payload
                .scope_value
                .parse::<i64>()
                .unwrap_or(MANUAL_MESSAGE_SOURCE);
            wasm_bindgen_futures::spawn_local(async move {
                match api::upsert_variable(
                    chat_id,
                    &ChatVariableUpdate {
                        key: payload.key,
                        value: payload.value,
                        source_message_id: Some(source_message_id),
                    },
                )
                .await
                {
                    Ok(saved) => {
                        variables.set(patch_chat_variable(&variables, saved));
                        on_complete.emit(Ok(()));
                    }
                    Err(err) => on_complete.emit(Err(err)),
                }
            });
        })
    };

    let on_delete = {
        let variables = variables.clone();
        Callback::from(move |variable_id: Option<i64>| {
            let Some(variable_id) = variable_id else {
                return;
            };
            let variables = variables.clone();
            let on_changed = on_changed.clone();
            wasm_bindgen_futures::spawn_local(async move {
                match api::delete_variable(chat_id, variable_id).await {
                    Ok(()) => {
                        variables.set(remove_chat_variable(&variables, variable_id));
                        if let Some(on_changed) = on_changed {
                            on_changed.emit(());
                        }
                    }
                    Err(err) => {
                        if let Some(window) = web_sys::window() {
                            let _ = window
                                .alert_with_message(&format!("Could not delete variable: {err}"));
                        }
                    }
                }
            });
        })
    };

    (on_save, on_delete)
}

fn patch_chat_variable(list: &[ChatVariable], saved: ChatVariable) -> Vec<ChatVariable> {
    let mut next: Vec<ChatVariable> = list
        .iter()
        .filter(|variable| variable.id != saved.id)
        .cloned()
        .collect();
    next.push(saved);
    next.sort_by(|left, right| {
        (left.source_message_id, &left.key).cmp(&(right.source_message_id, &right.key))
    });
    next
}

fn remove_chat_variable(list: &[ChatVariable], id: i64) -> Vec<ChatVariable> {
    list.iter()
        .filter(|variable| variable.id != id)
        .cloned()
        .collect()
}

fn patch_story_variable(list: &[StoryVariable], saved: StoryVariable) -> Vec<StoryVariable> {
    let mut next: Vec<StoryVariable> = list
        .iter()
        .filter(|variable| variable.id != saved.id)
        .cloned()
        .collect();
    next.push(saved);
    next.sort_by(|left, right| {
        (left.source_chapter_order, left.source_beat_order, &left.key).cmp(&(
            right.source_chapter_order,
            right.source_beat_order,
            &right.key,
        ))
    });
    next
}

fn remove_story_variable(list: &[StoryVariable], id: i64) -> Vec<StoryVariable> {
    list.iter()
        .filter(|variable| variable.id != id)
        .cloned()
        .collect()
}

pub fn make_story_variable_handlers(
    story_id: i64,
    variables: UseStateHandle<Vec<StoryVariable>>,
    on_detail: Option<Callback<StoryDetail>>,
) -> (Callback<VariableSaveAction>, Callback<Option<i64>>) {
    let on_save = {
        let variables = variables.clone();
        Callback::from(move |action: VariableSaveAction| {
            let payload = action.payload;
            let on_complete = action.on_complete;
            let variables = variables.clone();
            let (chapter_order, beat_order) = story_scope_from_value(&payload.scope_value);
            let old_scope = payload.id.and_then(|id| {
                variables
                    .iter()
                    .find(|variable| variable.id == id)
                    .map(|variable| {
                        story_scope_value(variable.source_chapter_order, variable.source_beat_order)
                    })
            });
            wasm_bindgen_futures::spawn_local(async move {
                if let Some(old_id) = payload.id {
                    if old_scope.as_deref() != Some(payload.scope_value.as_str()) {
                        let _ = api::delete_story_variable(story_id, old_id).await;
                        variables.set(remove_story_variable(&variables, old_id));
                    }
                }
                match api::upsert_story_variable(
                    story_id,
                    &StoryVariableUpdate {
                        key: payload.key,
                        value: payload.value,
                        source_chapter_order: Some(chapter_order),
                        source_beat_order: Some(beat_order),
                    },
                )
                .await
                {
                    Ok(saved) => {
                        variables.set(patch_story_variable(&variables, saved));
                        on_complete.emit(Ok(()));
                    }
                    Err(err) => on_complete.emit(Err(err)),
                }
            });
        })
    };

    let on_delete = {
        let variables = variables.clone();
        Callback::from(move |variable_id: Option<i64>| {
            let Some(variable_id) = variable_id else {
                return;
            };
            let variables = variables.clone();
            let on_detail = on_detail.clone();
            wasm_bindgen_futures::spawn_local(async move {
                match api::delete_story_variable(story_id, variable_id).await {
                    Ok(()) => {
                        variables.set(remove_story_variable(&variables, variable_id));
                        if let Some(on_detail) = on_detail {
                            if let Ok(detail) = api::get_story(story_id).await {
                                on_detail.emit(detail);
                            }
                        }
                    }
                    Err(err) => {
                        if let Some(window) = web_sys::window() {
                            let _ = window
                                .alert_with_message(&format!("Could not delete variable: {err}"));
                        }
                    }
                }
            });
        })
    };

    (on_save, on_delete)
}

#[derive(Properties, PartialEq)]
pub struct InlineChatVariablesProps {
    pub chat_id: i64,
    pub message_id: i64,
    pub scope_label: String,
    pub variable_updates: Vec<MessageVariableUpdate>,
    pub on_changed: Callback<()>,
}

#[function_component(InlineChatVariables)]
pub fn inline_chat_variables(props: &InlineChatVariablesProps) -> Html {
    let variables = use_state(Vec::<ChatVariable>::new);
    let refresh = (
        props.chat_id,
        props.message_id,
        props.variable_updates.len(),
    );

    {
        let variables = variables.clone();
        use_effect_with(refresh, move |&(chat_id, _, _)| {
            let variables = variables.clone();
            wasm_bindgen_futures::spawn_local(async move {
                if let Ok(list) = api::get_variables(chat_id).await {
                    variables.set(list);
                }
            });
            || ()
        });
    }

    let scoped: Vec<VariableRowModel> = variables
        .iter()
        .filter(|variable| variable.source_message_id == props.message_id)
        .map(|variable| VariableRowModel {
            id: Some(variable.id),
            key: variable.key.clone(),
            value: variable.value.clone(),
            scope_label: props.scope_label.clone(),
            scope_value: props.message_id.to_string(),
            key_readonly: true,
        })
        .collect();

    let (on_save, on_delete) =
        make_chat_variable_handlers(props.chat_id, variables, Some(props.on_changed.clone()));

    let count = scoped.len();
    let title = format!("Variables ({count})");

    html! {
        <CollapsibleVariablesSection title={title} default_expanded={true}>
            <VariableUpdatesAudit updates={props.variable_updates.clone()} />
            <VariableList
                rows={scoped}
                new_scope_value={props.message_id.to_string()}
                fixed_scope_label={Some(props.scope_label.clone())}
                on_save={on_save}
                on_delete={on_delete}
                compact={true}
            />
        </CollapsibleVariablesSection>
    }
}

#[derive(Properties, PartialEq)]
pub struct InlineStoryVariablesProps {
    pub story_id: i64,
    pub chapter_order: i64,
    pub beat_order: i64,
    pub scope_label: String,
    pub variable_updates: Vec<BeatVariableUpdate>,
    pub on_detail: Callback<StoryDetail>,
}

#[function_component(InlineStoryVariables)]
pub fn inline_story_variables(props: &InlineStoryVariablesProps) -> Html {
    let variables = use_state(Vec::<StoryVariable>::new);
    let scope_value = story_scope_value(props.chapter_order, props.beat_order);
    let refresh = (
        props.story_id,
        props.chapter_order,
        props.beat_order,
        props.variable_updates.len(),
    );

    {
        let variables = variables.clone();
        use_effect_with(refresh, move |&(story_id, _, _, _)| {
            let variables = variables.clone();
            wasm_bindgen_futures::spawn_local(async move {
                if let Ok(list) = api::get_story_variables(story_id).await {
                    variables.set(list);
                }
            });
            || ()
        });
    }

    let scoped: Vec<VariableRowModel> = variables
        .iter()
        .filter(|variable| {
            variable.source_chapter_order == props.chapter_order
                && variable.source_beat_order == props.beat_order
        })
        .map(|variable| VariableRowModel {
            id: Some(variable.id),
            key: variable.key.clone(),
            value: variable.value.clone(),
            scope_label: props.scope_label.clone(),
            scope_value: scope_value.clone(),
            key_readonly: true,
        })
        .collect();

    let (on_save, on_delete) =
        make_story_variable_handlers(props.story_id, variables, Some(props.on_detail.clone()));

    let count = scoped.len();
    let title = format!("Variables ({count})");

    html! {
        <CollapsibleVariablesSection title={title} default_expanded={true}>
            <VariableUpdatesAudit updates={props.variable_updates.clone()} />
            <VariableList
                rows={scoped}
                new_scope_value={scope_value}
                fixed_scope_label={Some(props.scope_label.clone())}
                on_save={on_save}
                on_delete={on_delete}
                compact={true}
            />
        </CollapsibleVariablesSection>
    }
}

#[derive(Properties, PartialEq)]
pub struct VariableListProps {
    pub rows: Vec<VariableRowModel>,
    #[prop_or_default]
    pub scope_options: Vec<ScopeOption>,
    pub on_save: Callback<VariableSaveAction>,
    pub on_delete: Callback<Option<i64>>,
    #[prop_or_default]
    pub new_scope_value: String,
    #[prop_or_default]
    pub description: String,
    #[prop_or_default]
    pub fixed_scope_label: Option<String>,
    #[prop_or(false)]
    pub compact: bool,
}

#[function_component(VariableList)]
pub fn variable_list(props: &VariableListProps) -> Html {
    let new_key = use_state(String::new);
    let new_value = use_state(String::new);
    let new_scope = use_state(|| props.new_scope_value.clone());
    let new_save_phase = use_state(|| AutoSavePhase::Synced);
    let new_save_error = use_state(|| None::<String>);
    let new_save_controller =
        AutoSaveController::new(new_save_phase.clone(), new_save_error.clone());

    {
        let new_scope = new_scope.clone();
        let scope = props.new_scope_value.clone();
        use_effect_with(scope, move |scope| {
            new_scope.set(scope.clone());
            || ()
        });
    }

    let clear_new = {
        let new_key = new_key.clone();
        let new_value = new_value.clone();
        let new_scope = new_scope.clone();
        let new_save_controller = new_save_controller.clone();
        let default_scope = props.new_scope_value.clone();
        Callback::from(move |_| {
            new_key.set(String::new());
            new_value.set(String::new());
            new_scope.set(default_scope.clone());
            new_save_controller.mark_saved();
        })
    };

    let fixed_scope = props.fixed_scope_label.is_some();

    if props.compact {
        return html! {
            <div class="variable-table" role="table">
                <div class="variable-table-header" role="row">
                    <div class="variable-table-header-cell" role="columnheader">{"Name"}</div>
                    <div class="variable-table-header-cell" role="columnheader">{"Value"}</div>
                    <div class="variable-table-header-cell variable-table-header-actions" role="columnheader" aria-hidden="true"></div>
                </div>
                { for props.rows.iter().map(|row| {
                    let model = row.clone();
                    let row_key = model
                        .id
                        .map(|id| id.to_string())
                        .unwrap_or_else(|| format!("new-{}", model.key));
                    html! {
                        <VariableRow
                            key={row_key}
                            model={model}
                            scope_options={props.scope_options.clone()}
                            scope_readonly={fixed_scope}
                            fixed_scope={fixed_scope}
                            compact={true}
                            on_save={props.on_save.clone()}
                            on_delete={props.on_delete.clone()}
                        />
                    }
                }) }
                <div class="variable-table-row variable-table-row--new" role="row">
                    <div class="variable-table-key-cell" role="cell">
                        <input
                            class="variable-table-key"
                            type="text"
                            placeholder="Key"
                            value={(*new_key).clone()}
                            oninput={text_input(new_key.clone())}
                        />
                    </div>
                    <div class="variable-table-value-cell" role="cell">
                        <AutoSaveField phase={*new_save_phase} error={(*new_save_error).clone()}>
                            <input
                                class="variable-table-value"
                                type="text"
                                placeholder="Value"
                                value={(*new_value).clone()}
                                oninput={text_input(new_value.clone())}
                            />
                        </AutoSaveField>
                    </div>
                    <div class="variable-table-actions" role="cell">
                        <button
                            class="btn btn-compact"
                            disabled={new_key.trim().is_empty() || *new_save_phase == AutoSavePhase::Saving}
                            onclick={{
                                let on_save = props.on_save.clone();
                                let new_key = new_key.clone();
                                let new_value = new_value.clone();
                                let new_scope = new_scope.clone();
                                let clear_new = clear_new.clone();
                                let new_save_controller = new_save_controller.clone();
                                let new_save_phase = new_save_phase.clone();
                                Callback::from(move |_| {
                                    if (*new_key).trim().is_empty() {
                                        return;
                                    }
                                    let payload = VariableSavePayload {
                                        id: None,
                                        key: (*new_key).trim().to_string(),
                                        value: (*new_value).clone(),
                                        scope_value: (*new_scope).clone(),
                                    };
                                    let clear_new = clear_new.clone();
                                    let new_save_controller = new_save_controller.clone();
                                    let new_save_phase = new_save_phase.clone();
                                    new_save_phase.set(AutoSavePhase::Saving);
                                    on_save.emit(VariableSaveAction {
                                        payload,
                                        on_complete: Callback::from(move |result| match result {
                                            Ok(()) => {
                                                new_save_controller.mark_saved();
                                                clear_new.emit(());
                                            }
                                            Err(message) => {
                                                new_save_controller.mark_failed(message);
                                            }
                                        }),
                                    });
                                })
                            }}
                        >
                            { if *new_save_phase == AutoSavePhase::Saving { "…" } else { "Add" } }
                        </button>
                    </div>
                </div>
                if let Some(label) = props.fixed_scope_label.as_ref() {
                    <div class="variable-table-scope muted">{ format!("Scope: {label}") }</div>
                }
            </div>
        };
    }

    html! {
        <div class="variable-list">
            if !props.description.is_empty() {
                <p class="muted">{ &props.description }</p>
            }
            { for props.rows.iter().map(|row| {
                let model = row.clone();
                let row_key = model
                    .id
                    .map(|id| id.to_string())
                    .unwrap_or_else(|| format!("new-{}", model.key));
                html! {
                    <VariableRow
                        key={row_key}
                        model={model}
                        scope_options={props.scope_options.clone()}
                        scope_readonly={fixed_scope}
                        fixed_scope={fixed_scope}
                        compact={false}
                        on_save={props.on_save.clone()}
                        on_delete={props.on_delete.clone()}
                    />
                }
            }) }
            <div class="variable-row variable-row--new">
                <div class="variable-row-header">
                    <input
                        class="variable-row-key"
                        type="text"
                        placeholder="Key"
                        value={(*new_key).clone()}
                        oninput={text_input(new_key.clone())}
                    />
                    if let Some(label) = props.fixed_scope_label.as_ref() {
                        <span class="variable-row-scope variable-row-scope--locked">{ label }</span>
                    } else if props.scope_options.is_empty() {
                        <span class="muted variable-row-scope">{"New"}</span>
                    } else {
                        <select
                            class="variable-row-scope-select"
                            value={(*new_scope).clone()}
                            onchange={{
                                let new_scope = new_scope.clone();
                                Callback::from(move |e: Event| {
                                    if let Some(select) = e.target_dyn_into::<HtmlSelectElement>() {
                                        new_scope.set(select.value());
                                    }
                                })
                            }}
                        >
                            { for props.scope_options.iter().map(|option| html! {
                                <option value={option.value.clone()}>{ &option.label }</option>
                            }) }
                        </select>
                    }
                </div>
                <AutoSaveField phase={*new_save_phase} error={(*new_save_error).clone()}>
                    <textarea
                        class="variable-row-value"
                        placeholder="Value"
                        rows="2"
                        value={(*new_value).clone()}
                        oninput={text_input(new_value.clone())}
                    />
                </AutoSaveField>
                <div class="variable-row-footer">
                    <button
                        class="btn"
                        disabled={new_key.trim().is_empty() || *new_save_phase == AutoSavePhase::Saving}
                        onclick={{
                            let on_save = props.on_save.clone();
                            let new_key = new_key.clone();
                            let new_value = new_value.clone();
                            let new_scope = new_scope.clone();
                            let clear_new = clear_new.clone();
                            let new_save_controller = new_save_controller.clone();
                            let new_save_phase = new_save_phase.clone();
                            Callback::from(move |_| {
                                if (*new_key).trim().is_empty() {
                                    return;
                                }
                                let payload = VariableSavePayload {
                                    id: None,
                                    key: (*new_key).trim().to_string(),
                                    value: (*new_value).clone(),
                                    scope_value: (*new_scope).clone(),
                                };
                                let clear_new = clear_new.clone();
                                let new_save_controller = new_save_controller.clone();
                                let new_save_phase = new_save_phase.clone();
                                new_save_phase.set(AutoSavePhase::Saving);
                                on_save.emit(VariableSaveAction {
                                    payload,
                                    on_complete: Callback::from(move |result| match result {
                                        Ok(()) => {
                                            new_save_controller.mark_saved();
                                            clear_new.emit(());
                                        }
                                        Err(message) => {
                                            new_save_controller.mark_failed(message);
                                        }
                                    }),
                                });
                            })
                        }}
                    >
                        { if *new_save_phase == AutoSavePhase::Saving { "Adding…" } else { "Add variable" } }
                    </button>
                </div>
            </div>
        </div>
    }
}
