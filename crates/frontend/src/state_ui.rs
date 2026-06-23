use dreamwell_types::{
    AppliedStateChange, ChatStateEntry, GameStateEntry, StateKind, StateOp, StoryStateEntry,
};
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
        StateKind::Resource => "resource",
        StateKind::Condition => "condition",
        StateKind::Fact => "fact",
        StateKind::Clock => "clock",
    }
}

fn op_label(op: StateOp) -> &'static str {
    match op {
        StateOp::Set => "set",
        StateOp::Add => "add",
        StateOp::Remove => "remove",
    }
}

fn op_chip_class(op: StateOp) -> &'static str {
    match op {
        StateOp::Set => "state-chip--op-set",
        StateOp::Add => "state-chip--op-add",
        StateOp::Remove => "state-chip--op-remove",
    }
}

fn format_state_change_value(sc: &AppliedStateChange) -> String {
    match sc.kind {
        StateKind::Resource | StateKind::Clock => {
            if let Some(delta) = sc.delta {
                if sc.op == StateOp::Add {
                    return format!("Δ{delta}");
                }
            }
            if let Some(prev) = sc.prev_num {
                if let Some(delta) = sc.delta {
                    let next = prev + delta;
                    return format!("{prev} → {next}");
                }
            }
            sc.value.clone().unwrap_or_default()
        }
        StateKind::Condition | StateKind::Fact => {
            if sc.op == StateOp::Remove {
                sc.prev_value.clone().unwrap_or_default()
            } else if let Some(prev) = sc.prev_value.as_ref().filter(|p| !p.is_empty()) {
                format!("{prev} → {}", sc.value.as_deref().unwrap_or_default())
            } else {
                sc.value.clone().unwrap_or_default()
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

/// Minimal expandable group for inline state-change markers in game prose.
#[function_component(InlineStateChangesGroup)]
pub fn inline_state_changes_group(props: &StateChangesListProps) -> Html {
    if props.changes.is_empty() {
        return html! {};
    }
    let count = props.changes.len();
    html! {
        <details class="game-inline-state-group">
            <summary class="game-inline-state-group-summary muted small">
                { format!("State changes ({count})") }
            </summary>
            <div class="game-inline-state-group-body">
                <StateChangesList changes={props.changes.clone()} />
            </div>
        </details>
    }
}

/// One applied state change row (shared by list and inline game prose).
pub fn render_state_change_row(sc: &AppliedStateChange) -> Html {
    let value_text = format_state_change_value(sc);
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
                <span class="state-row-value">{ value_text }</span>
            }
        </div>
    }
}

#[derive(Clone, PartialEq)]
pub struct StateEntryRow {
    pub id: i64,
    pub kind: StateKind,
    pub key: String,
    pub value: String,
    pub num_value: Option<i64>,
    pub max_value: Option<i64>,
}

impl From<&ChatStateEntry> for StateEntryRow {
    fn from(entry: &ChatStateEntry) -> Self {
        Self {
            id: entry.id,
            kind: entry.kind,
            key: entry.key.clone(),
            value: entry.value.clone(),
            num_value: entry.num_value,
            max_value: entry.max_value,
        }
    }
}

impl From<&StoryStateEntry> for StateEntryRow {
    fn from(entry: &StoryStateEntry) -> Self {
        Self {
            id: entry.id,
            kind: entry.kind,
            key: entry.key.clone(),
            value: entry.value.clone(),
            num_value: entry.num_value,
            max_value: entry.max_value,
        }
    }
}

impl From<&GameStateEntry> for StateEntryRow {
    fn from(entry: &GameStateEntry) -> Self {
        Self {
            id: entry.id,
            kind: entry.kind,
            key: entry.key.clone(),
            value: entry.value.clone(),
            num_value: entry.num_value,
            max_value: entry.max_value,
        }
    }
}

fn state_kind_order(kind: StateKind) -> u8 {
    match kind {
        StateKind::Resource => 0,
        StateKind::Condition => 1,
        StateKind::Fact => 2,
        StateKind::Clock => 3,
    }
}

pub fn sort_state_rows(mut rows: Vec<StateEntryRow>) -> Vec<StateEntryRow> {
    rows.sort_by(|left, right| {
        state_kind_order(left.kind)
            .cmp(&state_kind_order(right.kind))
            .then_with(|| left.key.cmp(&right.key))
    });
    rows
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
}

#[function_component(StateEntriesPanel)]
pub fn state_entries_panel(props: &StateEntriesPanelProps) -> Html {
    if props.entries.is_empty() {
        return html! {
            <p class="muted state-entries-empty">{"No typed state entries yet."}</p>
        };
    }
    let rows = sort_state_rows(props.entries.clone());
    let mut sections: Vec<(StateKind, Vec<&StateEntryRow>)> = Vec::new();
    for row in rows.iter() {
        if sections.last().map(|(kind, _)| *kind) != Some(row.kind) {
            sections.push((row.kind, Vec::new()));
        }
        sections.last_mut().unwrap().1.push(row);
    }

    html! {
        <div class="state-entries">
            { for sections.iter().enumerate().map(|(section_idx, (kind, entries))| {
                let kind_name = kind_label(*kind);
                html! {
                    <section class="state-group" key={section_idx}>
                        <div class="state-group-label">
                            <span class={classes!("state-chip", "state-chip--kind", format!("state-chip--kind-{kind_name}"))}>
                                { kind_name }
                            </span>
                        </div>
                        <ul class="state-list state-list--bordered">
                            { for entries.iter().map(|entry| {
                                let (value_text, meter_pct) = if matches!(entry.kind, StateKind::Resource | StateKind::Clock) {
                                    let current = entry.num_value.unwrap_or(0);
                                    let max = entry.max_value.unwrap_or(0).max(1);
                                    let pct = ((current as f64 / max as f64) * 100.0).clamp(0.0, 100.0);
                                    (format!("{current}/{max}"), Some(pct))
                                } else {
                                    (entry.value.clone(), None)
                                };
                                html! {
                                    <li class="state-row state-row--entry" key={entry.id}>
                                        <span class="state-row-key">{ &entry.key }</span>
                                        <span class="state-row-value">{ value_text }</span>
                                        if let Some(pct) = meter_pct {
                                            <span class="state-row-meter" aria-hidden="true">
                                                <span class="state-row-meter-fill" style={format!("width: {pct:.0}%")} />
                                            </span>
                                        }
                                    </li>
                                }
                            }) }
                        </ul>
                    </section>
                }
            }) }
        </div>
    }
}
