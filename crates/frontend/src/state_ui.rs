use dreamwell_types::{
    AppliedStateChange, ChatActor, ChatStateEntry, GameActor, GameStateEntry, SequencePayload,
    StateKind, StateOp, StoryActor, StoryStateEntry,
};
use dreamwell_units::{format_measurement_change_display, format_measurement_display};
use web_sys::MouseEvent;
use yew::prelude::*;

#[derive(Properties, PartialEq)]
pub struct PhaseSectionProps {
    pub label: String,
    /// Parent-controlled expansion; pair with `on_toggle`.
    #[prop_or_default]
    pub expanded: Option<bool>,
    #[prop_or_default]
    pub on_toggle: Option<Callback<MouseEvent>>,
    #[prop_or(true)]
    pub default_expanded: bool,
    pub children: Children,
}

#[function_component(PhaseSection)]
pub fn phase_section(props: &PhaseSectionProps) -> Html {
    let internal = use_state(|| props.default_expanded);
    let controlled = props.expanded.is_some() && props.on_toggle.is_some();
    let expanded = if controlled {
        props.expanded.unwrap_or(false)
    } else {
        *internal
    };

    let on_click = if let Some(on_toggle) = props.on_toggle.clone() {
        on_toggle
    } else {
        let internal = internal.clone();
        Callback::from(move |_: MouseEvent| internal.set(!*internal))
    };

    html! {
        <div class="message-thought game-phase-section">
            <button type="button" class="message-thought-toggle" onclick={on_click}>
                <span class="message-thought-label">{ &props.label }</span>
                <span class="message-thought-chevron" aria-hidden="true">
                    { if expanded { "▾" } else { "▸" } }
                </span>
            </button>
            if expanded {
                <div class="message-thought-body game-phase-section-body">
                    { for props.children.iter() }
                </div>
            }
        </div>
    }
}

#[derive(Properties, PartialEq)]
pub struct StateChangesListProps {
    pub changes: Vec<AppliedStateChange>,
}

fn kind_label(kind: StateKind) -> &'static str {
    match kind {
        StateKind::Measurement => "measurement",
        StateKind::Condition => "condition",
        StateKind::Variable => "variable",
        StateKind::Sequence => "sequence",
    }
}

fn op_label(op: StateOp) -> &'static str {
    match op {
        StateOp::Set => "set",
        StateOp::Add => "add",
        StateOp::Remove => "remove",
        StateOp::SetMin => "set min",
        StateOp::SetMax => "set max",
        StateOp::Step => "step",
    }
}

fn op_chip_class(op: StateOp) -> &'static str {
    match op {
        StateOp::Set => "state-chip--op-set",
        StateOp::Add => "state-chip--op-add",
        StateOp::Remove => "state-chip--op-remove",
        StateOp::SetMin | StateOp::SetMax | StateOp::Step => "state-chip--op-set",
    }
}

fn format_state_change_value(sc: &AppliedStateChange) -> (String, Option<String>) {
    match sc.kind {
        StateKind::Measurement => {
            let unit = sc.unit.as_deref();
            let prev = sc.prev_float.or_else(|| sc.prev_num.map(|n| n as f64));
            let next = sc
                .value
                .as_deref()
                .and_then(|v| v.parse::<f64>().ok())
                .or_else(|| prev.zip(sc.delta.map(|d| d as f64)).map(|(p, d)| p + d));
            if matches!(sc.op, StateOp::SetMin | StateOp::SetMax) {
                let Some(value) = next.or(prev) else {
                    return (String::new(), None);
                };
                let display = format_measurement_display(value, None, unit);
                let label = if sc.op == StateOp::SetMin {
                    "min"
                } else {
                    "max"
                };
                return (format!("{label}: {}", display.primary), display.secondary);
            }
            let display = format_measurement_change_display(prev, next, unit);
            (display.primary, display.secondary)
        }
        StateKind::Sequence => {
            if sc.op == StateOp::Step {
                let text = sc
                    .delta
                    .map(|d| {
                        if d == 1 {
                            "forward".to_string()
                        } else if d == -1 {
                            "back".to_string()
                        } else {
                            format!("{d:+}")
                        }
                    })
                    .unwrap_or_else(|| "step".to_string());
                (text, None)
            } else {
                (sc.value.clone().unwrap_or_default(), None)
            }
        }
        StateKind::Condition | StateKind::Variable => {
            if sc.op == StateOp::Remove {
                (sc.prev_value.clone().unwrap_or_default(), None)
            } else if let Some(prev) = sc.prev_value.as_ref().filter(|p| !p.is_empty()) {
                (
                    format!("{prev} → {}", sc.value.as_deref().unwrap_or_default()),
                    None,
                )
            } else {
                (sc.value.clone().unwrap_or_default(), None)
            }
        }
    }
}

#[function_component(StateChangesList)]
pub fn state_changes_list(props: &StateChangesListProps) -> Html {
    if props.changes.is_empty() {
        return html! {};
    }
    html! {
        <ul class="state-list state-list--changes">
            { for props.changes.iter().enumerate().map(|(i, sc)| html! {
                <li key={i}>{ render_state_change_row(sc) }</li>
            }) }
        </ul>
    }
}

fn format_state_change_capsule(sc: &AppliedStateChange) -> String {
    let (value_text, _) = format_state_change_value(sc);
    match sc.op {
        StateOp::Remove => {
            if value_text.is_empty() {
                format!("− {}", sc.key)
            } else {
                format!("− {} ({})", sc.key, value_text)
            }
        }
        StateOp::Add if sc.kind == StateKind::Measurement => {
            if let Some(delta) = sc.delta {
                format!("{} {:+}", sc.key, delta)
            } else if !value_text.is_empty() {
                format!("{} {value_text}", sc.key)
            } else {
                sc.key.clone()
            }
        }
        _ => {
            if value_text.is_empty() {
                sc.key.clone()
            } else if value_text.contains('→') {
                format!("{} {value_text}", sc.key)
            } else {
                format!("{} · {value_text}", sc.key)
            }
        }
    }
}

fn state_change_capsule_title(sc: &AppliedStateChange) -> String {
    let (value, alt) = format_state_change_value(sc);
    let mut parts = vec![
        format!("{} {}", op_label(sc.op), kind_label(sc.kind)),
        sc.key.clone(),
    ];
    if !sc.target.is_empty() {
        parts.push(sc.target.clone());
    }
    if !value.is_empty() {
        parts.push(value);
    }
    if let Some(alt) = alt {
        parts.push(alt);
    }
    parts.join(" · ")
}

/// Compact pill shown at inline `⟦state:N⟧` markers in game prose.
pub fn render_state_change_capsule(sc: &AppliedStateChange) -> Html {
    let label = format_state_change_capsule(sc);
    let kind_name = kind_label(sc.kind);
    let title = state_change_capsule_title(sc);
    html! {
        <span
            class={classes!("game-state-capsule", format!("game-state-capsule--{kind_name}"))}
            title={title}
        >
            { label }
        </span>
    }
}

/// One or more capsules at the point in narration where state tools fired.
pub fn render_inline_state_capsules(changes: &[AppliedStateChange]) -> Html {
    if changes.is_empty() {
        return html! {};
    }
    html! {
        <span class="game-inline-state-capsules">
            { for changes.iter().enumerate().map(|(i, sc)| html! {
                <span key={i}>{ render_state_change_capsule(sc) }</span>
            }) }
        </span>
    }
}

/// One applied state change row (shared by list and inline game prose).
pub fn render_state_change_row(sc: &AppliedStateChange) -> Html {
    let (value_text, value_alt) = format_state_change_value(sc);
    html! {
        <div class="state-row state-row--change">
            <span class={classes!("state-chip", "state-chip--op", op_chip_class(sc.op))}>
                { op_label(sc.op) }
            </span>
            <span class={classes!("state-chip", "state-chip--kind", format!("state-chip--kind-{}", kind_label(sc.kind)))}>
                { kind_label(sc.kind) }
            </span>
            if !sc.target.is_empty() {
                <span class="state-row-target">{ &sc.target }</span>
            }
            <span class="state-row-key">{ &sc.key }</span>
            if !value_text.is_empty() {
                <span class="state-row-value">
                    { value_text }
                    if let Some(alt) = value_alt {
                        <span class="state-row-value-alt">{ " · " }{ alt }</span>
                    }
                </span>
            }
        </div>
    }
}

#[derive(Clone, PartialEq)]
pub struct StateScopeActor {
    pub id: i64,
    pub name: String,
    pub role: String,
}

impl From<&GameActor> for StateScopeActor {
    fn from(actor: &GameActor) -> Self {
        Self {
            id: actor.id,
            name: actor.name.clone(),
            role: actor.role.clone(),
        }
    }
}

impl From<&ChatActor> for StateScopeActor {
    fn from(actor: &ChatActor) -> Self {
        Self {
            id: actor.id,
            name: actor.name.clone(),
            role: actor.role.clone(),
        }
    }
}

impl From<&StoryActor> for StateScopeActor {
    fn from(actor: &StoryActor) -> Self {
        Self {
            id: actor.id,
            name: actor.name.clone(),
            role: actor.role.clone(),
        }
    }
}

#[derive(Clone, PartialEq)]
pub struct StateEntryRow {
    pub id: i64,
    pub actor_id: Option<i64>,
    pub kind: StateKind,
    pub key: String,
    pub value: String,
    pub float_value: Option<f64>,
    pub float_max: Option<f64>,
    pub unit: Option<String>,
}

impl From<&ChatStateEntry> for StateEntryRow {
    fn from(entry: &ChatStateEntry) -> Self {
        Self {
            id: entry.id,
            actor_id: entry.actor_id,
            kind: entry.kind,
            key: entry.key.clone(),
            value: entry.value.clone(),
            float_value: entry.float_value,
            float_max: entry.float_max,
            unit: entry.unit.clone(),
        }
    }
}

impl From<&StoryStateEntry> for StateEntryRow {
    fn from(entry: &StoryStateEntry) -> Self {
        Self {
            id: entry.id,
            actor_id: entry.actor_id,
            kind: entry.kind,
            key: entry.key.clone(),
            value: entry.value.clone(),
            float_value: entry.float_value,
            float_max: entry.float_max,
            unit: entry.unit.clone(),
        }
    }
}

impl From<&GameStateEntry> for StateEntryRow {
    fn from(entry: &GameStateEntry) -> Self {
        Self {
            id: entry.id,
            actor_id: entry.actor_id,
            kind: entry.kind,
            key: entry.key.clone(),
            value: entry.value.clone(),
            float_value: entry.float_value,
            float_max: entry.float_max,
            unit: entry.unit.clone(),
        }
    }
}

fn state_kind_order(kind: StateKind) -> u8 {
    match kind {
        StateKind::Measurement => 0,
        StateKind::Condition => 1,
        StateKind::Variable => 2,
        StateKind::Sequence => 3,
    }
}

fn scope_sort_key(actor_id: Option<i64>, actors: &[StateScopeActor]) -> (u8, String) {
    match actor_id {
        None => (0, String::new()),
        Some(id) => actors
            .iter()
            .find(|actor| actor.id == id)
            .map(|actor| {
                let rank = if actor.role == "pc" { 1 } else { 2 };
                (rank, actor.name.to_lowercase())
            })
            .unwrap_or((3, format!("{id}"))),
    }
}

fn scope_heading(actor_id: Option<i64>, actors: &[StateScopeActor]) -> String {
    match actor_id {
        None => "World".to_string(),
        Some(id) => actors
            .iter()
            .find(|actor| actor.id == id)
            .map(|actor| {
                if actor.name.trim().is_empty() {
                    match actor.role.as_str() {
                        "pc" => "Player character".to_string(),
                        "npc" => "NPC".to_string(),
                        other => other.to_string(),
                    }
                } else {
                    format!("{} ({})", actor.name, actor.role)
                }
            })
            .unwrap_or_else(|| format!("Actor #{id}")),
    }
}

fn format_entry_value(entry: &StateEntryRow) -> (String, Option<String>, bool, Option<f64>) {
    match entry.kind {
        StateKind::Measurement => {
            if entry.float_value.is_none() && entry.float_max.is_none() {
                return ("(not set)".to_string(), None, true, None);
            }
            let current = entry.float_value.unwrap_or(0.0);
            let display =
                format_measurement_display(current, entry.float_max, entry.unit.as_deref());
            let pct = entry
                .float_max
                .map(|max| ((current / max) * 100.0).clamp(0.0, 100.0));
            (display.primary, display.secondary, false, pct)
        }
        StateKind::Sequence => {
            if let Some(seq) = SequencePayload::decode(&entry.value) {
                let active = seq.active_item().unwrap_or("?");
                let items = seq.items.join(", ");
                let loop_tag = if seq.r#loop { " (loop)" } else { "" };
                return (format!("{active} [{items}]{loop_tag}"), None, false, None);
            }
            if entry.value.trim().is_empty() {
                ("(not set)".to_string(), None, true, None)
            } else {
                (entry.value.clone(), None, false, None)
            }
        }
        _ => {
            if entry.value.trim().is_empty() {
                ("(empty)".to_string(), None, true, None)
            } else {
                (entry.value.clone(), None, false, None)
            }
        }
    }
}

#[allow(dead_code)]
pub fn sort_state_rows(mut rows: Vec<StateEntryRow>) -> Vec<StateEntryRow> {
    rows.sort_by(|left, right| {
        left.actor_id
            .cmp(&right.actor_id)
            .then_with(|| state_kind_order(left.kind).cmp(&state_kind_order(right.kind)))
            .then_with(|| left.key.cmp(&right.key))
    });
    rows
}

fn group_entries_by_scope<'a>(
    rows: &'a [StateEntryRow],
    actors: &[StateScopeActor],
) -> Vec<(Option<i64>, Vec<&'a StateEntryRow>)> {
    let mut actor_ids: Vec<Option<i64>> = Vec::new();
    for row in rows {
        if !actor_ids.contains(&row.actor_id) {
            actor_ids.push(row.actor_id);
        }
    }
    actor_ids.sort_by_key(|left| scope_sort_key(*left, actors));

    actor_ids
        .into_iter()
        .filter_map(|actor_id| {
            let mut entries: Vec<&StateEntryRow> =
                rows.iter().filter(|row| row.actor_id == actor_id).collect();
            if entries.is_empty() {
                return None;
            }
            entries.sort_by(|left, right| {
                state_kind_order(left.kind)
                    .cmp(&state_kind_order(right.kind))
                    .then_with(|| left.key.cmp(&right.key))
            });
            Some((actor_id, entries))
        })
        .collect()
}

#[derive(Properties, PartialEq)]
pub struct PlanBeatsListProps {
    pub beats: Vec<String>,
    #[prop_or("Plan beats".into())]
    pub label: String,
    /// When true, render only the list (for embedding inside another collapsible section).
    #[prop_or(false)]
    pub inline: bool,
}

#[function_component(PlanBeatsList)]
pub fn plan_beats_list(props: &PlanBeatsListProps) -> Html {
    if props.beats.is_empty() {
        return html! {};
    }
    let list = html! {
        <ul class="plan-beats-list">
            { for props.beats.iter().enumerate().map(|(i, beat)| html! {
                <li key={i}>{ beat }</li>
            }) }
        </ul>
    };
    if props.inline {
        return list;
    }
    html! {
        <details class="plan-beats-list">
            <summary>{ &props.label }</summary>
            { list }
        </details>
    }
}

#[derive(Properties, PartialEq)]
pub struct StateEntriesPanelProps {
    pub entries: Vec<StateEntryRow>,
    #[prop_or_default]
    pub actors: Vec<StateScopeActor>,
}

fn render_state_entry_row(entry: &StateEntryRow) -> Html {
    let kind_name = kind_label(entry.kind);
    let (value_text, value_alt, value_empty, meter_pct) = format_entry_value(entry);
    html! {
        <li class="state-row state-row--entry" key={entry.id}>
            <span class={classes!("state-chip", "state-chip--kind", format!("state-chip--kind-{kind_name}"))}>
                { kind_name }
            </span>
            <span class="state-row-key">{ &entry.key }</span>
            <span class={classes!("state-row-value", value_empty.then_some("state-row-value--empty"))}>
                { value_text }
                if let Some(alt) = value_alt {
                    <span class="state-row-value-alt">{ " · " }{ alt }</span>
                }
            </span>
            if let Some(pct) = meter_pct {
                <span class="state-row-meter" aria-hidden="true">
                    <span class="state-row-meter-fill" style={format!("width: {pct:.0}%")} />
                </span>
            }
        </li>
    }
}

#[function_component(StateEntriesPanel)]
pub fn state_entries_panel(props: &StateEntriesPanelProps) -> Html {
    if props.entries.is_empty() {
        return html! {
            <p class="muted state-entries-empty">{"No typed state entries yet."}</p>
        };
    }
    let scopes = group_entries_by_scope(&props.entries, &props.actors);

    html! {
        <div class="state-entries">
            { for scopes.iter().enumerate().map(|(scope_idx, (actor_id, entries))| {
                let heading = scope_heading(*actor_id, &props.actors);
                html! {
                    <section class="state-scope-group" key={scope_idx}>
                        <h4 class="state-scope-heading">{ heading }</h4>
                        <ul class="state-list state-list--bordered">
                            { for entries.iter().map(|entry| render_state_entry_row(entry)) }
                        </ul>
                    </section>
                }
            }) }
        </div>
    }
}
