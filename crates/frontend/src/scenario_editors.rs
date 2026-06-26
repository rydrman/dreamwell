use dreamwell_types::*;
use std::collections::HashMap;
use web_sys::HtmlInputElement;
use yew::prelude::*;

use crate::scenario_state_ui::{
    editable_character_state_to_saved, editable_tracked_var_to_saved, text_input, textarea_input,
    EditableCharacterStateDef, EditableTrackedVarDef, ScenarioStateDefEditor,
    ScenarioStateFieldUpdate,
};
use crate::scenario_ui::{
    add_trait_column, remove_trait_column, rename_trait_column, synced_trait_values_row,
    trait_column_names, traits_for_columns, DraftPcOption, DraftScenarioNpc, ScenarioDraft,
    TraitRowOwner,
};

fn mutate_draft(draft: &UseStateHandle<ScenarioDraft>, f: impl FnOnce(&mut ScenarioDraft)) {
    let mut next = (**draft).clone();
    f(&mut next);
    draft.set(next);
}

#[derive(Clone, PartialEq)]
enum CharacterStateOwner {
    DefaultPc,
    CastNpc(usize),
    PcOption(usize),
}

#[derive(Properties, PartialEq)]
struct CharacterStateEditorProps {
    draft: UseStateHandle<ScenarioDraft>,
    owner: CharacterStateOwner,
    label: &'static str,
}

fn character_state_entries(
    draft: &ScenarioDraft,
    owner: CharacterStateOwner,
) -> Vec<EditableCharacterStateDef> {
    match owner {
        CharacterStateOwner::DefaultPc => draft.pc_initial_state.clone(),
        CharacterStateOwner::CastNpc(index) => draft
            .cast
            .get(index)
            .map(|npc| npc.initial_state.clone())
            .unwrap_or_default(),
        CharacterStateOwner::PcOption(index) => draft
            .pc_options
            .get(index)
            .map(|pc| pc.initial_state.clone())
            .unwrap_or_default(),
    }
}

fn character_state_set_all(
    draft: &mut ScenarioDraft,
    owner: &CharacterStateOwner,
    state: Vec<CharacterStateDef>,
) {
    let editable = EditableCharacterStateDef::from_saved_vec(&state);
    match owner {
        CharacterStateOwner::DefaultPc => draft.pc_initial_state = editable,
        CharacterStateOwner::CastNpc(index) => {
            if let Some(npc) = draft.cast.get_mut(*index) {
                npc.initial_state = editable;
            }
        }
        CharacterStateOwner::PcOption(index) => {
            if let Some(pc) = draft.pc_options.get_mut(*index) {
                pc.initial_state = editable;
            }
        }
    }
}

fn build_generate_character_state_request(
    draft: &ScenarioDraft,
    owner: &CharacterStateOwner,
) -> Option<GenerateCharacterStateRequest> {
    let (role, name, description, traits, existing_state) = match owner {
        CharacterStateOwner::DefaultPc => {
            let mut traits = HashMap::new();
            for (name, value) in &draft.trait_rows {
                let name = name.trim();
                if name.is_empty() {
                    continue;
                }
                traits.insert(name.to_string(), *value);
            }
            (
                "pc".to_string(),
                draft.pc_name.clone(),
                draft.pc_description.clone(),
                traits,
                editable_character_state_to_saved(&draft.pc_initial_state, "Default PC state")
                    .unwrap_or_default(),
            )
        }
        CharacterStateOwner::CastNpc(index) => {
            let npc = draft.cast.get(*index)?;
            (
                "npc".to_string(),
                npc.name.clone(),
                npc.content.clone(),
                npc.traits.clone(),
                editable_character_state_to_saved(&npc.initial_state, "Cast state")
                    .unwrap_or_default(),
            )
        }
        CharacterStateOwner::PcOption(index) => {
            let pc = draft.pc_options.get(*index)?;
            (
                "pc_option".to_string(),
                pc.name.clone(),
                pc.description.clone(),
                pc.traits.clone(),
                editable_character_state_to_saved(&pc.initial_state, "PC option state")
                    .unwrap_or_default(),
            )
        }
    };
    if name.trim().is_empty() {
        return None;
    }
    Some(GenerateCharacterStateRequest {
        title: draft.title.clone(),
        premise: draft.premise.clone(),
        setting: draft.setting.clone(),
        gm_style: draft.gm_style.clone(),
        objective: draft.objective.clone(),
        state_schema: editable_tracked_var_to_saved(&draft.state_schema, "World state schema")
            .unwrap_or_default(),
        cast: draft
            .cast
            .iter()
            .enumerate()
            .filter_map(|(index, npc)| npc.to_saved(&format!("Cast entry {}", index + 1)).ok())
            .collect(),
        character: GenerateCharacterStateTarget {
            role,
            name,
            description,
            traits,
        },
        existing_state,
    })
}

#[function_component(CharacterStateEditor)]
fn character_state_editor(props: &CharacterStateEditorProps) -> Html {
    let generating = use_state(|| false);
    let draft = props.draft.clone();
    let owner = props.owner.clone();
    let label = props.label;
    let entries = character_state_entries(&draft, owner.clone());
    let on_generate = {
        let draft = props.draft.clone();
        let owner = props.owner.clone();
        let generating = generating.clone();
        Callback::from(move |_| {
            let snapshot = (*draft).clone();
            let Some(request) = build_generate_character_state_request(&snapshot, &owner) else {
                if let Some(window) = web_sys::window() {
                    let _ = window
                        .alert_with_message("Enter a character name before generating state.");
                }
                return;
            };
            generating.set(true);
            let generating_done = generating.clone();
            let draft = draft.clone();
            let owner = owner.clone();
            wasm_bindgen_futures::spawn_local(async move {
                let result = crate::api::generate_character_state(&request).await;
                generating_done.set(false);
                match result {
                    Ok(response) => {
                        mutate_draft(&draft, |d| {
                            character_state_set_all(d, &owner, response.initial_state);
                        });
                    }
                    Err(err) => {
                        if let Some(window) = web_sys::window() {
                            let _ = window
                                .alert_with_message(&format!("Could not generate state: {err}"));
                        }
                    }
                }
            });
        })
    };
    html! {
        <div class="scenario-character-state">
            <div class="scenario-editor-row">
                <span class="muted">{ format!("{label} ({})", entries.len()) }</span>
                <button
                    type="button"
                    class="btn secondary btn-compact"
                    disabled={*generating}
                    onclick={on_generate}
                >
                    if *generating {
                        {"Generating…"}
                    } else {
                        {"Generate with LLM"}
                    }
                </button>
            </div>
            { for entries.iter().enumerate().map(|(index, def)| {
                let owner = owner.clone();
                let draft = props.draft.clone();
                let view = def.to_view();
                html! {
                    <div class="scenario-editor-block scenario-editor-block-nested" key={index}>
                        <ScenarioStateDefEditor
                            view={view}
                            on_update={Callback::from({
                                let draft = draft.clone();
                                let owner = owner.clone();
                                move |update: ScenarioStateFieldUpdate| mutate_draft(&draft, |d| {
                                    if let Some(row) = character_state_mut(d, &owner, index) {
                                        row.apply_update(update);
                                    }
                                })
                            })}
                            on_remove={Callback::from({
                                let draft = draft.clone();
                                let owner = owner.clone();
                                move |_| mutate_draft(&draft, |d| {
                                    character_state_remove(d, &owner, index);
                                })
                            })}
                        />
                    </div>
                }
            }) }
            <button type="button" class="btn secondary btn-compact" onclick={{
                let draft = props.draft.clone();
                let owner = owner.clone();
                Callback::from(move |_| mutate_draft(&draft, |d| {
                    character_state_push(d, &owner, EditableCharacterStateDef::default());
                }))
            }}>{"Add state variable"}</button>
        </div>
    }
}

fn character_state_mut<'a>(
    draft: &'a mut ScenarioDraft,
    owner: &CharacterStateOwner,
    index: usize,
) -> Option<&'a mut EditableCharacterStateDef> {
    match owner {
        CharacterStateOwner::DefaultPc => draft.pc_initial_state.get_mut(index),
        CharacterStateOwner::CastNpc(npc_index) => draft
            .cast
            .get_mut(*npc_index)
            .and_then(|npc| npc.initial_state.get_mut(index)),
        CharacterStateOwner::PcOption(pc_index) => draft
            .pc_options
            .get_mut(*pc_index)
            .and_then(|pc| pc.initial_state.get_mut(index)),
    }
}

fn character_state_remove(draft: &mut ScenarioDraft, owner: &CharacterStateOwner, index: usize) {
    match owner {
        CharacterStateOwner::DefaultPc => {
            draft.pc_initial_state.remove(index);
        }
        CharacterStateOwner::CastNpc(npc_index) => {
            if let Some(npc) = draft.cast.get_mut(*npc_index) {
                npc.initial_state.remove(index);
            }
        }
        CharacterStateOwner::PcOption(pc_index) => {
            if let Some(pc) = draft.pc_options.get_mut(*pc_index) {
                pc.initial_state.remove(index);
            }
        }
    }
}

fn character_state_push(
    draft: &mut ScenarioDraft,
    owner: &CharacterStateOwner,
    def: EditableCharacterStateDef,
) {
    match owner {
        CharacterStateOwner::DefaultPc => draft.pc_initial_state.push(def),
        CharacterStateOwner::CastNpc(npc_index) => {
            if let Some(npc) = draft.cast.get_mut(*npc_index) {
                npc.initial_state.push(def);
            }
        }
        CharacterStateOwner::PcOption(pc_index) => {
            if let Some(pc) = draft.pc_options.get_mut(*pc_index) {
                pc.initial_state.push(def);
            }
        }
    }
}

fn default_pc_state_editor(draft: &UseStateHandle<ScenarioDraft>) -> Html {
    html! {
        <details class="scenario-extra-panel">
            <summary>{ format!("Default PC state ({})", draft.pc_initial_state.len()) }</summary>
            <p class="muted">{"Typed resources, conditions, facts, and clocks seeded on the default player character when a game starts."}</p>
            <CharacterStateEditor draft={draft.clone()} owner={CharacterStateOwner::DefaultPc} label="State variables" />
        </details>
    }
}

pub fn scenario_advanced_editors(draft: &UseStateHandle<ScenarioDraft>) -> Html {
    html! {
        <div class="scenario-advanced-sections">
            { trait_defs_editor(draft) }
            { default_pc_state_editor(draft) }
            { cast_editor(draft) }
            { rules_blocks_editor(draft) }
            { pc_options_editor(draft) }
            { state_schema_editor(draft) }
            { win_condition_editor(draft) }
            { content_flags_editor(draft) }
            { scenario_triggers_editor(draft) }
            { game_elements_editor(draft) }
            { source_meta_panel(draft) }
        </div>
    }
}

fn trait_defs_editor(draft: &UseStateHandle<ScenarioDraft>) -> Html {
    html! {
        <details class="scenario-extra-panel">
            <summary>{ format!("Trait definitions ({})", draft.trait_defs.len()) }</summary>
            <p class="muted scenario-traits-help">{"Define custom trait names and descriptions. Names and values stay synced with the traits matrix."}</p>
            { for draft.trait_defs.iter().enumerate().map(|(index, def)| {
                let name = def.name.clone();
                let description = def.description.clone();
                html! {
                    <div class="scenario-editor-row" key={index}>
                        <input type="text" class="input" placeholder="Trait name" value={name.clone()} oninput={{
                            let draft = draft.clone();
                            Callback::from(move |e: InputEvent| {
                                let input: HtmlInputElement = e.target_unchecked_into();
                                mutate_draft(&draft, |d| {
                                    rename_trait_column(d, index, input.value());
                                });
                            })
                        }} />
                        <input type="text" class="input" placeholder="Description" value={description} oninput={{
                            let draft = draft.clone();
                            Callback::from(move |e: InputEvent| {
                                let input: HtmlInputElement = e.target_unchecked_into();
                                mutate_draft(&draft, |d| {
                                    if let Some(row) = d.trait_defs.get_mut(index) {
                                        row.description = input.value();
                                    }
                                });
                            })
                        }} />
                        <button type="button" class="btn secondary btn-compact" onclick={{
                            let draft = draft.clone();
                            Callback::from(move |_| mutate_draft(&draft, |d| { remove_trait_column(d, index); }))
                        }}>{"Remove"}</button>
                    </div>
                }
            }) }
            <button type="button" class="btn secondary" onclick={{
                let draft = draft.clone();
                Callback::from(move |_| mutate_draft(&draft, |d| {
                    if d.trait_defs.is_empty() && !d.trait_rows.is_empty() {
                        for (name, _) in d.trait_rows.clone() {
                            d.trait_defs.push(TraitDef {
                                name,
                                description: String::new(),
                            });
                        }
                    }
                    add_trait_column(d);
                }))
            }}>{"Add trait definition"}</button>
        </details>
    }
}

fn cast_editor(draft: &UseStateHandle<ScenarioDraft>) -> Html {
    html! {
        <details class="scenario-extra-panel">
            <summary>{ format!("Cast ({})", draft.cast.len()) }</summary>
            <p class="muted">{"NPCs the player can invite during game setup."}</p>
            { for draft.cast.iter().enumerate().map(|(index, npc)| {
                let name = npc.name.clone();
                let content = npc.content.clone();
                let keywords = npc.keywords.join(", ");
                html! {
                    <div class="scenario-editor-block" key={index}>
                        { text_input("Name", &name, {
                            let draft = draft.clone();
                            Callback::from(move |value: String| mutate_draft(&draft, |d| {
                                if let Some(row) = d.cast.get_mut(index) { row.name = value; }
                            }))
                        }) }
                        { textarea_input("Description", &content, {
                            let draft = draft.clone();
                            Callback::from(move |value: String| mutate_draft(&draft, |d| {
                                if let Some(row) = d.cast.get_mut(index) { row.content = value; }
                            }))
                        }) }
                        { text_input("Keywords (comma-separated)", &keywords, {
                            let draft = draft.clone();
                            Callback::from(move |value: String| mutate_draft(&draft, |d| {
                                if let Some(row) = d.cast.get_mut(index) {
                                    row.keywords = value.split(',').map(|s| s.trim().to_string()).filter(|s| !s.is_empty()).collect();
                                }
                            }))
                        }) }
                        <div class="field">
                            <span class="muted">{"Traits"}</span>
                            { synced_trait_values_row(draft, TraitRowOwner::CastNpc(index)) }
                        </div>
                        <CharacterStateEditor draft={draft.clone()} owner={CharacterStateOwner::CastNpc(index)} label="Initial state" />
                        <button type="button" class="btn secondary btn-compact" onclick={{
                            let draft = draft.clone();
                            Callback::from(move |_| mutate_draft(&draft, |d| { d.cast.remove(index); }))
                        }}>{"Remove NPC"}</button>
                    </div>
                }
            }) }
            <button type="button" class="btn secondary" onclick={{
                let draft = draft.clone();
                Callback::from(move |_| mutate_draft(&draft, |d| {
                    let columns = trait_column_names(d);
                    d.cast.push(DraftScenarioNpc {
                        traits: traits_for_columns(&columns),
                        ..DraftScenarioNpc::default()
                    });
                }))
            }}>{"Add NPC"}</button>
        </details>
    }
}

fn rules_blocks_editor(draft: &UseStateHandle<ScenarioDraft>) -> Html {
    html! {
        <details class="scenario-extra-panel">
            <summary>{ format!("Rules blocks ({})", draft.rules_blocks.len()) }</summary>
            <p class="muted">{"Mechanics and house rules injected into the GM prompt each turn."}</p>
            { for draft.rules_blocks.iter().enumerate().map(|(index, block)| {
                let name = block.name.clone();
                let content = block.content.clone();
                html! {
                    <div class="scenario-editor-block" key={index}>
                        { text_input("Block name", &name, {
                            let draft = draft.clone();
                            Callback::from(move |value: String| mutate_draft(&draft, |d| {
                                if let Some(row) = d.rules_blocks.get_mut(index) { row.name = value; }
                            }))
                        }) }
                        { textarea_input("Content", &content, {
                            let draft = draft.clone();
                            Callback::from(move |value: String| mutate_draft(&draft, |d| {
                                if let Some(row) = d.rules_blocks.get_mut(index) { row.content = value; }
                            }))
                        }) }
                        <button type="button" class="btn secondary btn-compact" onclick={{
                            let draft = draft.clone();
                            Callback::from(move |_| mutate_draft(&draft, |d| { d.rules_blocks.remove(index); }))
                        }}>{"Remove block"}</button>
                    </div>
                }
            }) }
            <button type="button" class="btn secondary" onclick={{
                let draft = draft.clone();
                Callback::from(move |_| mutate_draft(&draft, |d| { d.rules_blocks.push(RulesBlock::default()); }))
            }}>{"Add rules block"}</button>
        </details>
    }
}

fn pc_options_editor(draft: &UseStateHandle<ScenarioDraft>) -> Html {
    html! {
        <details class="scenario-extra-panel">
            <summary>{ format!("PC options ({})", draft.pc_options.len()) }</summary>
            <p class="muted">{"Alternate player character templates shown in the pre-play setup wizard."}</p>
            { for draft.pc_options.iter().enumerate().map(|(index, pc)| {
                let name = pc.name.clone();
                let description = pc.description.clone();
                html! {
                    <div class="scenario-editor-block" key={index}>
                        { text_input("Name", &name, {
                            let draft = draft.clone();
                            Callback::from(move |value: String| mutate_draft(&draft, |d| {
                                if let Some(row) = d.pc_options.get_mut(index) { row.name = value; }
                            }))
                        }) }
                        { textarea_input("Description", &description, {
                            let draft = draft.clone();
                            Callback::from(move |value: String| mutate_draft(&draft, |d| {
                                if let Some(row) = d.pc_options.get_mut(index) { row.description = value; }
                            }))
                        }) }
                        <div class="field">
                            <span class="muted">{"Traits"}</span>
                            { synced_trait_values_row(draft, TraitRowOwner::PcOption(index)) }
                        </div>
                        <CharacterStateEditor draft={draft.clone()} owner={CharacterStateOwner::PcOption(index)} label="Initial state" />
                        <button type="button" class="btn secondary btn-compact" onclick={{
                            let draft = draft.clone();
                            Callback::from(move |_| mutate_draft(&draft, |d| { d.pc_options.remove(index); }))
                        }}>{"Remove PC option"}</button>
                    </div>
                }
            }) }
            <button type="button" class="btn secondary" onclick={{
                let draft = draft.clone();
                Callback::from(move |_| mutate_draft(&draft, |d| {
                    let columns = trait_column_names(d);
                    d.pc_options.push(DraftPcOption {
                        traits: traits_for_columns(&columns),
                        ..DraftPcOption::default()
                    });
                }))
            }}>{"Add PC option"}</button>
        </details>
    }
}

fn state_schema_editor(draft: &UseStateHandle<ScenarioDraft>) -> Html {
    html! {
        <details class="scenario-extra-panel">
            <summary>{ format!("State schema ({})", draft.state_schema.len()) }</summary>
            <p class="muted">{"World-level state seeded when a game starts. Per-character state belongs on the default PC, PC options, and cast entries above."}</p>
            { for draft.state_schema.iter().enumerate().map(|(index, def)| {
                let draft = draft.clone();
                let view = def.to_view();
                html! {
                    <div class="scenario-editor-block" key={index}>
                        <ScenarioStateDefEditor
                            view={view}
                            on_update={Callback::from({
                                let draft = draft.clone();
                                move |update: ScenarioStateFieldUpdate| mutate_draft(&draft, |d| {
                                    if let Some(row) = d.state_schema.get_mut(index) {
                                        row.apply_update(update);
                                    }
                                })
                            })}
                            on_remove={Callback::from({
                                let draft = draft.clone();
                                move |_| mutate_draft(&draft, |d| {
                                    d.state_schema.remove(index);
                                })
                            })}
                        />
                    </div>
                }
            }) }
            <button type="button" class="btn secondary" onclick={{
                let draft = draft.clone();
                Callback::from(move |_| mutate_draft(&draft, |d| {
                    d.state_schema.push(EditableTrackedVarDef::default());
                }))
            }}>{"Add state variable"}</button>
        </details>
    }
}

fn win_condition_editor(draft: &UseStateHandle<ScenarioDraft>) -> Html {
    let enabled = draft.win_condition.is_some();
    let condition = draft
        .win_condition
        .as_ref()
        .map(|w| w.condition.clone())
        .unwrap_or_default();
    let epilogue = draft
        .win_condition
        .as_ref()
        .map(|w| w.epilogue_text.clone())
        .unwrap_or_default();
    html! {
        <details class="scenario-extra-panel">
            <summary>{"Win condition"}</summary>
            <label class="field field-inline">
                <input type="checkbox" checked={enabled} onchange={{
                    let draft = draft.clone();
                    Callback::from(move |e: Event| {
                        let input: HtmlInputElement = e.target_unchecked_into();
                        mutate_draft(&draft, |d| {
                            if input.checked() {
                                if d.win_condition.is_none() {
                                    d.win_condition = Some(WinCondition::default());
                                }
                            } else {
                                d.win_condition = None;
                            }
                        });
                    })
                }} />
                <span class="muted">{"Enable win condition"}</span>
            </label>
            if enabled {
                { textarea_input("Condition", &condition, {
                    let draft = draft.clone();
                    Callback::from(move |value: String| mutate_draft(&draft, |d| {
                        if let Some(win) = d.win_condition.as_mut() { win.condition = value; }
                    }))
                }) }
                { textarea_input("Epilogue text", &epilogue, {
                    let draft = draft.clone();
                    Callback::from(move |value: String| mutate_draft(&draft, |d| {
                        if let Some(win) = d.win_condition.as_mut() { win.epilogue_text = value; }
                    }))
                }) }
            }
        </details>
    }
}

fn content_flags_editor(draft: &UseStateHandle<ScenarioDraft>) -> Html {
    let warnings = draft.content_flags.warnings.join(", ");
    html! {
        <details class="scenario-extra-panel">
            <summary>{"Content flags"}</summary>
            <label class="field field-inline">
                <input type="checkbox" checked={draft.content_flags.mature} onchange={{
                    let draft = draft.clone();
                    Callback::from(move |e: Event| {
                        let input: HtmlInputElement = e.target_unchecked_into();
                        mutate_draft(&draft, |d| { d.content_flags.mature = input.checked(); });
                    })
                }} />
                <span class="muted">{"Mature content"}</span>
            </label>
            { text_input("Warnings (comma-separated)", &warnings, {
                let draft = draft.clone();
                Callback::from(move |value: String| mutate_draft(&draft, |d| {
                    d.content_flags.warnings = value.split(',').map(|s| s.trim().to_string()).filter(|s| !s.is_empty()).collect();
                }))
            }) }
        </details>
    }
}

fn scenario_triggers_editor(draft: &UseStateHandle<ScenarioDraft>) -> Html {
    html! {
        <details class="scenario-extra-panel">
            <summary>{ format!("Scenario triggers ({})", draft.scenario_triggers.len()) }</summary>
            <p class="muted">{"Stored for future runtime evaluation. Define conditions and effects for reference and export."}</p>
            { for draft.scenario_triggers.iter().enumerate().map(|(index, trigger)| {
                let name = trigger.name.clone();
                let can_repeat = trigger.can_repeat;
                html! {
                    <div class="scenario-editor-block" key={index}>
                        { text_input("Name", &name, {
                            let draft = draft.clone();
                            Callback::from(move |value: String| mutate_draft(&draft, |d| {
                                if let Some(row) = d.scenario_triggers.get_mut(index) { row.name = value; }
                            }))
                        }) }
                        <label class="field field-inline">
                            <input type="checkbox" checked={can_repeat} onchange={{
                                let draft = draft.clone();
                                Callback::from(move |e: Event| {
                                    let input: HtmlInputElement = e.target_unchecked_into();
                                    mutate_draft(&draft, |d| {
                                        if let Some(row) = d.scenario_triggers.get_mut(index) {
                                            row.can_repeat = input.checked();
                                        }
                                    });
                                })
                            }} />
                            <span class="muted">{"Can repeat"}</span>
                        </label>
                        <div class="muted">{"Conditions"}</div>
                        { for trigger.conditions.iter().enumerate().map(|(ci, cond)| {
                            let key = cond.key.clone();
                            let inequality = cond.inequality.clone();
                            let required = cond.required_value.clone();
                            html! {
                                <div class="scenario-editor-row" key={ci}>
                                    <input type="text" class="input" placeholder="Key" value={key} oninput={{
                                        let draft = draft.clone();
                                        Callback::from(move |e: InputEvent| {
                                            let input: HtmlInputElement = e.target_unchecked_into();
                                            mutate_draft(&draft, |d| {
                                                if let Some(t) = d.scenario_triggers.get_mut(index) {
                                                    if let Some(c) = t.conditions.get_mut(ci) { c.key = input.value(); }
                                                }
                                            });
                                        })
                                    }} />
                                    <input type="text" class="input input-compact" placeholder="Inequality" value={inequality} oninput={{
                                        let draft = draft.clone();
                                        Callback::from(move |e: InputEvent| {
                                            let input: HtmlInputElement = e.target_unchecked_into();
                                            mutate_draft(&draft, |d| {
                                                if let Some(t) = d.scenario_triggers.get_mut(index) {
                                                    if let Some(c) = t.conditions.get_mut(ci) { c.inequality = input.value(); }
                                                }
                                            });
                                        })
                                    }} />
                                    <input type="text" class="input" placeholder="Required value" value={required} oninput={{
                                        let draft = draft.clone();
                                        Callback::from(move |e: InputEvent| {
                                            let input: HtmlInputElement = e.target_unchecked_into();
                                            mutate_draft(&draft, |d| {
                                                if let Some(t) = d.scenario_triggers.get_mut(index) {
                                                    if let Some(c) = t.conditions.get_mut(ci) { c.required_value = input.value(); }
                                                }
                                            });
                                        })
                                    }} />
                                </div>
                            }
                        }) }
                        <button type="button" class="btn secondary btn-compact" onclick={{
                            let draft = draft.clone();
                            Callback::from(move |_| mutate_draft(&draft, |d| {
                                if let Some(t) = d.scenario_triggers.get_mut(index) {
                                    t.conditions.push(TriggerCondition::default());
                                }
                            }))
                        }}>{"Add condition"}</button>
                        <div class="muted">{"Effects"}</div>
                        { for trigger.effects.iter().enumerate().map(|(ei, effect)| {
                            match effect {
                                TriggerEffect::SetState { key, value } => {
                                    let key = key.clone();
                                    let value = value.clone();
                                    html! {
                                        <div class="scenario-editor-row" key={ei}>
                                            <span class="muted">{"Set state"}</span>
                                            <input type="text" class="input" placeholder="Key" value={key} oninput={{
                                                let draft = draft.clone();
                                                Callback::from(move |e: InputEvent| {
                                                    let input: HtmlInputElement = e.target_unchecked_into();
                                                    mutate_draft(&draft, |d| {
                                                        if let Some(t) = d.scenario_triggers.get_mut(index) {
                                                            if let Some(TriggerEffect::SetState { key, .. }) = t.effects.get_mut(ei) {
                                                                *key = input.value();
                                                            }
                                                        }
                                                    });
                                                })
                                            }} />
                                            <input type="text" class="input" placeholder="Value" value={value} oninput={{
                                                let draft = draft.clone();
                                                Callback::from(move |e: InputEvent| {
                                                    let input: HtmlInputElement = e.target_unchecked_into();
                                                    mutate_draft(&draft, |d| {
                                                        if let Some(t) = d.scenario_triggers.get_mut(index) {
                                                            if let Some(TriggerEffect::SetState { value, .. }) = t.effects.get_mut(ei) {
                                                                *value = input.value();
                                                            }
                                                        }
                                                    });
                                                })
                                            }} />
                                        </div>
                                    }
                                }
                                TriggerEffect::AppendGmInstruction { text } => {
                                    let text = text.clone();
                                    html! {
                                        <div class="scenario-editor-row" key={ei}>
                                            <span class="muted">{"GM instruction"}</span>
                                            <input type="text" class="input" placeholder="Text" value={text} oninput={{
                                                let draft = draft.clone();
                                                Callback::from(move |e: InputEvent| {
                                                    let input: HtmlInputElement = e.target_unchecked_into();
                                                    mutate_draft(&draft, |d| {
                                                        if let Some(t) = d.scenario_triggers.get_mut(index) {
                                                            if let Some(TriggerEffect::AppendGmInstruction { text }) = t.effects.get_mut(ei) {
                                                                *text = input.value();
                                                            }
                                                        }
                                                    });
                                                })
                                            }} />
                                        </div>
                                    }
                                }
                            }
                        }) }
                        <div class="scenario-editor-row">
                            <button type="button" class="btn secondary btn-compact" onclick={{
                                let draft = draft.clone();
                                Callback::from(move |_| mutate_draft(&draft, |d| {
                                    if let Some(t) = d.scenario_triggers.get_mut(index) {
                                        t.effects.push(TriggerEffect::SetState { key: String::new(), value: String::new() });
                                    }
                                }))
                            }}>{"Add set-state effect"}</button>
                            <button type="button" class="btn secondary btn-compact" onclick={{
                                let draft = draft.clone();
                                Callback::from(move |_| mutate_draft(&draft, |d| {
                                    if let Some(t) = d.scenario_triggers.get_mut(index) {
                                        t.effects.push(TriggerEffect::AppendGmInstruction { text: String::new() });
                                    }
                                }))
                            }}>{"Add GM instruction"}</button>
                        </div>
                        <button type="button" class="btn secondary btn-compact" onclick={{
                            let draft = draft.clone();
                            Callback::from(move |_| mutate_draft(&draft, |d| { d.scenario_triggers.remove(index); }))
                        }}>{"Remove trigger"}</button>
                    </div>
                }
            }) }
            <button type="button" class="btn secondary" onclick={{
                let draft = draft.clone();
                Callback::from(move |_| mutate_draft(&draft, |d| {
                    d.scenario_triggers.push(ScenarioTrigger::default());
                }))
            }}>{"Add trigger"}</button>
        </details>
    }
}

fn game_elements_editor(draft: &UseStateHandle<ScenarioDraft>) -> Html {
    html! {
        <details class="scenario-extra-panel">
            <summary>{ format!("Game elements ({} boards, {} decks)", draft.game_elements.boards.len(), draft.game_elements.decks.len()) }</summary>
            <p class="muted">{"Boards and card decks used by the inline-prose agent during play."}</p>
            <h4 class="muted">{"Boards"}</h4>
            { for draft.game_elements.boards.iter().enumerate().map(|(index, board)| {
                let id = board.id.clone();
                let spaces = board.spaces;
                let move_dice = board.move_dice.clone();
                let default_tag = board.default_tag.clone();
                let tag_rules_text = format_tag_rules(&board.tag_rules);
                html! {
                    <div class="scenario-editor-block" key={format!("board-{index}")}>
                        { text_input("Board id", &id, {
                            let draft = draft.clone();
                            Callback::from(move |value: String| mutate_draft(&draft, |d| {
                                if let Some(row) = d.game_elements.boards.get_mut(index) { row.id = value; }
                            }))
                        }) }
                        <div class="scenario-editor-row">
                            <label class="field field-inline">
                                <span class="muted">{"Spaces"}</span>
                                <input type="number" class="input input-compact" value={spaces.to_string()} oninput={{
                                    let draft = draft.clone();
                                    Callback::from(move |e: InputEvent| {
                                        let input: HtmlInputElement = e.target_unchecked_into();
                                        let parsed = input.value().parse::<u32>().unwrap_or(80);
                                        mutate_draft(&draft, |d| {
                                            if let Some(row) = d.game_elements.boards.get_mut(index) { row.spaces = parsed; }
                                        });
                                    })
                                }} />
                            </label>
                            { text_input("Move dice", &move_dice, {
                                let draft = draft.clone();
                                Callback::from(move |value: String| mutate_draft(&draft, |d| {
                                    if let Some(row) = d.game_elements.boards.get_mut(index) { row.move_dice = value; }
                                }))
                            }) }
                            { text_input("Default tag", &default_tag, {
                                let draft = draft.clone();
                                Callback::from(move |value: String| mutate_draft(&draft, |d| {
                                    if let Some(row) = d.game_elements.boards.get_mut(index) { row.default_tag = value; }
                                }))
                            }) }
                        </div>
                        { text_input("Tag rules (tag:1,2,3 per line)", &tag_rules_text, {
                            let draft = draft.clone();
                            Callback::from(move |value: String| mutate_draft(&draft, |d| {
                                if let Some(row) = d.game_elements.boards.get_mut(index) {
                                    row.tag_rules = parse_tag_rules(&value);
                                }
                            }))
                        }) }
                        <button type="button" class="btn secondary btn-compact" onclick={{
                            let draft = draft.clone();
                            Callback::from(move |_| mutate_draft(&draft, |d| { d.game_elements.boards.remove(index); }))
                        }}>{"Remove board"}</button>
                    </div>
                }
            }) }
            <button type="button" class="btn secondary" onclick={{
                let draft = draft.clone();
                Callback::from(move |_| mutate_draft(&draft, |d| {
                    d.game_elements.boards.push(BoardDef {
                        id: String::new(),
                        spaces: 80,
                        move_dice: "1d6".to_string(),
                        tag_rules: Vec::new(),
                        default_tag: "space".to_string(),
                    });
                }))
            }}>{"Add board"}</button>
            <h4 class="muted">{"Decks"}</h4>
            { for draft.game_elements.decks.iter().enumerate().map(|(index, deck)| {
                let id = deck.id.clone();
                let consume = deck.consume_on_draw;
                html! {
                    <div class="scenario-editor-block" key={format!("deck-{index}")}>
                        { text_input("Deck id", &id, {
                            let draft = draft.clone();
                            Callback::from(move |value: String| mutate_draft(&draft, |d| {
                                if let Some(row) = d.game_elements.decks.get_mut(index) { row.id = value; }
                            }))
                        }) }
                        <label class="field field-inline">
                            <input type="checkbox" checked={consume} onchange={{
                                let draft = draft.clone();
                                Callback::from(move |e: Event| {
                                    let input: HtmlInputElement = e.target_unchecked_into();
                                    mutate_draft(&draft, |d| {
                                        if let Some(row) = d.game_elements.decks.get_mut(index) {
                                            row.consume_on_draw = input.checked();
                                        }
                                    });
                                })
                            }} />
                            <span class="muted">{"Consume on draw"}</span>
                        </label>
                        <div class="muted">{"Cards"}</div>
                        { for deck.cards.iter().enumerate().map(|(ci, card)| {
                            let card_id = card.id.clone();
                            let name = card.name.clone();
                            let text = card.text.clone();
                            html! {
                                <div class="scenario-editor-block" key={ci}>
                                    <div class="scenario-editor-row">
                                        <input type="text" class="input" placeholder="Card id" value={card_id} oninput={{
                                            let draft = draft.clone();
                                            Callback::from(move |e: InputEvent| {
                                                let input: HtmlInputElement = e.target_unchecked_into();
                                                mutate_draft(&draft, |d| {
                                                    if let Some(deck) = d.game_elements.decks.get_mut(index) {
                                                        if let Some(card) = deck.cards.get_mut(ci) { card.id = input.value(); }
                                                    }
                                                });
                                            })
                                        }} />
                                        <input type="text" class="input" placeholder="Name" value={name} oninput={{
                                            let draft = draft.clone();
                                            Callback::from(move |e: InputEvent| {
                                                let input: HtmlInputElement = e.target_unchecked_into();
                                                mutate_draft(&draft, |d| {
                                                    if let Some(deck) = d.game_elements.decks.get_mut(index) {
                                                        if let Some(card) = deck.cards.get_mut(ci) { card.name = input.value(); }
                                                    }
                                                });
                                            })
                                        }} />
                                    </div>
                                    { textarea_input("Card text", &text, {
                                        let draft = draft.clone();
                                        Callback::from(move |value: String| mutate_draft(&draft, |d| {
                                            if let Some(deck) = d.game_elements.decks.get_mut(index) {
                                                if let Some(card) = deck.cards.get_mut(ci) { card.text = value; }
                                            }
                                        }))
                                    }) }
                                    <button type="button" class="btn secondary btn-compact" onclick={{
                                        let draft = draft.clone();
                                        Callback::from(move |_| mutate_draft(&draft, |d| {
                                            if let Some(deck) = d.game_elements.decks.get_mut(index) {
                                                deck.cards.remove(ci);
                                            }
                                        }))
                                    }}>{"Remove card"}</button>
                                </div>
                            }
                        }) }
                        <button type="button" class="btn secondary btn-compact" onclick={{
                            let draft = draft.clone();
                            Callback::from(move |_| mutate_draft(&draft, |d| {
                                if let Some(deck) = d.game_elements.decks.get_mut(index) {
                                    deck.cards.push(CardDef {
                                        id: String::new(),
                                        name: String::new(),
                                        text: String::new(),
                                    });
                                }
                            }))
                        }}>{"Add card"}</button>
                        <button type="button" class="btn secondary btn-compact" onclick={{
                            let draft = draft.clone();
                            Callback::from(move |_| mutate_draft(&draft, |d| { d.game_elements.decks.remove(index); }))
                        }}>{"Remove deck"}</button>
                    </div>
                }
            }) }
            <button type="button" class="btn secondary" onclick={{
                let draft = draft.clone();
                Callback::from(move |_| mutate_draft(&draft, |d| {
                    d.game_elements.decks.push(DeckDef {
                        id: String::new(),
                        consume_on_draw: true,
                        cards: Vec::new(),
                    });
                }))
            }}>{"Add deck"}</button>
        </details>
    }
}

fn format_tag_rules(rules: &[BoardTagRule]) -> String {
    rules
        .iter()
        .map(|rule| {
            let spaces = rule
                .spaces
                .iter()
                .map(|s| s.to_string())
                .collect::<Vec<_>>()
                .join(",");
            format!("{}:{}", rule.tag, spaces)
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn parse_tag_rules(raw: &str) -> Vec<BoardTagRule> {
    raw.lines()
        .filter_map(|line| {
            let line = line.trim();
            if line.is_empty() {
                return None;
            }
            let (tag, spaces_raw) = line.split_once(':')?;
            let tag = tag.trim().to_string();
            if tag.is_empty() {
                return None;
            }
            let spaces = spaces_raw
                .split(',')
                .filter_map(|s| s.trim().parse::<u32>().ok())
                .collect();
            Some(BoardTagRule { tag, spaces })
        })
        .collect()
}

fn source_meta_panel(draft: &UseStateHandle<ScenarioDraft>) -> Html {
    let Some(meta) = draft.source_meta.as_ref() else {
        return html! {};
    };
    html! {
        <details class="scenario-extra-panel">
            <summary>{"Import metadata"}</summary>
            <p class="muted">{ format!("Platform: {} · Schema: {} · Version: {}", meta.platform, meta.schema_version, meta.original_version) }</p>
        </details>
    }
}
