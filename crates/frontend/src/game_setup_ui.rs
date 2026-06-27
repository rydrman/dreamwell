use dreamwell_types::*;
use std::collections::HashMap;
use web_sys::HtmlSelectElement;
use yew::prelude::*;

use crate::scenario_ui::{game_create_from_scenario, sorted_trait_rows, traits_from_rows};

#[derive(Properties, PartialEq)]
pub struct GameSetupWizardProps {
    pub scenario: Scenario,
    pub games: Vec<Game>,
    pub on_close: Callback<()>,
    pub on_create: Callback<GameCreate>,
}

fn setup_var_label(key: &str) -> &str {
    match key {
        "Character1" => "Friend 1",
        "Character2" => "Friend 2",
        "Character3" => "Friend 3",
        other => other,
    }
}

#[function_component(GameSetupWizard)]
pub fn game_setup_wizard(props: &GameSetupWizardProps) -> Html {
    let pc_index = use_state(|| 0usize);
    let pc_name = use_state(String::new);
    let pc_description = use_state(String::new);
    let trait_rows = use_state(Vec::<(String, i64)>::new);
    let setup_values = use_state(HashMap::<String, String>::new);
    let cast_picks = use_state(Vec::<String>::new);

    let pc_options = props.scenario.pc_options.clone();
    let selected_pc = pc_options.get(*pc_index).cloned();

    {
        let setup_values = setup_values.clone();
        let cast_picks = cast_picks.clone();
        let pc_name = pc_name.clone();
        let pc_description = pc_description.clone();
        let trait_rows = trait_rows.clone();
        let pc_index = *pc_index;
        let scenario = props.scenario.clone();
        use_effect_with(pc_index, move |_| {
            let mut values = HashMap::new();
            let mut picks = Vec::new();
            if let Some(pc) = scenario.pc_options.get(pc_index) {
                pc_name.set(pc.name.clone());
                pc_description.set(pc.description.clone());
                trait_rows.set(sorted_trait_rows(&pc.traits));
                for var in &pc.setup_vars {
                    if let Some(first) = var.options.first() {
                        values.insert(var.key.clone(), first.clone());
                    }
                }
                for var in &pc.setup_vars {
                    if var.key.starts_with("Character") && var.key.len() <= 11 {
                        if let Some(name) = var.options.first() {
                            picks.push(name.clone());
                        }
                    }
                }
            } else {
                pc_name.set(scenario.pc_name.clone());
                pc_description.set(scenario.pc_description.clone());
                trait_rows.set(sorted_trait_rows(&scenario.traits));
            }
            setup_values.set(values);
            cast_picks.set(picks);
            || ()
        });
    }

    let on_submit = {
        let scenario = props.scenario.clone();
        let games = props.games.clone();
        let setup_values = (*setup_values).clone();
        let cast_picks = (*cast_picks).clone();
        let pc_name = (*pc_name).clone();
        let pc_description = (*pc_description).clone();
        let trait_rows = (*trait_rows).clone();
        let pc_index = *pc_index;
        let on_create = props.on_create.clone();
        let on_close = props.on_close.clone();
        Callback::from(move |_| {
            let invited_cast: Vec<ScenarioNpc> = scenario
                .cast
                .iter()
                .filter(|npc| cast_picks.iter().any(|n| n == &npc.name))
                .cloned()
                .collect();
            let pc_state = scenario
                .pc_options
                .get(pc_index)
                .map(|pc| pc.initial_state.as_slice())
                .unwrap_or(scenario.pc_initial_state.as_slice());
            let title =
                crate::scenario_ui::default_game_title(&scenario.title, scenario.id, &games);
            let mut payload = game_create_from_scenario(&scenario, title);
            payload.pc_name = pc_name.trim().to_string();
            payload.pc_description = pc_description.clone();
            payload.pc_traits = traits_from_rows(&trait_rows);
            payload.setup_var_values = setup_values.clone();
            payload.cast_selections = cast_picks.clone();
            payload.invited_cast = invited_cast.clone();
            payload.state_schema = merge_game_state_schema(
                &scenario.state_schema,
                pc_state,
                &scenario.cast_uniform_state,
                &invited_cast,
            );
            on_create.emit(payload);
            on_close.emit(());
        })
    };

    html! {
        <div class="modal-backdrop" onclick={props.on_close.reform(|_| ())}>
            <div class="modal modal-wide" onclick={Callback::from(|e: MouseEvent| e.stop_propagation())}>
                <div class="settings-header">
                    <h2>{"Game setup"}</h2>
                    <button type="button" class="btn secondary" onclick={props.on_close.reform(|_| ())}>{"Cancel"}</button>
                </div>
                <div class="panel-overlay-body">
                    if !props.scenario.setup_text.is_empty() {
                        <div class="field">
                            <span class="muted">{"Setup"}</span>
                            <div class="markdown-body setup-text">{ &props.scenario.setup_text }</div>
                        </div>
                    }
                    if !pc_options.is_empty() {
                        <label class="field">
                            <span class="muted">{"Player character template"}</span>
                            <select onchange={{
                                let pc_index = pc_index.clone();
                                Callback::from(move |e: Event| {
                                    let select: HtmlSelectElement = e.target_unchecked_into();
                                    if let Ok(idx) = select.value().parse::<usize>() {
                                        pc_index.set(idx);
                                    }
                                })
                            }}>
                                { for pc_options.iter().enumerate().map(|(i, pc)| {
                                    html! { <option value={i.to_string()} selected={i == *pc_index}>{ &pc.name }</option> }
                                }) }
                            </select>
                        </label>
                    }
                    <label class="field">
                        <span class="muted">{"Character name"}</span>
                        <input
                            type="text"
                            value={(*pc_name).clone()}
                            oninput={{
                                let pc_name = pc_name.clone();
                                Callback::from(move |e: InputEvent| {
                                    let input: web_sys::HtmlInputElement = e.target_unchecked_into();
                                    pc_name.set(input.value());
                                })
                            }}
                        />
                    </label>
                    <label class="field">
                        <span class="muted">{"Character description"}</span>
                        <textarea
                            rows="6"
                            value={(*pc_description).clone()}
                            oninput={{
                                let pc_description = pc_description.clone();
                                Callback::from(move |e: InputEvent| {
                                    let input: web_sys::HtmlTextAreaElement = e.target_unchecked_into();
                                    pc_description.set(input.value());
                                })
                            }}
                        />
                    </label>
                    if !trait_rows.is_empty() || !props.scenario.trait_defs.is_empty() {
                        <div class="field">
                            <span class="muted">{"Traits"}</span>
                            <div class="trait-rows">
                                { for trait_rows.iter().enumerate().map(|(index, (name, value))| {
                                    html! {
                                        <div class="trait-row" key={index}>
                                            <input
                                                type="text"
                                                value={name.clone()}
                                                placeholder="Trait"
                                                oninput={{
                                                    let trait_rows = trait_rows.clone();
                                                    Callback::from(move |e: InputEvent| {
                                                        let input: web_sys::HtmlInputElement = e.target_unchecked_into();
                                                        let mut next = (*trait_rows).clone();
                                                        if let Some(row) = next.get_mut(index) {
                                                            row.0 = input.value();
                                                        }
                                                        trait_rows.set(next);
                                                    })
                                                }}
                                            />
                                            <input
                                                type="number"
                                                value={value.to_string()}
                                                oninput={{
                                                    let trait_rows = trait_rows.clone();
                                                    Callback::from(move |e: InputEvent| {
                                                        let input: web_sys::HtmlInputElement = e.target_unchecked_into();
                                                        let mut next = (*trait_rows).clone();
                                                        if let Some(row) = next.get_mut(index) {
                                                            row.1 = input.value().parse().unwrap_or(0);
                                                        }
                                                        trait_rows.set(next);
                                                    })
                                                }}
                                            />
                                            <button
                                                type="button"
                                                class="btn secondary"
                                                onclick={{
                                                    let trait_rows = trait_rows.clone();
                                                    Callback::from(move |_| {
                                                        let mut next = (*trait_rows).clone();
                                                        next.remove(index);
                                                        trait_rows.set(next);
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
                                onclick={{
                                    let trait_rows = trait_rows.clone();
                                    Callback::from(move |_| {
                                        let mut next = (*trait_rows).clone();
                                        next.push((String::new(), 0));
                                        trait_rows.set(next);
                                    })
                                }}
                            >
                                {"Add trait"}
                            </button>
                        </div>
                    }
                    if let Some(pc) = selected_pc {
                        { for pc.setup_vars.iter().map(|var| {
                            let key = var.key.clone();
                            let current = setup_values.get(&key).cloned().unwrap_or_default();
                            html! {
                                <label class="field" key={key.clone()}>
                                    <span class="muted">{ setup_var_label(&key) }</span>
                                    <select onchange={{
                                        let setup_values = setup_values.clone();
                                        let cast_picks = cast_picks.clone();
                                        let key = key.clone();
                                        Callback::from(move |e: Event| {
                                            let select: HtmlSelectElement = e.target_unchecked_into();
                                            let value = select.value();
                                            let mut next = (*setup_values).clone();
                                            next.insert(key.clone(), value.clone());
                                            setup_values.set(next);
                                            if key.starts_with("Character") && key.len() <= 11 {
                                                let mut picks = (*cast_picks).clone();
                                                let slot = key
                                                    .trim_start_matches("Character")
                                                    .parse::<usize>()
                                                    .ok()
                                                    .and_then(|n| n.checked_sub(1));
                                                if let Some(idx) = slot {
                                                    match idx.cmp(&picks.len()) {
                                                        std::cmp::Ordering::Less => {
                                                            picks[idx] = value;
                                                        }
                                                        std::cmp::Ordering::Equal => {
                                                            picks.push(value);
                                                        }
                                                        std::cmp::Ordering::Greater => {}
                                                    }
                                                    cast_picks.set(picks);
                                                }
                                            }
                                        })
                                    }}>
                                        { for var.options.iter().map(|opt| {
                                            html! { <option value={opt.clone()} selected={opt == &current}>{ opt }</option> }
                                        }) }
                                    </select>
                                </label>
                            }
                        }) }
                    }
                    if !props.scenario.cast.is_empty() {
                        <div class="field">
                            <span class="muted">{"Invited friends"}</span>
                            <div class="setup-cast-grid">
                                { for props.scenario.cast.iter().map(|npc| {
                                    let name = npc.name.clone();
                                    let selected = cast_picks.contains(&name);
                                    html! {
                                        <label class="setup-cast-option" key={name.clone()}>
                                            <input
                                                type="checkbox"
                                                checked={selected}
                                                onchange={{
                                                    let cast_picks = cast_picks.clone();
                                                    let name = name.clone();
                                                    Callback::from(move |e: Event| {
                                                        let input: web_sys::HtmlInputElement = e.target_unchecked_into();
                                                        let mut next = (*cast_picks).clone();
                                                        if input.checked() {
                                                            if !next.contains(&name) {
                                                                next.push(name.clone());
                                                            }
                                                        } else {
                                                            next.retain(|n| n != &name);
                                                        }
                                                        cast_picks.set(next);
                                                    })
                                                }}
                                            />
                                            <span>{ &npc.name }</span>
                                        </label>
                                    }
                                }) }
                            </div>
                        </div>
                    }
                    <button type="button" class="btn" onclick={on_submit} disabled={pc_name.trim().is_empty()}>
                        {"Start game"}
                    </button>
                </div>
            </div>
        </div>
    }
}

pub fn scenario_needs_setup(scenario: &Scenario) -> bool {
    !scenario.setup_text.is_empty() || !scenario.pc_options.is_empty()
}
