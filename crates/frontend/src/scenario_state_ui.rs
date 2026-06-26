use dreamwell_types::{normalize_target, CharacterStateDef, StateKind, TrackedVarDef};
use web_sys::{HtmlInputElement, HtmlTextAreaElement};
use yew::prelude::*;

use crate::auto_grow::{fit_textarea, fit_textarea_when_ready};
use crate::use_fit_textarea;

pub fn optional_i64_input(
    label: &str,
    value: Option<i64>,
    on_change: Callback<Option<i64>>,
) -> Html {
    let display = value.map(|v| v.to_string()).unwrap_or_default();
    html! {
        <label class="field field-inline">
            <span class="muted">{ label }</span>
            <input
                type="number"
                class="input input-compact"
                value={display}
                oninput={Callback::from(move |e: InputEvent| {
                    let input: HtmlInputElement = e.target_unchecked_into();
                    let raw = input.value();
                    let parsed = if raw.trim().is_empty() {
                        None
                    } else {
                        raw.parse::<i64>().ok()
                    };
                    on_change.emit(parsed);
                })}
            />
        </label>
    }
}

fn start_edit_on_click(editing: UseStateHandle<bool>) -> Callback<MouseEvent> {
    Callback::from(move |e: MouseEvent| {
        e.stop_propagation();
        editing.set(true);
    })
}

fn start_edit_on_keydown(editing: UseStateHandle<bool>) -> Callback<KeyboardEvent> {
    Callback::from(move |e: KeyboardEvent| {
        if e.key() == "Enter" || e.key() == " " {
            e.prevent_default();
            editing.set(true);
        }
    })
}

#[derive(Properties, PartialEq)]
pub struct InlineTextFieldProps {
    pub label: String,
    pub value: String,
    #[prop_or("")]
    pub placeholder: &'static str,
    pub on_change: Callback<String>,
}

#[function_component(InlineTextField)]
pub fn inline_text_field(props: &InlineTextFieldProps) -> Html {
    let editing = use_state(|| false);
    let input_ref = use_node_ref();
    let value = props.value.clone();

    {
        let editing = editing.clone();
        let value = value.clone();
        use_effect_with(value, move |_| {
            editing.set(false);
            || ()
        });
    }

    {
        let input_ref = input_ref.clone();
        let editing = *editing;
        use_effect_with(editing, move |editing| {
            if *editing {
                if let Some(input) = input_ref.cast::<HtmlInputElement>() {
                    let _ = input.focus();
                    input.select();
                }
            }
            || ()
        });
    }

    let on_change = props.on_change.clone();
    let placeholder = props.placeholder;
    let label = props.label.clone();
    let display = if value.is_empty() {
        placeholder.to_string()
    } else {
        value.clone()
    };
    let display_empty = value.is_empty();

    html! {
        <label class="field scenario-inline-field">
            <span class="muted">{ label }</span>
            if *editing {
                <input
                    ref={input_ref}
                    type="text"
                    class="input scenario-inline-field__control"
                    placeholder={placeholder}
                    value={value}
                    onclick={Callback::from(|e: MouseEvent| e.stop_propagation())}
                    oninput={{
                        let on_change = on_change.clone();
                        Callback::from(move |e: InputEvent| {
                            let input: HtmlInputElement = e.target_unchecked_into();
                            on_change.emit(input.value());
                        })
                    }}
                    onkeydown={{
                        let editing = editing.clone();
                        Callback::from(move |e: KeyboardEvent| {
                            if e.key() == "Escape" {
                                editing.set(false);
                            }
                        })
                    }}
                    onblur={{
                        let editing = editing.clone();
                        Callback::from(move |_| editing.set(false))
                    }}
                />
            } else {
                <div
                    class={classes!(
                        "scenario-inline-display",
                        display_empty.then_some("scenario-inline-display--empty"),
                    )}
                    title="Click to edit"
                    tabindex="0"
                    role="button"
                    onclick={start_edit_on_click(editing.clone())}
                    onkeydown={start_edit_on_keydown(editing.clone())}
                >
                    { display }
                </div>
            }
        </label>
    }
}

#[derive(Properties, PartialEq)]
pub struct InlineTextareaFieldProps {
    pub label: String,
    pub value: String,
    #[prop_or("")]
    pub placeholder: &'static str,
    pub on_change: Callback<String>,
    #[prop_or(false)]
    pub secondary: bool,
}

#[function_component(InlineTextareaField)]
pub fn inline_textarea_field(props: &InlineTextareaFieldProps) -> Html {
    let editing = use_state(|| false);
    let textarea_ref = use_node_ref();
    let value = props.value.clone();

    use_fit_textarea!(&textarea_ref, value.clone());

    {
        let editing = editing.clone();
        let value = value.clone();
        use_effect_with(value, move |_| {
            editing.set(false);
            || ()
        });
    }

    {
        let textarea_ref = textarea_ref.clone();
        let editing = *editing;
        use_effect_with(editing, move |editing| {
            if *editing {
                if let Some(textarea) = textarea_ref.cast::<HtmlTextAreaElement>() {
                    let _ = textarea.focus();
                    fit_textarea(&textarea);
                    fit_textarea_when_ready(textarea);
                }
            }
            || ()
        });
    }

    let on_change = props.on_change.clone();
    let placeholder = props.placeholder;
    let label = props.label.clone();
    let display = if value.is_empty() {
        placeholder.to_string()
    } else {
        value.clone()
    };
    let display_empty = value.is_empty();

    html! {
        <label class="field scenario-inline-field">
            <span class="muted">{ label }</span>
            if *editing {
                <textarea
                    ref={textarea_ref}
                    class={classes!(
                        "input",
                        "scenario-inline-field__control",
                        props.secondary.then_some("scenario-inline-field__control--secondary"),
                    )}
                    rows="1"
                    placeholder={placeholder}
                    value={value}
                    onclick={Callback::from(|e: MouseEvent| e.stop_propagation())}
                    oninput={{
                        let on_change = on_change.clone();
                        Callback::from(move |e: InputEvent| {
                            let input: HtmlTextAreaElement = e.target_unchecked_into();
                            fit_textarea(&input);
                            on_change.emit(input.value());
                        })
                    }}
                    onkeydown={{
                        let editing = editing.clone();
                        Callback::from(move |e: KeyboardEvent| {
                            if e.key() == "Escape" {
                                editing.set(false);
                            }
                        })
                    }}
                    onblur={{
                        let editing = editing.clone();
                        Callback::from(move |_| editing.set(false))
                    }}
                />
            } else {
                <div
                    class={classes!(
                        "scenario-inline-display",
                        display_empty.then_some("scenario-inline-display--empty"),
                        props.secondary.then_some("scenario-inline-display--secondary"),
                    )}
                    title="Click to edit"
                    tabindex="0"
                    role="button"
                    onclick={start_edit_on_click(editing.clone())}
                    onkeydown={start_edit_on_keydown(editing.clone())}
                >
                    { display }
                </div>
            }
        </label>
    }
}

pub fn text_input(label: &str, value: &str, on_change: Callback<String>) -> Html {
    html! {
        <InlineTextField label={label.to_string()} value={value.to_string()} on_change={on_change} />
    }
}

pub fn textarea_input(label: &str, value: &str, on_change: Callback<String>) -> Html {
    html! {
        <InlineTextareaField label={label.to_string()} value={value.to_string()} on_change={on_change} />
    }
}

/// Scenario editor row — `kind: None` is UI-only until the user picks a type and saves.
#[derive(Clone, PartialEq, Default)]
pub struct EditableCharacterStateDef {
    pub key: String,
    pub kind: Option<StateKind>,
    pub description: String,
    pub initial_value: String,
    pub initial_num: Option<i64>,
    pub initial_max: Option<i64>,
    pub visibility: String,
    pub update_hints: String,
}

/// World/schema editor row — same unset-type behavior as character state.
/// `target` is a runtime target ("world" or "pc"); blank is treated as "world".
#[derive(Clone, PartialEq)]
pub struct EditableTrackedVarDef {
    pub key: String,
    pub kind: Option<StateKind>,
    pub target: String,
    pub description: String,
    pub initial_value: String,
    pub initial_num: Option<i64>,
    pub initial_max: Option<i64>,
    pub visibility: String,
    pub update_hints: String,
}

impl Default for EditableTrackedVarDef {
    fn default() -> Self {
        Self {
            key: String::new(),
            kind: None,
            target: "world".to_string(),
            description: String::new(),
            initial_value: String::new(),
            initial_num: None,
            initial_max: None,
            visibility: String::new(),
            update_hints: String::new(),
        }
    }
}

impl EditableCharacterStateDef {
    pub fn from_saved(def: &CharacterStateDef) -> Self {
        Self {
            key: def.key.clone(),
            kind: Some(def.kind),
            description: def.description.clone(),
            initial_value: def.initial_value.clone(),
            initial_num: def.initial_num,
            initial_max: def.initial_max,
            visibility: def.visibility.clone(),
            update_hints: def.update_hints.clone(),
        }
    }

    pub fn from_saved_vec(defs: &[CharacterStateDef]) -> Vec<Self> {
        defs.iter().map(Self::from_saved).collect()
    }

    pub fn to_saved(&self, label: &str) -> Result<Option<CharacterStateDef>, String> {
        if self.key.trim().is_empty() && self.kind.is_none() {
            return Ok(None);
        }
        let kind = self
            .kind
            .ok_or_else(|| format!("{label}: choose a type for each state entry"))?;
        if self.key.trim().is_empty() {
            return Err(format!("{label}: each state entry needs a key"));
        }
        Ok(Some(CharacterStateDef {
            key: self.key.trim().to_string(),
            kind,
            description: self.description.clone(),
            initial_value: self.initial_value.clone(),
            initial_num: self.initial_num,
            initial_max: self.initial_max,
            visibility: self.visibility.clone(),
            update_hints: self.update_hints.clone(),
        }))
    }

    pub fn set_kind(&mut self, kind: StateKind) {
        if self.kind == Some(kind) {
            return;
        }
        self.initial_value.clear();
        self.initial_num = None;
        self.initial_max = None;
        self.kind = Some(kind);
        match kind {
            StateKind::Clock if self.initial_max.is_none() => self.initial_max = Some(4),
            StateKind::Resource if self.initial_max.is_none() => self.initial_max = Some(5),
            _ => {}
        }
    }

    pub fn apply_update(&mut self, update: ScenarioStateFieldUpdate) {
        match update {
            ScenarioStateFieldUpdate::Key(value) => self.key = value,
            ScenarioStateFieldUpdate::Kind(kind) => self.set_kind(kind),
            ScenarioStateFieldUpdate::Target(_) => {}
            ScenarioStateFieldUpdate::Description(value) => self.description = value,
            ScenarioStateFieldUpdate::InitialValue(value) => self.initial_value = value,
            ScenarioStateFieldUpdate::InitialNum(value) => self.initial_num = value,
            ScenarioStateFieldUpdate::InitialMax(value) => self.initial_max = value,
            ScenarioStateFieldUpdate::Visibility(value) => self.visibility = value,
            ScenarioStateFieldUpdate::UpdateHints(value) => self.update_hints = value,
        }
    }

    pub fn to_view(&self) -> ScenarioStateDefView {
        ScenarioStateDefView {
            key: self.key.clone(),
            kind: self.kind,
            description: self.description.clone(),
            initial_value: self.initial_value.clone(),
            initial_num: self.initial_num,
            initial_max: self.initial_max,
            visibility: self.visibility.clone(),
            update_hints: self.update_hints.clone(),
            target: None,
        }
    }
}

impl EditableTrackedVarDef {
    pub fn from_saved(def: &TrackedVarDef) -> Self {
        Self {
            key: def.key.clone(),
            kind: Some(def.kind),
            target: normalize_target(&def.target),
            description: def.description.clone(),
            initial_value: def.initial_value.clone(),
            initial_num: def.initial_num,
            initial_max: def.initial_max,
            visibility: def.visibility.clone(),
            update_hints: def.update_hints.clone(),
        }
    }

    pub fn from_saved_vec(defs: &[TrackedVarDef]) -> Vec<Self> {
        defs.iter().map(Self::from_saved).collect()
    }

    pub fn to_saved(&self, label: &str) -> Result<Option<TrackedVarDef>, String> {
        if self.key.trim().is_empty() && self.kind.is_none() {
            return Ok(None);
        }
        let kind = self
            .kind
            .ok_or_else(|| format!("{label}: choose a type for each state entry"))?;
        if self.key.trim().is_empty() {
            return Err(format!("{label}: each state entry needs a key"));
        }
        Ok(Some(TrackedVarDef {
            key: self.key.trim().to_string(),
            kind,
            target: normalize_target(&self.target),
            description: self.description.clone(),
            initial_value: self.initial_value.clone(),
            initial_num: self.initial_num,
            initial_max: self.initial_max,
            visibility: self.visibility.clone(),
            update_hints: self.update_hints.clone(),
        }))
    }

    pub fn set_kind(&mut self, kind: StateKind) {
        if self.kind == Some(kind) {
            return;
        }
        self.initial_value.clear();
        self.initial_num = None;
        self.initial_max = None;
        self.kind = Some(kind);
        match kind {
            StateKind::Clock if self.initial_max.is_none() => self.initial_max = Some(4),
            StateKind::Resource if self.initial_max.is_none() => self.initial_max = Some(5),
            _ => {}
        }
    }

    pub fn apply_update(&mut self, update: ScenarioStateFieldUpdate) {
        match update {
            ScenarioStateFieldUpdate::Key(value) => self.key = value,
            ScenarioStateFieldUpdate::Kind(kind) => self.set_kind(kind),
            ScenarioStateFieldUpdate::Target(value) => self.target = value,
            ScenarioStateFieldUpdate::Description(value) => self.description = value,
            ScenarioStateFieldUpdate::InitialValue(value) => self.initial_value = value,
            ScenarioStateFieldUpdate::InitialNum(value) => self.initial_num = value,
            ScenarioStateFieldUpdate::InitialMax(value) => self.initial_max = value,
            ScenarioStateFieldUpdate::Visibility(value) => self.visibility = value,
            ScenarioStateFieldUpdate::UpdateHints(value) => self.update_hints = value,
        }
    }

    pub fn to_view(&self) -> ScenarioStateDefView {
        ScenarioStateDefView {
            key: self.key.clone(),
            kind: self.kind,
            description: self.description.clone(),
            initial_value: self.initial_value.clone(),
            initial_num: self.initial_num,
            initial_max: self.initial_max,
            visibility: self.visibility.clone(),
            update_hints: self.update_hints.clone(),
            target: Some(normalize_target(&self.target)),
        }
    }
}

pub fn editable_character_state_to_saved(
    defs: &[EditableCharacterStateDef],
    label: &str,
) -> Result<Vec<CharacterStateDef>, String> {
    defs.iter()
        .map(|def| def.to_saved(label))
        .collect::<Result<Vec<_>, _>>()
        .map(|rows| rows.into_iter().flatten().collect())
}

pub fn editable_tracked_var_to_saved(
    defs: &[EditableTrackedVarDef],
    label: &str,
) -> Result<Vec<TrackedVarDef>, String> {
    defs.iter()
        .map(|def| def.to_saved(label))
        .collect::<Result<Vec<_>, _>>()
        .map(|rows| rows.into_iter().flatten().collect())
}

pub fn state_kind_blurb(kind: StateKind) -> &'static str {
    match kind {
        StateKind::Resource => {
            "Numeric track with a maximum — stress 2/5, hit points, supply. During play the GM adjusts it with add/set/remove."
        }
        StateKind::Clock => {
            "Stepped progress toward an outcome — investigation 2/4, countdown. Each step fills one segment; often triggers or clears when full."
        }
        StateKind::Fact => {
            "Durable text attribute — location, shirt color, has_key. Set when established in play; update when the fiction changes."
        }
        StateKind::Condition => {
            "Temporary status tag — bleeding, hidden, suspicious. Same storage as a fact, but expected to clear when resolved."
        }
    }
}

fn state_kind_option_label(kind: StateKind) -> &'static str {
    match kind {
        StateKind::Resource => "Resource — numeric with max",
        StateKind::Condition => "Condition — temporary tag",
        StateKind::Fact => "Fact — durable text",
        StateKind::Clock => "Clock — stepped progress",
    }
}

#[derive(Clone, PartialEq)]
pub struct ScenarioStateDefView {
    pub key: String,
    pub kind: Option<StateKind>,
    pub description: String,
    pub initial_value: String,
    pub initial_num: Option<i64>,
    pub initial_max: Option<i64>,
    pub visibility: String,
    pub update_hints: String,
    /// `Some(target)` shows the world/PC target selector (tracked-var editor);
    /// `None` hides it (per-character state, which is implicitly that character).
    pub target: Option<String>,
}

#[derive(Clone, PartialEq)]
pub enum ScenarioStateFieldUpdate {
    Key(String),
    Kind(StateKind),
    Target(String),
    Description(String),
    InitialValue(String),
    InitialNum(Option<i64>),
    InitialMax(Option<i64>),
    Visibility(String),
    UpdateHints(String),
}

#[derive(Properties, PartialEq)]
pub struct ScenarioStateDefEditorProps {
    pub view: ScenarioStateDefView,
    pub on_update: Callback<ScenarioStateFieldUpdate>,
    pub on_remove: Callback<()>,
}

fn kind_select(selected: Option<StateKind>, on_change: Callback<StateKind>) -> Html {
    html! {
        <label class="field field-inline scenario-state-kind-picker">
            <span class="muted">{"Type"}</span>
            <select
                class="input"
                onchange={Callback::from(move |e: Event| {
                    let select: web_sys::HtmlSelectElement = e.target_unchecked_into();
                    let parsed = match select.value().as_str() {
                        "condition" => StateKind::Condition,
                        "fact" => StateKind::Fact,
                        "clock" => StateKind::Clock,
                        "resource" => StateKind::Resource,
                        _ => return,
                    };
                    on_change.emit(parsed);
                })}
            >
                <option value="" selected={selected.is_none()} disabled={true}>
                    {"Choose type…"}
                </option>
                <option value="resource" selected={selected == Some(StateKind::Resource)}>
                    { state_kind_option_label(StateKind::Resource) }
                </option>
                <option value="clock" selected={selected == Some(StateKind::Clock)}>
                    { state_kind_option_label(StateKind::Clock) }
                </option>
                <option value="fact" selected={selected == Some(StateKind::Fact)}>
                    { state_kind_option_label(StateKind::Fact) }
                </option>
                <option value="condition" selected={selected == Some(StateKind::Condition)}>
                    { state_kind_option_label(StateKind::Condition) }
                </option>
            </select>
        </label>
    }
}

fn target_select(target: &str, on_change: Callback<String>) -> Html {
    let is_pc = target.eq_ignore_ascii_case("pc");
    html! {
        <label class="field field-inline">
            <span class="muted">{"Target"}</span>
            <select
                class="input"
                onchange={Callback::from(move |e: Event| {
                    let select: web_sys::HtmlSelectElement = e.target_unchecked_into();
                    let value = if select.value() == "pc" { "pc" } else { "world" };
                    on_change.emit(value.to_string());
                })}
            >
                <option value="world" selected={!is_pc}>{"World"}</option>
                <option value="pc" selected={is_pc}>{"Player character"}</option>
            </select>
        </label>
    }
}

fn numeric_fields(
    kind: StateKind,
    initial_num: Option<i64>,
    initial_max: Option<i64>,
    on_update: Callback<ScenarioStateFieldUpdate>,
) -> Html {
    let current_label = if kind == StateKind::Clock {
        "Filled segments"
    } else {
        "Starting value"
    };
    let max_label = if kind == StateKind::Clock {
        "Total segments"
    } else {
        "Maximum"
    };
    let on_num = {
        let on_update = on_update.clone();
        Callback::from(move |value: Option<i64>| {
            on_update.emit(ScenarioStateFieldUpdate::InitialNum(value))
        })
    };
    let on_max = {
        let on_update = on_update.clone();
        Callback::from(move |value: Option<i64>| {
            on_update.emit(ScenarioStateFieldUpdate::InitialMax(value))
        })
    };
    html! {
        <div class="scenario-state-def-fields">
            { optional_i64_input(current_label, initial_num, on_num) }
            { optional_i64_input(max_label, initial_max, on_max) }
        </div>
    }
}

fn text_value_fields(initial_value: &str, on_update: Callback<ScenarioStateFieldUpdate>) -> Html {
    text_input("Starting value", initial_value, {
        let on_update = on_update.clone();
        Callback::from(move |value: String| {
            on_update.emit(ScenarioStateFieldUpdate::InitialValue(value))
        })
    })
}

#[function_component(ScenarioStateDefEditor)]
pub fn scenario_state_def_editor(props: &ScenarioStateDefEditorProps) -> Html {
    let view = &props.view;
    let on_update = props.on_update.clone();
    let on_kind = {
        let on_update = on_update.clone();
        Callback::from(move |kind: StateKind| on_update.emit(ScenarioStateFieldUpdate::Kind(kind)))
    };

    html! {
        <div class="scenario-state-def">
            <div class="scenario-state-def-header">
                { text_input("Key", &view.key, {
                    let on_update = on_update.clone();
                    Callback::from(move |value: String| on_update.emit(ScenarioStateFieldUpdate::Key(value)))
                }) }
                { kind_select(view.kind, on_kind) }
                if let Some(target) = &view.target {
                    { target_select(target, {
                        let on_update = on_update.clone();
                        Callback::from(move |value: String| on_update.emit(ScenarioStateFieldUpdate::Target(value)))
                    }) }
                }
                <button type="button" class="btn secondary btn-compact" onclick={props.on_remove.reform(|_| ())}>
                    {"Remove"}
                </button>
            </div>
            if view.kind.is_none() {
                <p class="muted scenario-state-def-hint">
                    {"Pick a type to show the fields that apply. Each key is one slot on this character or in the world."}
                </p>
            } else if let Some(kind) = view.kind {
                <p class="scenario-state-def-help">{ state_kind_blurb(kind) }</p>
                { textarea_input("Description", &view.description, {
                    let on_update = on_update.clone();
                    Callback::from(move |value: String| on_update.emit(ScenarioStateFieldUpdate::Description(value)))
                }) }
                if matches!(kind, StateKind::Resource | StateKind::Clock) {
                    { numeric_fields(kind, view.initial_num, view.initial_max, on_update.clone()) }
                } else {
                    { text_value_fields(&view.initial_value, on_update.clone()) }
                }
                <details class="scenario-state-def-advanced">
                    <summary class="muted">{"Advanced"}</summary>
                    { text_input("Visibility", &view.visibility, {
                        let on_update = on_update.clone();
                        Callback::from(move |value: String| on_update.emit(ScenarioStateFieldUpdate::Visibility(value)))
                    }) }
                    { textarea_input("Update hints for the GM", &view.update_hints, {
                        let on_update = on_update.clone();
                        Callback::from(move |value: String| on_update.emit(ScenarioStateFieldUpdate::UpdateHints(value)))
                    }) }
                </details>
            }
        </div>
    }
}
