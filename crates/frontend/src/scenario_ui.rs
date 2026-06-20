use dreamwell_types::*;
use web_sys::HtmlInputElement;
use yew::prelude::*;

use crate::api;

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
                            <span class="list-row-name">{ &scenario.title }</span>
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

#[derive(Clone, Default, PartialEq)]
struct ScenarioDraft {
    title: String,
    premise: String,
    setting: String,
    gm_style: String,
    pc_name: String,
    pc_description: String,
}

impl ScenarioDraft {
    fn from(scenario: &Scenario) -> Self {
        Self {
            title: scenario.title.clone(),
            premise: scenario.premise.clone(),
            setting: scenario.setting.clone(),
            gm_style: scenario.gm_style.clone(),
            pc_name: scenario.pc_name.clone(),
            pc_description: scenario.pc_description.clone(),
        }
    }

    fn to_create(&self) -> ScenarioCreate {
        ScenarioCreate {
            title: self.title.trim().to_string(),
            premise: self.premise.clone(),
            setting: self.setting.clone(),
            gm_style: self.gm_style.clone(),
            pc_name: self.pc_name.clone(),
            pc_description: self.pc_description.clone(),
            character_id: None,
        }
    }

    fn to_update(&self) -> ScenarioUpdate {
        ScenarioUpdate {
            title: Some(self.title.trim().to_string()),
            premise: Some(self.premise.clone()),
            setting: Some(self.setting.clone()),
            gm_style: Some(self.gm_style.clone()),
            pc_name: Some(self.pc_name.clone()),
            pc_description: Some(self.pc_description.clone()),
            character_id: None,
        }
    }
}

fn scenario_fields(draft: &UseStateHandle<ScenarioDraft>) -> Html {
    let fields = [
        ("title", "Title", false),
        ("premise", "Premise / scenario", true),
        ("setting", "Setting / world", true),
        ("gm_style", "GM style", true),
        ("pc_name", "Default PC name", false),
        ("pc_description", "Default PC description", true),
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

pub fn game_create_from_scenario(scenario: &Scenario, title: String) -> GameCreate {
    GameCreate {
        title,
        premise: scenario.premise.clone(),
        setting: scenario.setting.clone(),
        gm_style: scenario.gm_style.clone(),
        character_id: scenario.character_id,
        scenario_id: Some(scenario.id),
        pc_name: scenario.pc_name.clone(),
        pc_description: scenario.pc_description.clone(),
    }
}
