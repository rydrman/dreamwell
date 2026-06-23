use dreamwell_types::*;
use std::collections::HashMap;
use web_sys::HtmlInputElement;
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
                <p class="header-subtitle muted">{"Create, edit, and import world or scenario cards, then play them as games."}</p>
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
    let iw_file_input = use_node_ref();

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
                <button class="btn secondary" onclick={{
                    let iw_file_input = iw_file_input.clone();
                    Callback::from(move |_| {
                        if let Some(input) = iw_file_input.cast::<HtmlInputElement>() {
                            input.click();
                        }
                    })
                }}>{"Import IW JSON"}</button>
                <input type="file" accept=".json" ref={iw_file_input} style="display:none;" onchange={{
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
                                if let Ok(scenario) = api::import_iw_scenario(&file).await {
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
                                if  scenario.content_flags.mature {
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
            { scenario_iw_sections(&draft) }
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
struct ScenarioDraft {
    title: String,
    premise: String,
    setting: String,
    gm_style: String,
    opening_message: String,
    pc_name: String,
    pc_description: String,
    trait_rows: Vec<(String, i64)>,
    objective: String,
    setup_text: String,
    rules_blocks: Vec<RulesBlock>,
    cast: Vec<ScenarioNpc>,
    trait_defs: Vec<TraitDef>,
    pc_options: Vec<PcOption>,
    state_schema: Vec<TrackedVarDef>,
    content_flags: ContentFlags,
    win_condition: Option<WinCondition>,
    scenario_triggers: Vec<ScenarioTrigger>,
    source_meta: Option<SourceMeta>,
    game_elements: GameElementsConfig,
}

impl Default for ScenarioDraft {
    fn default() -> Self {
        Self {
            title: String::new(),
            premise: String::new(),
            setting: String::new(),
            gm_style: String::new(),
            opening_message: String::new(),
            pc_name: String::new(),
            pc_description: String::new(),
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
        Self {
            title: scenario.title.clone(),
            premise: scenario.premise.clone(),
            setting: scenario.setting.clone(),
            gm_style: scenario.gm_style.clone(),
            opening_message: scenario.opening_message.clone(),
            pc_name: scenario.pc_name.clone(),
            pc_description: scenario.pc_description.clone(),
            trait_rows,
            objective: scenario.objective.clone(),
            setup_text: scenario.setup_text.clone(),
            rules_blocks: scenario.rules_blocks.clone(),
            cast: scenario.cast.clone(),
            trait_defs: scenario.trait_defs.clone(),
            pc_options: scenario.pc_options.clone(),
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
            pc_name: self.pc_name.clone(),
            pc_description: self.pc_description.clone(),
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
            pc_name: Some(self.pc_name.clone()),
            pc_description: Some(self.pc_description.clone()),
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

fn scenario_iw_sections(draft: &UseStateHandle<ScenarioDraft>) -> Html {
    if draft.rules_blocks.is_empty() && draft.cast.is_empty() && draft.trait_defs.is_empty() {
        return html! {};
    }
    html! {
        <div class="scenario-iw-sections">
            if !draft.trait_defs.is_empty() {
                <details class="scenario-iw-panel" open=true>
                    <summary>{"Scenario traits"}</summary>
                    <ul>
                        { for draft.trait_defs.iter().map(|t| html! { <li>{ &t.name }</li> }) }
                    </ul>
                </details>
            }
            if !draft.cast.is_empty() {
                <details class="scenario-iw-panel">
                    <summary>{ format!("Cast ({})", draft.cast.len()) }</summary>
                    <ul>
                        { for draft.cast.iter().map(|npc| html! {
                            <li><strong>{ &npc.name }</strong>{ ": " }{ &npc.content }</li>
                        }) }
                    </ul>
                </details>
            }
            if !draft.rules_blocks.is_empty() {
                <details class="scenario-iw-panel">
                    <summary>{ format!("Rules blocks ({})", draft.rules_blocks.len()) }</summary>
                    { for draft.rules_blocks.iter().map(|block| html! {
                        <details class="rules-block-item" key={block.name.clone()}>
                            <summary>{ &block.name }</summary>
                            <pre class="rules-block-content">{ &block.content }</pre>
                        </details>
                    }) }
                </details>
            }
        </div>
    }
}

fn scenario_traits_editor(draft: &UseStateHandle<ScenarioDraft>) -> Html {
    html! {
        <div class="scenario-traits">
            <div class="scenario-traits-header">
                <span class="muted">{"Traits / roles"}</span>
                <p class="muted scenario-traits-help">{"These names are used for dice checks when you play this scenario. Default sheet modifiers can be negative or positive."}</p>
            </div>
            <div class="scenario-traits-grid">
                { for draft.trait_rows.iter().enumerate().map(|(index, (name, value))| {
                    let name = name.clone();
                    let value = *value;
                    html! {
                        <div class="scenario-trait-row" key={index}>
                            <input
                                type="text"
                                class="input"
                                placeholder="Trait name"
                                value={name.clone()}
                                oninput={{
                                    let draft = draft.clone();
                                    Callback::from(move |e: InputEvent| {
                                        let input: HtmlInputElement = e.target_unchecked_into();
                                        let mut next = (*draft).clone();
                                        if let Some(row) = next.trait_rows.get_mut(index) {
                                            row.0 = input.value();
                                        }
                                        draft.set(next);
                                    })
                                }}
                            />
                            <input
                                type="number"
                                class="input input-compact scenario-trait-mod"
                                value={value.to_string()}
                                oninput={{
                                    let draft = draft.clone();
                                    Callback::from(move |e: InputEvent| {
                                        let input: HtmlInputElement = e.target_unchecked_into();
                                        let parsed = input.value().parse::<i64>().unwrap_or(0);
                                        let mut next = (*draft).clone();
                                        if let Some(row) = next.trait_rows.get_mut(index) {
                                            row.1 = parsed;
                                        }
                                        draft.set(next);
                                    })
                                }}
                            />
                            <button
                                type="button"
                                class="btn secondary btn-compact"
                                onclick={{
                                    let draft = draft.clone();
                                    Callback::from(move |_| {
                                        let mut next = (*draft).clone();
                                        next.trait_rows.remove(index);
                                        draft.set(next);
                                    })
                                }}
                            >
                                {"Remove"}
                            </button>
                        </div>
                    }
                }) }
            </div>
            <button
                type="button"
                class="btn secondary"
                style="margin-top:0.5rem;"
                onclick={{
                    let draft = draft.clone();
                    Callback::from(move |_| {
                        let mut next = (*draft).clone();
                        next.trait_rows.push((String::new(), 0));
                        draft.set(next);
                    })
                }}
            >
                {"Add trait"}
            </button>
        </div>
    }
}

fn scenario_fields(draft: &UseStateHandle<ScenarioDraft>) -> Html {
    let fields_before_tone = [
        ("title", "Title", false),
        ("opening_message", "Opening message", true),
        ("premise", "Premise / scenario", true),
    ];
    let fields_after_tone = [
        ("setting", "Setting / world", true),
        ("gm_style", "GM style", true),
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
        "premise" => draft.premise.clone(),
        "setting" => draft.setting.clone(),
        "gm_style" => draft.gm_style.clone(),
        "pc_name" => draft.pc_name.clone(),
        "pc_description" => draft.pc_description.clone(),
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
            "premise" => next.premise = value,
            "setting" => next.setting = value,
            "gm_style" => next.gm_style = value,
            "pc_name" => next.pc_name = value,
            "pc_description" => next.pc_description = value,
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

pub fn scenario_is_infinite_worlds(scenario: &Scenario) -> bool {
    scenario
        .source_meta
        .as_ref()
        .is_some_and(|meta| meta.platform == "infinite_worlds")
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
    let opening_as_player_action =
        scenario_is_infinite_worlds(scenario) && !scenario.opening_message.trim().is_empty();
    GameCreate {
        title,
        premise: scenario.premise.clone(),
        setting: scenario.setting.clone(),
        gm_style: scenario.gm_style.clone(),
        opening_message: scenario.opening_message.clone(),
        character_id: scenario.character_id,
        scenario_id: Some(scenario.id),
        pc_name: scenario.pc_name.clone(),
        pc_description: scenario.pc_description.clone(),
        pc_traits: scenario.traits.clone(),
        rules_blocks: scenario.rules_blocks.clone(),
        state_schema: scenario.state_schema.clone(),
        win_condition: scenario.win_condition.clone(),
        scenario_triggers: scenario.scenario_triggers.clone(),
        trait_defs: scenario.trait_defs.clone(),
        game_elements: scenario.game_elements.clone(),
        opening_as_player_action,
        ..Default::default()
    }
}
