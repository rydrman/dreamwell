use dreamwell_types::*;
use std::collections::HashMap;
use web_sys::HtmlInputElement;
use yew::prelude::*;

use crate::scenario_ui::ScenarioDraft;

fn mutate_draft(draft: &UseStateHandle<ScenarioDraft>, f: impl FnOnce(&mut ScenarioDraft)) {
    let mut next = (**draft).clone();
    f(&mut next);
    draft.set(next);
}

fn optional_i64_input(label: &str, value: Option<i64>, on_change: Callback<Option<i64>>) -> Html {
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

fn text_input(label: &str, value: &str, on_change: Callback<String>) -> Html {
    html! {
        <label class="field">
            <span class="muted">{ label }</span>
            <input type="text" class="input" value={value.to_string()} oninput={Callback::from(move |e: InputEvent| {
                let input: HtmlInputElement = e.target_unchecked_into();
                on_change.emit(input.value());
            })} />
        </label>
    }
}

fn textarea_input(label: &str, value: &str, on_change: Callback<String>) -> Html {
    html! {
        <label class="field">
            <span class="muted">{ label }</span>
            <textarea value={value.to_string()} oninput={Callback::from(move |e: InputEvent| {
                let input: HtmlInputElement = e.target_unchecked_into();
                on_change.emit(input.value());
            })} />
        </label>
    }
}

pub fn scenario_advanced_editors(draft: &UseStateHandle<ScenarioDraft>) -> Html {
    html! {
        <div class="scenario-advanced-sections">
            { trait_defs_editor(draft) }
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
            <p class="muted scenario-traits-help">{"Define custom trait names and descriptions. When present, these replace the default PbtA sheet in the traits editor above."}</p>
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
                                    if let Some(row) = d.trait_defs.get_mut(index) {
                                        row.name = input.value();
                                    }
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
                            Callback::from(move |_| mutate_draft(&draft, |d| { d.trait_defs.remove(index); }))
                        }}>{"Remove"}</button>
                    </div>
                }
            }) }
            <button type="button" class="btn secondary" onclick={{
                let draft = draft.clone();
                Callback::from(move |_| mutate_draft(&draft, |d| {
                    d.trait_defs.push(TraitDef::default());
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
                        <button type="button" class="btn secondary btn-compact" onclick={{
                            let draft = draft.clone();
                            Callback::from(move |_| mutate_draft(&draft, |d| { d.cast.remove(index); }))
                        }}>{"Remove NPC"}</button>
                    </div>
                }
            }) }
            <button type="button" class="btn secondary" onclick={{
                let draft = draft.clone();
                Callback::from(move |_| mutate_draft(&draft, |d| { d.cast.push(ScenarioNpc::default()); }))
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
                let trait_summary: String = pc.traits.iter().map(|(k,v)| format!("{k}:{v}")).collect::<Vec<_>>().join(", ");
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
                        { text_input("Traits (name:value, comma-separated)", &trait_summary, {
                            let draft = draft.clone();
                            Callback::from(move |value: String| mutate_draft(&draft, |d| {
                                if let Some(row) = d.pc_options.get_mut(index) {
                                    row.traits = parse_trait_pairs(&value);
                                }
                            }))
                        }) }
                        <button type="button" class="btn secondary btn-compact" onclick={{
                            let draft = draft.clone();
                            Callback::from(move |_| mutate_draft(&draft, |d| { d.pc_options.remove(index); }))
                        }}>{"Remove PC option"}</button>
                    </div>
                }
            }) }
            <button type="button" class="btn secondary" onclick={{
                let draft = draft.clone();
                Callback::from(move |_| mutate_draft(&draft, |d| { d.pc_options.push(PcOption::default()); }))
            }}>{"Add PC option"}</button>
        </details>
    }
}

fn parse_trait_pairs(raw: &str) -> HashMap<String, i64> {
    let mut traits = HashMap::new();
    for part in raw.split(',') {
        let part = part.trim();
        if part.is_empty() {
            continue;
        }
        if let Some((name, value)) = part.split_once(':') {
            let name = name.trim();
            if name.is_empty() {
                continue;
            }
            let value = value.trim().parse::<i64>().unwrap_or(0);
            traits.insert(name.to_string(), value);
        }
    }
    traits
}

fn state_schema_editor(draft: &UseStateHandle<ScenarioDraft>) -> Html {
    html! {
        <details class="scenario-extra-panel">
            <summary>{ format!("State schema ({})", draft.state_schema.len()) }</summary>
            <p class="muted">{"Initial world and PC state seeded when a game starts. World-scoped entries apply to the scene; PC-scoped entries apply to the player character."}</p>
            { for draft.state_schema.iter().enumerate().map(|(index, def)| {
                let key = def.key.clone();
                let description = def.description.clone();
                let initial_value = def.initial_value.clone();
                let visibility = def.visibility.clone();
                let update_hints = def.update_hints.clone();
                let kind = def.kind;
                let scope = def.scope;
                html! {
                    <div class="scenario-editor-block" key={index}>
                        <div class="scenario-editor-row">
                            { text_input("Key", &key, {
                                let draft = draft.clone();
                                Callback::from(move |value: String| mutate_draft(&draft, |d| {
                                    if let Some(row) = d.state_schema.get_mut(index) { row.key = value; }
                                }))
                            }) }
                            <label class="field field-inline">
                                <span class="muted">{"Kind"}</span>
                                <select onchange={{
                                    let draft = draft.clone();
                                    Callback::from(move |e: Event| {
                                        let select: web_sys::HtmlSelectElement = e.target_unchecked_into();
                                        let parsed = match select.value().as_str() {
                                            "condition" => StateKind::Condition,
                                            "fact" => StateKind::Fact,
                                            "clock" => StateKind::Clock,
                                            _ => StateKind::Resource,
                                        };
                                        mutate_draft(&draft, |d| {
                                            if let Some(row) = d.state_schema.get_mut(index) { row.kind = parsed; }
                                        });
                                    })
                                }}>
                                    <option value="resource" selected={kind == StateKind::Resource}>{"resource"}</option>
                                    <option value="condition" selected={kind == StateKind::Condition}>{"condition"}</option>
                                    <option value="fact" selected={kind == StateKind::Fact}>{"fact"}</option>
                                    <option value="clock" selected={kind == StateKind::Clock}>{"clock"}</option>
                                </select>
                            </label>
                            <label class="field field-inline">
                                <span class="muted">{"Scope"}</span>
                                <select onchange={{
                                    let draft = draft.clone();
                                    Callback::from(move |e: Event| {
                                        let select: web_sys::HtmlSelectElement = e.target_unchecked_into();
                                        let parsed = if select.value() == "pc" { StateScope::Pc } else { StateScope::World };
                                        mutate_draft(&draft, |d| {
                                            if let Some(row) = d.state_schema.get_mut(index) { row.scope = parsed; }
                                        });
                                    })
                                }}>
                                    <option value="world" selected={scope == StateScope::World}>{"world"}</option>
                                    <option value="pc" selected={scope == StateScope::Pc}>{"pc"}</option>
                                </select>
                            </label>
                        </div>
                        { text_input("Description", &description, {
                            let draft = draft.clone();
                            Callback::from(move |value: String| mutate_draft(&draft, |d| {
                                if let Some(row) = d.state_schema.get_mut(index) { row.description = value; }
                            }))
                        }) }
                        <div class="scenario-editor-row">
                            { text_input("Initial value", &initial_value, {
                                let draft = draft.clone();
                                Callback::from(move |value: String| mutate_draft(&draft, |d| {
                                    if let Some(row) = d.state_schema.get_mut(index) { row.initial_value = value; }
                                }))
                            }) }
                            { optional_i64_input("Initial num", def.initial_num, {
                                let draft = draft.clone();
                                Callback::from(move |value: Option<i64>| mutate_draft(&draft, |d| {
                                    if let Some(row) = d.state_schema.get_mut(index) { row.initial_num = value; }
                                }))
                            }) }
                            { optional_i64_input("Initial max", def.initial_max, {
                                let draft = draft.clone();
                                Callback::from(move |value: Option<i64>| mutate_draft(&draft, |d| {
                                    if let Some(row) = d.state_schema.get_mut(index) { row.initial_max = value; }
                                }))
                            }) }
                        </div>
                        { text_input("Visibility", &visibility, {
                            let draft = draft.clone();
                            Callback::from(move |value: String| mutate_draft(&draft, |d| {
                                if let Some(row) = d.state_schema.get_mut(index) { row.visibility = value; }
                            }))
                        }) }
                        { text_input("Update hints", &update_hints, {
                            let draft = draft.clone();
                            Callback::from(move |value: String| mutate_draft(&draft, |d| {
                                if let Some(row) = d.state_schema.get_mut(index) { row.update_hints = value; }
                            }))
                        }) }
                        <button type="button" class="btn secondary btn-compact" onclick={{
                            let draft = draft.clone();
                            Callback::from(move |_| mutate_draft(&draft, |d| { d.state_schema.remove(index); }))
                        }}>{"Remove variable"}</button>
                    </div>
                }
            }) }
            <button type="button" class="btn secondary" onclick={{
                let draft = draft.clone();
                Callback::from(move |_| mutate_draft(&draft, |d| {
                    d.state_schema.push(TrackedVarDef {
                        key: String::new(),
                        kind: StateKind::Fact,
                        scope: StateScope::World,
                        description: String::new(),
                        initial_value: String::new(),
                        initial_num: None,
                        initial_max: None,
                        visibility: String::new(),
                        update_hints: String::new(),
                    });
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
                                    <textarea placeholder="Card text" value={text} oninput={{
                                        let draft = draft.clone();
                                        Callback::from(move |e: InputEvent| {
                                            let input: HtmlInputElement = e.target_unchecked_into();
                                            mutate_draft(&draft, |d| {
                                                if let Some(deck) = d.game_elements.decks.get_mut(index) {
                                                    if let Some(card) = deck.cards.get_mut(ci) { card.text = input.value(); }
                                                }
                                            });
                                        })
                                    }} />
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
