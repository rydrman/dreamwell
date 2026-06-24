use dreamwell_types::*;
use std::collections::HashMap;
use web_sys::{HtmlInputElement, HtmlTextAreaElement};
use yew::prelude::*;

use crate::api;
use crate::game_presets_ui::GmTonePresetPicker;

#[derive(Properties, PartialEq)]
pub struct ScenariosPageProps {
    pub selected_scenario_id: Option<i64>,
    pub game_id: Option<i64>,
    pub on_back: Callback<()>,
    pub on_scenario_change: Callback<(i64, i64)>,
    pub on_start_game: Callback<Scenario>,
    pub on_scenarios_changed: Callback<()>,
}

#[function_component(ScenariosPage)]
pub fn scenarios_page(props: &ScenariosPageProps) -> Html {
    html! {
        <main class="main scenarios-page">
            <header class="header">
                <button class="btn secondary" onclick={props.on_back.reform(|_| ())}>{"← Back"}</button>
                <h1 class="header-title">{"Scenarios"}</h1>
                <p class="header-subtitle muted">{"Create, edit, import, and export scenarios, then play them as games."}</p>
            </header>
            <div class="scenarios-page-body">
                <ScenarioPanel
                    selected_scenario_id={props.selected_scenario_id}
                    game_id={props.game_id}
                    on_scenario_change={props.on_scenario_change.clone()}
                    on_start_game={props.on_start_game.clone()}
                    on_scenarios_changed={props.on_scenarios_changed.clone()}
                />
            </div>
        </main>
    }
}

#[derive(Properties, PartialEq)]
struct ScenarioPanelProps {
    selected_scenario_id: Option<i64>,
    game_id: Option<i64>,
    on_scenario_change: Callback<(i64, i64)>,
    on_start_game: Callback<Scenario>,
    on_scenarios_changed: Callback<()>,
}

#[function_component(ScenarioPanel)]
fn scenario_panel(props: &ScenarioPanelProps) -> Html {
    let scenarios = use_state(Vec::<Scenario>::new);
    let draft = use_state(ScenarioDraft::default);
    let editing_id = use_state(|| None::<i64>);
    let file_input = use_node_ref();

    {
        let scenarios = scenarios.clone();
        use_effect_with((), move |_| {
            wasm_bindgen_futures::spawn_local(async move {
                if let Ok(list) = api::list_scenarios().await {
                    scenarios.set(list);
                }
            });
            || ()
        });
    }

    {
        let draft = draft.clone();
        let editing_id = editing_id.clone();
        let scenarios = scenarios.clone();
        let selected = props.selected_scenario_id;
        use_effect_with(selected, move |selected| {
            if let Some(id) = *selected {
                if let Some(scenario) = scenarios.iter().find(|s| s.id == id) {
                    editing_id.set(Some(scenario.id));
                    draft.set(ScenarioDraft::from(scenario));
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
                        draft.set(ScenarioDraft::default());
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
                if let Some(id) = *editing_id {
                    <button class="btn secondary" onclick={{
                        Callback::from(move |_| {
                            wasm_bindgen_futures::spawn_local(async move {
                                if let Err(err) = download_scenario_export(id).await {
                                    if let Some(window) = web_sys::window() {
                                        let _ = window.alert_with_message(&format!(
                                            "Could not export scenario: {err}"
                                        ));
                                    }
                                }
                            });
                        })
                    }}>{"Export JSON"}</button>
                }
                <input type="file" accept=".json,.png" ref={file_input} style="display:none;" onchange={{
                    let scenarios = scenarios.clone();
                    let draft = draft.clone();
                    let editing_id = editing_id.clone();
                    let on_scenarios_changed = props.on_scenarios_changed.clone();
                    Callback::from(move |e: Event| {
                        let input: HtmlInputElement = e.target_unchecked_into();
                        if let Some(file) = input.files().and_then(|f| f.get(0)) {
                            let scenarios = scenarios.clone();
                            let draft = draft.clone();
                            let editing_id = editing_id.clone();
                            let on_scenarios_changed = on_scenarios_changed.clone();
                            wasm_bindgen_futures::spawn_local(async move {
                                if let Ok(scenario) = api::import_scenario(&file).await {
                                    if let Ok(list) = api::list_scenarios().await {
                                        scenarios.set(list);
                                    }
                                    on_scenarios_changed.emit(());
                                    editing_id.set(Some(scenario.id));
                                    draft.set(ScenarioDraft::from(&scenario));
                                }
                            });
                        }
                    })
                }} />
            </div>
            <div class="scroll-list">
                { for scenarios.iter().map(|scenario| {
                    let id = scenario.id;
                    let delete_name = scenario.title.clone();
                    let play_scenario = scenario.clone();
                    html! {
                        <div class="list-row"
                            onclick={{
                                let draft = draft.clone();
                                let editing_id = editing_id.clone();
                                let scenario = scenario.clone();
                                Callback::from(move |_| {
                                    editing_id.set(Some(id));
                                    draft.set(ScenarioDraft::from(&scenario));
                                })
                            }}>
                            <span class="list-row-name">
                                { &scenario.title }
                                if scenario.content_flags.mature {
                                    <span class="content-warning-badge" title="Mature content">{" ⚠"}</span>
                                }
                            </span>
                            <button class="btn secondary btn-compact" onclick={{
                                let on_start_game = props.on_start_game.clone();
                                let play_scenario = play_scenario.clone();
                                Callback::from(move |e: MouseEvent| {
                                    e.stop_propagation();
                                    on_start_game.emit(play_scenario.clone());
                                })
                            }}>{"Play"}</button>
                            <button class="btn secondary btn-compact" onclick={{
                                let scenarios = scenarios.clone();
                                let draft = draft.clone();
                                let editing_id = editing_id.clone();
                                let on_scenarios_changed = props.on_scenarios_changed.clone();
                                Callback::from(move |e: MouseEvent| {
                                    e.stop_propagation();
                                    if !confirm_scenario_delete(&delete_name) {
                                        return;
                                    }
                                    let scenarios = scenarios.clone();
                                    let draft = draft.clone();
                                    let editing_id = editing_id.clone();
                                    let on_scenarios_changed = on_scenarios_changed.clone();
                                    wasm_bindgen_futures::spawn_local(async move {
                                        match api::delete_scenario(id).await {
                                            Ok(()) => {
                                                if *editing_id == Some(id) {
                                                    editing_id.set(None);
                                                    draft.set(ScenarioDraft::default());
                                                }
                                                if let Ok(list) = api::list_scenarios().await {
                                                    scenarios.set(list);
                                                }
                                                on_scenarios_changed.emit(());
                                            }
                                            Err(err) => {
                                                if let Some(window) = web_sys::window() {
                                                    let _ = window.alert_with_message(&format!(
                                                        "Could not delete scenario: {err}"
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
            { scenario_fields(&draft) }
            { crate::scenario_editors::scenario_advanced_editors(&draft) }
            { scenario_traits_editor(&draft) }
            <button class="btn" style="margin-top:0.5rem;" onclick={{
                let draft = draft.clone();
                let editing_id = editing_id.clone();
                let scenarios = scenarios.clone();
                let on_scenario_change = props.on_scenario_change.clone();
                let on_scenarios_changed = props.on_scenarios_changed.clone();
                let game_id = props.game_id;
                Callback::from(move |_| {
                    if draft.title.trim().is_empty() {
                        if let Some(window) = web_sys::window() {
                            let _ = window.alert_with_message("Title is required.");
                        }
                        return;
                    }
                    let payload = draft.to_create();
                    let editing_id_val = *editing_id;
                    let scenarios = scenarios.clone();
                    let draft = draft.clone();
                    let editing_id = editing_id.clone();
                    let on_scenario_change = on_scenario_change.clone();
                    let on_scenarios_changed = on_scenarios_changed.clone();
                    wasm_bindgen_futures::spawn_local(async move {
                        let scenario = if let Some(id) = editing_id_val {
                            api::update_scenario(id, &draft.to_update()).await
                        } else {
                            api::create_scenario(&payload).await
                        };
                        if let Ok(scenario) = scenario {
                            if let Ok(list) = api::list_scenarios().await {
                                scenarios.set(list);
                            }
                            on_scenarios_changed.emit(());
                            editing_id.set(Some(scenario.id));
                            draft.set(ScenarioDraft::from(&scenario));
                            if let Some(game_id) = game_id {
                                on_scenario_change.emit((game_id, scenario.id));
                            }
                        }
                    });
                })
            }}>{"Save scenario"}</button>
        </div>
    }
}

#[derive(Clone, PartialEq)]
pub(crate) struct ScenarioDraft {
    pub(crate) title: String,
    pub(crate) premise: String,
    pub(crate) setting: String,
    pub(crate) gm_style: String,
    pub(crate) opening_message: String,
    pub(crate) opening_guidance: String,
    pub(crate) pc_name: String,
    pub(crate) pc_description: String,
    pub(crate) pc_initial_state: Vec<CharacterStateDef>,
    pub(crate) trait_rows: Vec<(String, i64)>,
    pub(crate) objective: String,
    pub(crate) setup_text: String,
    pub(crate) rules_blocks: Vec<RulesBlock>,
    pub(crate) cast: Vec<ScenarioNpc>,
    pub(crate) trait_defs: Vec<TraitDef>,
    pub(crate) pc_options: Vec<PcOption>,
    pub(crate) state_schema: Vec<TrackedVarDef>,
    pub(crate) content_flags: ContentFlags,
    pub(crate) win_condition: Option<WinCondition>,
    pub(crate) scenario_triggers: Vec<ScenarioTrigger>,
    pub(crate) source_meta: Option<SourceMeta>,
    pub(crate) game_elements: GameElementsConfig,
}

impl Default for ScenarioDraft {
    fn default() -> Self {
        Self {
            title: String::new(),
            premise: String::new(),
            setting: String::new(),
            gm_style: String::new(),
            opening_message: String::new(),
            opening_guidance: String::new(),
            pc_name: String::new(),
            pc_description: String::new(),
            pc_initial_state: Vec::new(),
            trait_rows: sorted_trait_rows(&default_game_traits()),
            objective: String::new(),
            setup_text: String::new(),
            rules_blocks: Vec::new(),
            cast: Vec::new(),
            trait_defs: Vec::new(),
            pc_options: Vec::new(),
            state_schema: Vec::new(),
            content_flags: ContentFlags::default(),
            win_condition: None,
            scenario_triggers: Vec::new(),
            source_meta: None,
            game_elements: GameElementsConfig::default(),
        }
    }
}

impl ScenarioDraft {
    fn from(scenario: &Scenario) -> Self {
        let trait_rows = if scenario.trait_defs.is_empty() {
            sorted_trait_rows(&scenario.traits)
        } else {
            scenario
                .trait_defs
                .iter()
                .map(|t| {
                    (
                        t.name.clone(),
                        scenario.traits.get(&t.name).copied().unwrap_or(0),
                    )
                })
                .collect()
        };
        let columns: Vec<String> = if scenario.trait_defs.is_empty() {
            trait_rows.iter().map(|(name, _)| name.clone()).collect()
        } else {
            scenario.trait_defs.iter().map(|t| t.name.clone()).collect()
        };
        let mut pc_options = scenario.pc_options.clone();
        for pc in &mut pc_options {
            ensure_trait_keys(&mut pc.traits, &columns);
        }
        let mut cast = scenario.cast.clone();
        for npc in &mut cast {
            ensure_trait_keys(&mut npc.traits, &columns);
        }
        Self {
            title: scenario.title.clone(),
            premise: scenario.premise.clone(),
            setting: scenario.setting.clone(),
            gm_style: scenario.gm_style.clone(),
            opening_message: scenario.opening_message.clone(),
            opening_guidance: scenario.opening_guidance.clone(),
            pc_name: scenario.pc_name.clone(),
            pc_description: scenario.pc_description.clone(),
            pc_initial_state: scenario.pc_initial_state.clone(),
            trait_rows,
            objective: scenario.objective.clone(),
            setup_text: scenario.setup_text.clone(),
            rules_blocks: scenario.rules_blocks.clone(),
            cast,
            trait_defs: scenario.trait_defs.clone(),
            pc_options,
            state_schema: scenario.state_schema.clone(),
            content_flags: scenario.content_flags.clone(),
            win_condition: scenario.win_condition.clone(),
            scenario_triggers: scenario.scenario_triggers.clone(),
            source_meta: scenario.source_meta.clone(),
            game_elements: scenario.game_elements.clone(),
        }
    }

    fn traits_map(&self) -> HashMap<String, i64> {
        let mut traits = HashMap::new();
        for (name, value) in &self.trait_rows {
            let name = name.trim();
            if name.is_empty() {
                continue;
            }
            traits.insert(name.to_string(), *value);
        }
        traits
    }

    fn to_create(&self) -> ScenarioCreate {
        ScenarioCreate {
            title: self.title.trim().to_string(),
            premise: self.premise.clone(),
            setting: self.setting.clone(),
            gm_style: self.gm_style.clone(),
            opening_message: self.opening_message.clone(),
            opening_guidance: self.opening_guidance.clone(),
            pc_name: self.pc_name.clone(),
            pc_description: self.pc_description.clone(),
            pc_initial_state: self.pc_initial_state.clone(),
            traits: self.traits_map(),
            character_id: None,
            objective: self.objective.clone(),
            setup_text: self.setup_text.clone(),
            rules_blocks: self.rules_blocks.clone(),
            cast: self.cast.clone(),
            trait_defs: self.trait_defs.clone(),
            pc_options: self.pc_options.clone(),
            state_schema: self.state_schema.clone(),
            win_condition: self.win_condition.clone(),
            content_flags: self.content_flags.clone(),
            source_meta: self.source_meta.clone(),
            scenario_triggers: self.scenario_triggers.clone(),
            game_elements: self.game_elements.clone(),
        }
    }

    fn to_update(&self) -> ScenarioUpdate {
        ScenarioUpdate {
            title: Some(self.title.trim().to_string()),
            premise: Some(self.premise.clone()),
            setting: Some(self.setting.clone()),
            gm_style: Some(self.gm_style.clone()),
            opening_message: Some(self.opening_message.clone()),
            opening_guidance: Some(self.opening_guidance.clone()),
            pc_name: Some(self.pc_name.clone()),
            pc_description: Some(self.pc_description.clone()),
            pc_initial_state: Some(self.pc_initial_state.clone()),
            traits: Some(self.traits_map()),
            character_id: None,
            objective: Some(self.objective.clone()),
            setup_text: Some(self.setup_text.clone()),
            rules_blocks: Some(self.rules_blocks.clone()),
            cast: Some(self.cast.clone()),
            trait_defs: Some(self.trait_defs.clone()),
            pc_options: Some(self.pc_options.clone()),
            state_schema: Some(self.state_schema.clone()),
            win_condition: Some(self.win_condition.clone()),
            content_flags: Some(self.content_flags.clone()),
            source_meta: Some(self.source_meta.clone()),
            scenario_triggers: Some(self.scenario_triggers.clone()),
            game_elements: Some(self.game_elements.clone()),
        }
    }
}

fn ensure_trait_keys(traits: &mut HashMap<String, i64>, columns: &[String]) {
    for column in columns {
        traits.entry(column.clone()).or_insert(0);
    }
}

pub(crate) fn trait_column_names(draft: &ScenarioDraft) -> Vec<String> {
    if !draft.trait_defs.is_empty() {
        draft
            .trait_defs
            .iter()
            .map(|def| def.name.clone())
            .collect()
    } else {
        draft
            .trait_rows
            .iter()
            .map(|(name, _)| name.clone())
            .collect()
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum TraitRowOwner {
    Default,
    PcOption(usize),
    CastNpc(usize),
}

pub(crate) fn trait_value(draft: &ScenarioDraft, owner: TraitRowOwner, trait_name: &str) -> i64 {
    match owner {
        TraitRowOwner::Default => draft.traits_map().get(trait_name).copied().unwrap_or(0),
        TraitRowOwner::PcOption(index) => draft
            .pc_options
            .get(index)
            .and_then(|pc| pc.traits.get(trait_name))
            .copied()
            .unwrap_or(0),
        TraitRowOwner::CastNpc(index) => draft
            .cast
            .get(index)
            .and_then(|npc| npc.traits.get(trait_name))
            .copied()
            .unwrap_or(0),
    }
}

pub(crate) fn set_trait_value(
    draft: &mut ScenarioDraft,
    owner: TraitRowOwner,
    trait_name: &str,
    value: i64,
) {
    match owner {
        TraitRowOwner::Default => {
            if let Some(row) = draft
                .trait_rows
                .iter_mut()
                .find(|(name, _)| name == trait_name)
            {
                row.1 = value;
            } else {
                draft.trait_rows.push((trait_name.to_string(), value));
            }
        }
        TraitRowOwner::PcOption(index) => {
            if let Some(pc) = draft.pc_options.get_mut(index) {
                pc.traits.insert(trait_name.to_string(), value);
            }
        }
        TraitRowOwner::CastNpc(index) => {
            if let Some(npc) = draft.cast.get_mut(index) {
                npc.traits.insert(trait_name.to_string(), value);
            }
        }
    }
}

pub(crate) fn rename_trait_column(draft: &mut ScenarioDraft, col_index: usize, new_name: String) {
    let columns = trait_column_names(draft);
    let Some(old_name) = columns.get(col_index).cloned() else {
        return;
    };
    if old_name == new_name {
        return;
    }

    if !draft.trait_defs.is_empty() {
        if let Some(def) = draft.trait_defs.get_mut(col_index) {
            def.name = new_name.clone();
        }
    }
    if let Some(row) = draft.trait_rows.get_mut(col_index) {
        row.0 = new_name.clone();
    }

    for pc in &mut draft.pc_options {
        if let Some(value) = pc.traits.remove(&old_name) {
            pc.traits.insert(new_name.clone(), value);
        }
    }
    for npc in &mut draft.cast {
        if let Some(value) = npc.traits.remove(&old_name) {
            npc.traits.insert(new_name.clone(), value);
        }
    }
}

pub(crate) fn add_trait_column(draft: &mut ScenarioDraft) {
    if !draft.trait_defs.is_empty() {
        draft.trait_defs.push(TraitDef::default());
    }
    draft.trait_rows.push((String::new(), 0));
    for pc in &mut draft.pc_options {
        pc.traits.insert(String::new(), 0);
    }
    for npc in &mut draft.cast {
        npc.traits.insert(String::new(), 0);
    }
}

pub(crate) fn remove_trait_column(draft: &mut ScenarioDraft, col_index: usize) {
    let name = trait_column_names(draft).get(col_index).cloned();
    if !draft.trait_defs.is_empty() {
        draft.trait_defs.remove(col_index);
    }
    draft.trait_rows.remove(col_index);
    if let Some(name) = name {
        for pc in &mut draft.pc_options {
            pc.traits.remove(&name);
        }
        for npc in &mut draft.cast {
            npc.traits.remove(&name);
        }
    }
}

pub(crate) fn traits_for_columns(columns: &[String]) -> HashMap<String, i64> {
    columns.iter().map(|column| (column.clone(), 0)).collect()
}

fn trait_row_label(draft: &ScenarioDraft, owner: TraitRowOwner) -> String {
    match owner {
        TraitRowOwner::Default => {
            if draft.pc_name.trim().is_empty() {
                "Default PC".to_string()
            } else {
                draft.pc_name.clone()
            }
        }
        TraitRowOwner::PcOption(index) => draft
            .pc_options
            .get(index)
            .map(|pc| {
                if pc.name.trim().is_empty() {
                    format!("PC option {}", index + 1)
                } else {
                    pc.name.clone()
                }
            })
            .unwrap_or_else(|| format!("PC option {}", index + 1)),
        TraitRowOwner::CastNpc(index) => draft
            .cast
            .get(index)
            .map(|npc| {
                let name = if npc.name.trim().is_empty() {
                    format!("NPC {}", index + 1)
                } else {
                    npc.name.clone()
                };
                format!("{name} (NPC)")
            })
            .unwrap_or_else(|| format!("NPC {} (NPC)", index + 1)),
    }
}

pub(crate) fn synced_trait_values_row(
    draft: &UseStateHandle<ScenarioDraft>,
    owner: TraitRowOwner,
) -> Html {
    let columns = trait_column_names(draft);
    if columns.is_empty() {
        return html! {};
    }
    html! {
        <div class="scenario-trait-matrix-wrap">
            <table class="scenario-trait-matrix scenario-trait-matrix-inline">
                <thead>
                    <tr>
                        { for columns.iter().enumerate().map(|(col_index, column)| {
                            html! {
                                <th key={col_index}>{ column.clone() }</th>
                            }
                        }) }
                    </tr>
                </thead>
                <tbody>
                    <tr>
                        { for columns.iter().enumerate().map(|(col_index, column)| {
                            let column = column.clone();
                            let value = trait_value(draft, owner, &column);
                            html! {
                                <td key={col_index}>
                                    <input
                                        type="number"
                                        class="input input-compact scenario-trait-mod"
                                        value={value.to_string()}
                                        oninput={{
                                            let draft = draft.clone();
                                            let column = column.clone();
                                            Callback::from(move |e: InputEvent| {
                                                let input: HtmlInputElement = e.target_unchecked_into();
                                                let parsed = input.value().parse::<i64>().unwrap_or(0);
                                                let mut next = (*draft).clone();
                                                set_trait_value(&mut next, owner, &column, parsed);
                                                draft.set(next);
                                            })
                                        }}
                                    />
                                </td>
                            }
                        }) }
                    </tr>
                </tbody>
            </table>
        </div>
    }
}

fn scenario_traits_editor(draft: &UseStateHandle<ScenarioDraft>) -> Html {
    let columns = trait_column_names(draft);
    let custom_defs = !draft.trait_defs.is_empty();
    let pc_count = draft.pc_options.len();
    let cast_count = draft.cast.len();
    let row_count = 1 + pc_count + cast_count;

    html! {
        <div class="scenario-traits">
            <div class="scenario-traits-header">
                <span class="muted">{"Traits / roles"}</span>
                <p class="muted scenario-traits-help">{"Trait modifiers for the default PC, alternate PC options, and cast NPCs. Columns match the trait sheet used during play."}</p>
            </div>
            if columns.is_empty() {
                <p class="muted">{"No traits defined yet."}</p>
            } else {
                <div class="scenario-trait-matrix-wrap">
                    <table class="scenario-trait-matrix">
                        <thead>
                            <tr>
                                <th>{"Character"}</th>
                                { for columns.iter().enumerate().map(|(col_index, column)| {
                                    let column = column.clone();
                                    html! {
                                        <th key={col_index}>
                                            if custom_defs {
                                                { column }
                                            } else {
                                                <div class="scenario-trait-header-cell">
                                                    <input
                                                        type="text"
                                                        class="input"
                                                        placeholder="Trait"
                                                        value={column}
                                                        oninput={{
                                                            let draft = draft.clone();
                                                            Callback::from(move |e: InputEvent| {
                                                                let input: HtmlInputElement = e.target_unchecked_into();
                                                                let mut next = (*draft).clone();
                                                                rename_trait_column(&mut next, col_index, input.value());
                                                                draft.set(next);
                                                            })
                                                        }}
                                                    />
                                                    <button
                                                        type="button"
                                                        class="btn secondary btn-compact"
                                                        title="Remove trait"
                                                        onclick={{
                                                            let draft = draft.clone();
                                                            Callback::from(move |_| {
                                                                let mut next = (*draft).clone();
                                                                remove_trait_column(&mut next, col_index);
                                                                draft.set(next);
                                                            })
                                                        }}
                                                    >
                                                        {"×"}
                                                    </button>
                                                </div>
                                            }
                                        </th>
                                    }
                                }) }
                            </tr>
                        </thead>
                        <tbody>
                            { for (0..row_count).map(|row_index| {
                                let owner = if row_index == 0 {
                                    TraitRowOwner::Default
                                } else if row_index - 1 < pc_count {
                                    TraitRowOwner::PcOption(row_index - 1)
                                } else {
                                    TraitRowOwner::CastNpc(row_index - 1 - pc_count)
                                };
                                html! {
                                    <tr key={row_index}>
                                        <th scope="row">{ trait_row_label(draft, owner) }</th>
                                        { for columns.iter().enumerate().map(|(col_index, column)| {
                                            let column = column.clone();
                                            let value = trait_value(draft, owner, &column);
                                            html! {
                                                <td key={col_index}>
                                                    <input
                                                        type="number"
                                                        class="input input-compact scenario-trait-mod"
                                                        value={value.to_string()}
                                                        oninput={{
                                                            let draft = draft.clone();
                                                            let column = column.clone();
                                                            Callback::from(move |e: InputEvent| {
                                                                let input: HtmlInputElement = e.target_unchecked_into();
                                                                let parsed = input.value().parse::<i64>().unwrap_or(0);
                                                                let mut next = (*draft).clone();
                                                                set_trait_value(&mut next, owner, &column, parsed);
                                                                draft.set(next);
                                                            })
                                                        }}
                                                    />
                                                </td>
                                            }
                                        }) }
                                    </tr>
                                }
                            }) }
                        </tbody>
                    </table>
                </div>
            }
            <button
                type="button"
                class="btn secondary"
                style="margin-top:0.5rem;"
                onclick={{
                    let draft = draft.clone();
                    Callback::from(move |_| {
                        let mut next = (*draft).clone();
                        add_trait_column(&mut next);
                        draft.set(next);
                    })
                }}
            >
                {"Add trait"}
            </button>
        </div>
    }
}

fn scenario_opening_fields(draft: &UseStateHandle<ScenarioDraft>) -> Html {
    let draft = draft.clone();
    html! {
        <label class="field">
            <span class="muted">{"Opening message"}</span>
            <div class="composer-input-stack">
                <textarea
                    class="composer-input-stack__primary input"
                    rows="4"
                    placeholder="Opening narration or first player action"
                    value={draft.opening_message.clone()}
                    oninput={{
                        let draft = draft.clone();
                        Callback::from(move |e: InputEvent| {
                            let input: HtmlTextAreaElement = e.target_unchecked_into();
                            let mut next = (*draft).clone();
                            next.opening_message = input.value();
                            draft.set(next);
                        })
                    }}
                />
                <textarea
                    class="composer-input-stack__secondary input"
                    rows="2"
                    placeholder="Optional guidance for the GM"
                    value={draft.opening_guidance.clone()}
                    oninput={{
                        let draft = draft.clone();
                        Callback::from(move |e: InputEvent| {
                            let input: HtmlTextAreaElement = e.target_unchecked_into();
                            let mut next = (*draft).clone();
                            next.opening_guidance = input.value();
                            draft.set(next);
                        })
                    }}
                />
            </div>
            <span class="muted scenario-field-hint">
                {"GM guidance is applied to the auto-submitted first turn when you start a game from this scenario."}
            </span>
        </label>
    }
}

fn scenario_fields(draft: &UseStateHandle<ScenarioDraft>) -> Html {
    let fields_before_tone = [
        ("title", "Title", false),
        ("premise", "Premise / scenario", true),
        ("objective", "Objective", true),
    ];
    let fields_after_tone = [
        ("setting", "Setting / world", true),
        ("gm_style", "GM style", true),
        ("setup_text", "Setup instructions", true),
        ("pc_name", "Default PC name", false),
        ("pc_description", "Default PC description", true),
    ];
    html! {
        <>
            { for fields_before_tone.iter().map(|(key, label, multiline)| {
                let key = *key;
                let draft = draft.clone();
                html! {
                    <label class="field">
                        <span class="muted">{ *label }</span>
                        if *multiline {
                            <textarea value={scenario_draft_field(draft.clone(), key)} oninput={scenario_draft_oninput(draft, key)} />
                        } else {
                            <input type="text" value={scenario_draft_field(draft.clone(), key)} oninput={scenario_draft_oninput(draft, key)} />
                        }
                    </label>
                }
            }) }
            { scenario_opening_fields(draft) }
            <GmTonePresetPicker on_apply={Callback::from({
                let draft = draft.clone();
                move |(setting, gm_style)| {
                    let mut next = (*draft).clone();
                    next.setting = setting;
                    next.gm_style = gm_style;
                    draft.set(next);
                }
            })} />
            { for fields_after_tone.iter().map(|(key, label, multiline)| {
                let key = *key;
                let draft = draft.clone();
                html! {
                    <label class="field">
                        <span class="muted">{ *label }</span>
                        if *multiline {
                            <textarea value={scenario_draft_field(draft.clone(), key)} oninput={scenario_draft_oninput(draft, key)} />
                        } else {
                            <input type="text" value={scenario_draft_field(draft.clone(), key)} oninput={scenario_draft_oninput(draft, key)} />
                        }
                    </label>
                }
            }) }
        </>
    }
}

fn scenario_draft_field(draft: UseStateHandle<ScenarioDraft>, key: &str) -> String {
    match key {
        "title" => draft.title.clone(),
        "opening_message" => draft.opening_message.clone(),
        "opening_guidance" => draft.opening_guidance.clone(),
        "premise" => draft.premise.clone(),
        "setting" => draft.setting.clone(),
        "gm_style" => draft.gm_style.clone(),
        "pc_name" => draft.pc_name.clone(),
        "pc_description" => draft.pc_description.clone(),
        "objective" => draft.objective.clone(),
        "setup_text" => draft.setup_text.clone(),
        _ => String::new(),
    }
}

fn scenario_draft_oninput(draft: UseStateHandle<ScenarioDraft>, key: &str) -> Callback<InputEvent> {
    let key = key.to_string();
    Callback::from(move |e: InputEvent| {
        let input: HtmlInputElement = e.target_unchecked_into();
        let value = input.value();
        let mut next = (*draft).clone();
        match key.as_str() {
            "title" => next.title = value,
            "opening_message" => next.opening_message = value,
            "opening_guidance" => next.opening_guidance = value,
            "premise" => next.premise = value,
            "setting" => next.setting = value,
            "gm_style" => next.gm_style = value,
            "pc_name" => next.pc_name = value,
            "pc_description" => next.pc_description = value,
            "objective" => next.objective = value,
            "setup_text" => next.setup_text = value,
            _ => {}
        }
        draft.set(next);
    })
}

fn confirm_scenario_delete(name: &str) -> bool {
    web_sys::window()
        .and_then(|w| {
            w.confirm_with_message(&format!(
                "Delete scenario \"{name}\"? Existing games keep their saved world text."
            ))
            .ok()
        })
        .unwrap_or(false)
}

pub fn default_game_title(scenario_title: &str, scenario_id: i64, games: &[Game]) -> String {
    let same = games
        .iter()
        .filter(|game| game.scenario_id == Some(scenario_id))
        .count();
    if same == 0 {
        scenario_title.to_string()
    } else {
        format!("{scenario_title} ({})", same + 1)
    }
}

pub fn scenario_opening_as_player_action(scenario: &Scenario) -> bool {
    if scenario.pc_options.is_empty() {
        return false;
    }
    !scenario.opening_message.trim().is_empty() || !scenario.opening_guidance.trim().is_empty()
}

pub fn sorted_trait_rows(traits: &HashMap<String, i64>) -> Vec<(String, i64)> {
    let mut rows: Vec<_> = traits.iter().map(|(k, v)| (k.clone(), *v)).collect();
    rows.sort_by(|left, right| left.0.cmp(&right.0));
    rows
}

pub fn traits_from_rows(rows: &[(String, i64)]) -> HashMap<String, i64> {
    let mut traits = HashMap::new();
    for (name, value) in rows {
        let name = name.trim();
        if name.is_empty() {
            continue;
        }
        traits.insert(name.to_string(), *value);
    }
    traits
}

pub fn game_create_from_scenario(scenario: &Scenario, title: String) -> GameCreate {
    let opening_as_player_action = scenario_opening_as_player_action(scenario);
    GameCreate {
        title,
        premise: scenario.premise.clone(),
        setting: scenario.setting.clone(),
        gm_style: scenario.gm_style.clone(),
        opening_message: scenario.opening_message.clone(),
        opening_guidance: scenario.opening_guidance.clone(),
        character_id: scenario.character_id,
        scenario_id: Some(scenario.id),
        pc_name: scenario.pc_name.clone(),
        pc_description: scenario.pc_description.clone(),
        pc_traits: scenario.traits.clone(),
        rules_blocks: scenario.rules_blocks.clone(),
        state_schema: merge_game_state_schema(
            &scenario.state_schema,
            &scenario.pc_initial_state,
            &[],
        ),
        win_condition: scenario.win_condition.clone(),
        scenario_triggers: scenario.scenario_triggers.clone(),
        trait_defs: scenario.trait_defs.clone(),
        game_elements: scenario.game_elements.clone(),
        opening_as_player_action,
        ..Default::default()
    }
}

async fn download_scenario_export(id: i64) -> Result<(), String> {
    use wasm_bindgen::JsCast;
    use web_sys::{Blob, BlobPropertyBag, HtmlAnchorElement, Url};

    let export = api::export_scenario(id).await?;
    let json = serde_json::to_string_pretty(&export).map_err(|e| e.to_string())?;
    let window = web_sys::window().ok_or_else(|| "no window".to_string())?;
    let document = window.document().ok_or_else(|| "no document".to_string())?;
    let parts = js_sys::Array::new();
    parts.push(&wasm_bindgen::JsValue::from_str(&json));
    let bag = BlobPropertyBag::new();
    bag.set_type("application/json");
    let blob = Blob::new_with_str_sequence_and_options(&parts, &bag)
        .map_err(|_| "blob failed".to_string())?;
    let url = Url::create_object_url_with_blob(&blob).map_err(|_| "url failed".to_string())?;
    let anchor = document
        .create_element("a")
        .map_err(|_| "anchor failed".to_string())?
        .dyn_into::<HtmlAnchorElement>()
        .map_err(|_| "anchor cast failed".to_string())?;
    anchor.set_href(&url);
    anchor.set_download(&export_filename(&export.scenario.title));
    anchor.click();
    let _ = Url::revoke_object_url(&url);
    Ok(())
}

fn export_filename(title: &str) -> String {
    let slug: String = title
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() {
                ch.to_ascii_lowercase()
            } else {
                '-'
            }
        })
        .collect();
    let slug = slug.trim_matches('-');
    if slug.is_empty() {
        "scenario.json".to_string()
    } else {
        format!("{slug}.json")
    }
}
