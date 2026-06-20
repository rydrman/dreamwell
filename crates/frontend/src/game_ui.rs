use std::collections::HashSet;

use dreamwell_types::*;
use web_sys::{HtmlInputElement, HtmlTextAreaElement};
use yew::prelude::*;

use crate::api;
use crate::game_sync::should_replace_detail_from_sse;
use crate::generation_ui::{game_notice, GenerationStatusBar};
use crate::markdown::render_message_content;
use crate::message_menu::MessageOptionsMenu;
use crate::router::{AppRoute, Overlay};
use crate::title_editor::TitleEditor;

#[derive(Properties, PartialEq)]
pub struct GameShellProps {
    pub route: AppRoute,
    pub on_navigate: Callback<(AppRoute, bool)>,
}

fn game_id_from_route(route: &AppRoute) -> Option<i64> {
    match route {
        AppRoute::Games { game_id, .. } => *game_id,
        _ => None,
    }
}

fn tier_class(tier: Option<CheckTier>) -> &'static str {
    match tier {
        Some(CheckTier::Fail) => "tier-fail",
        Some(CheckTier::Mixed) => "tier-mixed",
        Some(CheckTier::Strong) => "tier-strong",
        None => "",
    }
}

fn tier_label(tier: Option<CheckTier>) -> &'static str {
    match tier {
        Some(CheckTier::Fail) => "Fail",
        Some(CheckTier::Mixed) => "Mixed",
        Some(CheckTier::Strong) => "Strong",
        None => "—",
    }
}

fn phase_label(phase: &str) -> &str {
    match phase {
        "checks" | "checks_pause" => "Checks",
        "rolled" | "rolled_pause" => "Roll",
        "resolved" | "resolved_pause" => "State",
        "scene" | "scene_pause" => "Scene",
        "prose" => "Prose",
        "done" => "Done",
        "failed" => "Failed",
        _ => phase,
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

fn sorted_skills(skills: &std::collections::HashMap<String, i64>) -> Vec<(String, i64)> {
    let mut rows: Vec<_> = skills.iter().map(|(k, v)| (k.clone(), *v)).collect();
    rows.sort_by(|left, right| left.0.cmp(&right.0));
    rows
}

fn sorted_state_entries(state: &[GameStateEntry]) -> Vec<GameStateEntry> {
    let mut rows = state.to_vec();
    rows.sort_by(|left, right| {
        state_kind_order(left.kind)
            .cmp(&state_kind_order(right.kind))
            .then_with(|| left.key.cmp(&right.key))
    });
    rows
}

#[function_component(GameShell)]
pub fn game_shell(props: &GameShellProps) -> Html {
    let game_id = game_id_from_route(&props.route);
    let detail = use_state(|| None::<GameDetail>);
    let action_input = use_state(String::new);
    let guidance_input = use_state(String::new);
    let submitting = use_state(|| false);
    let expanded_phases = use_state(HashSet::<(i64, String)>::new);

    {
        let detail = detail.clone();
        use_effect_with(game_id, move |id| {
            let mut stream = None;
            if let Some(game_id) = *id {
                let detail_fetch = detail.clone();
                wasm_bindgen_futures::spawn_local(async move {
                    if let Ok(d) = api::get_game(game_id).await {
                        detail_fetch.set(Some(d));
                    }
                });
                stream = Some(api::GameStream::new(game_id, {
                    let detail = detail.clone();
                    move |payload| {
                        if should_replace_detail_from_sse(payload.active_job.as_ref())
                            || (*detail).is_none()
                        {
                            detail.set(Some(payload.detail));
                        }
                    }
                }));
            } else {
                detail.set(None);
            }
            move || {
                drop(stream);
            }
        });
    }

    let on_submit = {
        let action_input = action_input.clone();
        let guidance_input = guidance_input.clone();
        let detail = detail.clone();
        let submitting = submitting.clone();
        Callback::from(move |_| {
            let Some(game_id) = game_id else { return };
            let action = (*action_input).clone();
            if action.trim().is_empty() {
                return;
            }
            submitting.set(true);
            let guidance = (*guidance_input).clone();
            let detail = detail.clone();
            let action_input = action_input.clone();
            let submitting = submitting.clone();
            wasm_bindgen_futures::spawn_local(async move {
                let payload = SubmitTurnRequest {
                    player_action: action,
                    guidance_notes: guidance,
                };
                if let Ok(d) = api::submit_turn(game_id, &payload).await {
                    detail.set(Some(d));
                    action_input.set(String::new());
                }
                submitting.set(false);
            });
        })
    };

    let on_continue = {
        let detail = detail.clone();
        Callback::from(move |turn_id: i64| {
            let Some(game_id) = game_id else { return };
            let detail = detail.clone();
            wasm_bindgen_futures::spawn_local(async move {
                if let Ok(d) = api::continue_turn(game_id, turn_id).await {
                    detail.set(Some(d));
                }
            });
        })
    };

    let on_regenerate = {
        let detail = detail.clone();
        Callback::from(move |turn_id: i64| {
            let Some(game_id) = game_id else { return };
            let detail = detail.clone();
            wasm_bindgen_futures::spawn_local(async move {
                if let Ok(d) = api::regenerate_turn(game_id, turn_id).await {
                    detail.set(Some(d));
                }
            });
        })
    };

    let on_recheck_prose = {
        let detail = detail.clone();
        let guidance_input = guidance_input.clone();
        Callback::from(move |turn_id: i64| {
            let Some(game_id) = game_id else { return };
            let detail = detail.clone();
            let guidance = (*guidance_input).clone();
            wasm_bindgen_futures::spawn_local(async move {
                if api::recheck_turn_prose(game_id, turn_id, &guidance)
                    .await
                    .is_ok()
                {
                    if let Ok(d) = api::get_game(game_id).await {
                        detail.set(Some(d));
                    }
                }
            });
        })
    };

    let on_recheck_state = {
        let detail = detail.clone();
        let guidance_input = guidance_input.clone();
        Callback::from(move |turn_id: i64| {
            let Some(game_id) = game_id else { return };
            let detail = detail.clone();
            let guidance = (*guidance_input).clone();
            wasm_bindgen_futures::spawn_local(async move {
                if api::recheck_turn_state(game_id, turn_id, &guidance)
                    .await
                    .is_ok()
                {
                    if let Ok(d) = api::get_game(game_id).await {
                        detail.set(Some(d));
                    }
                }
            });
        })
    };

    let toggle_phase = {
        let expanded_phases = expanded_phases.clone();
        Callback::from(move |(turn_id, phase): (i64, String)| {
            let mut set = (*expanded_phases).clone();
            let key = (turn_id, phase);
            if set.contains(&key) {
                set.remove(&key);
            } else {
                set.insert(key);
            }
            expanded_phases.set(set);
        })
    };

    let open_state_overlay = {
        let on_navigate = props.on_navigate.clone();
        let route = props.route.clone();
        Callback::from(move |_| {
            on_navigate.emit((route.clone().with_overlay(Overlay::State), true));
        })
    };

    let close_state_overlay = {
        let on_navigate = props.on_navigate.clone();
        let route = props.route.clone();
        Callback::from(move |_| {
            on_navigate.emit((route.clone().without_overlay(), true));
        })
    };

    let game_detail = (*detail).clone();
    let notice = game_detail.as_ref().and_then(game_notice);
    let state_overlay_open = props.route.overlay() == Some(Overlay::State);

    html! {
        <>
            if state_overlay_open {
                if let Some(game_detail) = game_detail.clone() {
                    <GameStateOverlay
                        game_detail={game_detail}
                        on_close={close_state_overlay.clone()}
                        on_detail={Callback::from({
                            let detail = detail.clone();
                            move |updated: GameDetail| detail.set(Some(updated))
                        })}
                    />
                } else {
                    <div class="settings-popover panel-overlay">
                        <div class="settings-header">
                            <h2>{"Character & state"}</h2>
                            <button class="btn secondary btn-compact" onclick={close_state_overlay.reform(|_| ())}>{"Close"}</button>
                        </div>
                        <p class="muted">{"Loading game…"}</p>
                    </div>
                }
            }
            <div class="game-shell chat-pane">
                if let Some(game_detail) = game_detail {
                    <header class="header content-header">
                        <div class="content-header-row">
                            <TitleEditor
                                title={game_detail.game.title.clone()}
                                class="header-title"
                                placeholder="Game title"
                                on_save={Callback::from({
                                    let detail_state = detail.clone();
                                    let game_id = game_detail.game.id;
                                    move |title| {
                                        let detail_state = detail_state.clone();
                                        wasm_bindgen_futures::spawn_local(async move {
                                            let payload = GameUpdate {
                                                title: Some(title),
                                                ..Default::default()
                                            };
                                            if let Ok(d) = api::update_game(game_id, &payload).await {
                                                detail_state.set(Some(d));
                                            }
                                        });
                                    }
                                })}
                            />
                            <div class="header-actions">
                                <button
                                    class="btn secondary btn-compact header-icon-btn"
                                    title="Character & state"
                                    onclick={open_state_overlay.reform(|_| ())}
                                >
                                    {"State"}
                                </button>
                            </div>
                        </div>
                    </header>

                    <div class="content-scroll">
                        <div class="messages game-turn-feed">
                            { for game_detail.turns.iter().map(|turn| {
                                let turn_id = turn.id;
                                let is_active = !matches!(turn.phase.as_str(), "done" | "failed");
                                let step_paused = turn.phase.ends_with("_pause");
                                let show_continue = step_paused;
                                let show_regenerate = turn.phase == "done";
                                let show_align_prose = turn.phase == "done"
                                    && !turn.prose.is_empty()
                                    && !turn.scene_beats.is_empty();
                                let show_recheck_state = turn.phase == "done" && !turn.prose.is_empty();
                                let can_menu = show_continue
                                    || show_regenerate
                                    || show_align_prose
                                    || show_recheck_state;
                                html! {
                                    <div key={turn_id} class="game-turn-pair">
                                        <div class="message user">
                                            { &turn.player_action }
                                        </div>
                                        <div class="message assistant">
                                            <div class="message-header">
                                                <div class="message-meta muted">
                                                    { format!("Phase: {}", phase_label(&turn.phase)) }
                                                </div>
                                                if can_menu {
                                                    <MessageOptionsMenu title="Turn options">
                                                        if show_continue {
                                                            <button
                                                                type="button"
                                                                class="message-menu-item"
                                                                onclick={on_continue.reform(move |_| turn_id)}
                                                            >
                                                                {"Continue turn"}
                                                            </button>
                                                        }
                                                        if show_regenerate {
                                                            <button
                                                                type="button"
                                                                class="message-menu-item"
                                                                onclick={on_regenerate.reform(move |_| turn_id)}
                                                            >
                                                                {"Regenerate"}
                                                            </button>
                                                        }
                                                        if show_align_prose {
                                                            <button
                                                                type="button"
                                                                class="message-menu-item"
                                                                onclick={on_recheck_prose.reform(move |_| turn_id)}
                                                            >
                                                                {"Align prose"}
                                                            </button>
                                                        }
                                                        if show_recheck_state {
                                                            <button
                                                                type="button"
                                                                class="message-menu-item"
                                                                onclick={on_recheck_state.reform(move |_| turn_id)}
                                                            >
                                                                {"Recheck state"}
                                                            </button>
                                                        }
                                                    </MessageOptionsMenu>
                                                }
                                            </div>
                                            if !turn.checks.is_empty() {
                                                <GamePhaseSection
                                                    turn_id={turn_id}
                                                    phase_key={"checks".to_string()}
                                                    label={"Checks"}
                                                    expanded={expanded_phases.contains(&(turn_id, "checks".to_string())) || is_active}
                                                    on_toggle={toggle_phase.clone()}
                                                >
                                                    { for turn.checks.iter().map(|c| html! {
                                                        <div class="check-item">
                                                            <div class="check-label">{ &c.label }</div>
                                                            <div class="muted">{ format!("{} +{}", c.skill, c.modifier) }</div>
                                                            <div class="muted">{ &c.stakes }</div>
                                                            <div class="muted small">{ &c.justification }</div>
                                                        </div>
                                                    }) }
                                                </GamePhaseSection>
                                            }

                                            if turn.checks.iter().any(|c| !c.rolls.is_empty()) {
                                                <GamePhaseSection
                                                    turn_id={turn_id}
                                                    phase_key={"roll".to_string()}
                                                    label={"Roll"}
                                                    expanded={expanded_phases.contains(&(turn_id, "roll".to_string())) || is_active}
                                                    on_toggle={toggle_phase.clone()}
                                                >
                                                    { for turn.checks.iter().map(|c| html! {
                                                        <div class={classes!("roll-result", tier_class(c.tier))}>
                                                            <span>{ format!("{:?}", c.rolls) }</span>
                                                            <span>{ format!(" = {} ", c.total) }</span>
                                                            <span class="tier-badge">{ tier_label(c.tier) }</span>
                                                        </div>
                                                    }) }
                                                </GamePhaseSection>
                                            }

                                            if !turn.state_changes.is_empty() {
                                                <GamePhaseSection
                                                    turn_id={turn_id}
                                                    phase_key={"state".to_string()}
                                                    label={"State changes"}
                                                    expanded={expanded_phases.contains(&(turn_id, "state".to_string()))}
                                                    on_toggle={toggle_phase.clone()}
                                                >
                                                    { for turn.state_changes.iter().map(|sc| html! {
                                                        <div class="state-delta">
                                                            { format!("{} {}.{} {:?} ", sc.target, format!("{:?}", sc.kind).to_lowercase(), sc.key, sc.op) }
                                                            if let Some(prev) = sc.prev_num {
                                                                { format!("{prev} → ") }
                                                            }
                                                            if let Some(delta) = sc.delta {
                                                                { format!("Δ{delta}") }
                                                            }
                                                            if let Some(val) = &sc.value {
                                                                { val.clone() }
                                                            }
                                                        </div>
                                                    }) }
                                                </GamePhaseSection>
                                            }

                                            if !turn.scene_beats.is_empty() {
                                                <GamePhaseSection
                                                    turn_id={turn_id}
                                                    phase_key={"scene".to_string()}
                                                    label={"Scene"}
                                                    expanded={expanded_phases.contains(&(turn_id, "scene".to_string()))}
                                                    on_toggle={toggle_phase.clone()}
                                                >
                                                    <ul class="scene-beats">
                                                        { for turn.scene_beats.iter().map(|b| html! { <li>{ b }</li> }) }
                                                    </ul>
                                                </GamePhaseSection>
                                            }

                                            if !turn.prose.is_empty() || turn.phase == "prose" {
                                                <div class="game-prose markdown-body">
                                                    { render_message_content(&turn.prose) }
                                                </div>
                                            }
                                        </div>
                                    </div>
                                }
                            }) }
                        </div>

                        if let Some(notice) = notice {
                            <GenerationStatusBar notice={notice} />
                        }
                        <div class="composer game-composer">
                            <textarea
                                class="input"
                                placeholder="What do you do?"
                                rows="2"
                                value={(*action_input).clone()}
                                oninput={Callback::from({
                                    let action_input = action_input.clone();
                                    move |e: InputEvent| {
                                        let input: HtmlTextAreaElement = e.target_unchecked_into();
                                        action_input.set(input.value());
                                    }
                                })}
                            />
                            <input
                                class="input"
                                type="text"
                                placeholder="Optional guidance for the GM"
                                value={(*guidance_input).clone()}
                                oninput={Callback::from({
                                    let guidance_input = guidance_input.clone();
                                    move |e: InputEvent| {
                                        let input: HtmlInputElement = e.target_unchecked_into();
                                        guidance_input.set(input.value());
                                    }
                                })}
                            />
                            <button class="btn" disabled={*submitting} onclick={on_submit}>
                                { if *submitting { "Submitting…" } else { "Take action" } }
                            </button>
                        </div>
                    </div>
                } else if game_id.is_some() {
                    <p class="muted">{"Loading game…"}</p>
                } else {
                    <p class="muted">{"Select or create a game from the sidebar."}</p>
                }
            </div>
        </>
    }
}

#[derive(Properties, PartialEq)]
struct GamePhaseSectionProps {
    turn_id: i64,
    phase_key: String,
    label: &'static str,
    expanded: bool,
    on_toggle: Callback<(i64, String)>,
    children: Children,
}

#[function_component(GamePhaseSection)]
fn game_phase_section(props: &GamePhaseSectionProps) -> Html {
    let turn_id = props.turn_id;
    let phase_key = props.phase_key.clone();
    let on_toggle = props.on_toggle.clone();
    let expanded = props.expanded;

    html! {
        <div class="message-thought game-phase-section">
            <button
                type="button"
                class="message-thought-toggle"
                onclick={Callback::from(move |_| on_toggle.emit((turn_id, phase_key.clone())))}
            >
                <span class="message-thought-label">{ props.label }</span>
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
pub struct GameStateOverlayProps {
    pub game_detail: GameDetail,
    pub on_close: Callback<()>,
    pub on_detail: Callback<GameDetail>,
}

#[function_component(GameStateOverlay)]
pub fn game_state_overlay(props: &GameStateOverlayProps) -> Html {
    let game_detail = &props.game_detail;
    let detail_state = props.on_detail.clone();
    let game_id = game_detail.game.id;
    let state_entries = sorted_state_entries(&game_detail.state);

    html! {
        <div id="game-state-panel" class="settings-popover panel-overlay">
            <div class="settings-header">
                <h2>{"Character & state"}</h2>
                <button class="btn secondary btn-compact" onclick={props.on_close.reform(|_| ())}>{"Close"}</button>
            </div>
            <div class="panel-overlay-body">
                { for game_detail.actors.iter().filter(|a| a.role == "pc").map(|actor| html! {
                    <div class="actor-sheet" key={actor.id}>
                        <h4>{ if actor.name.is_empty() { "Player character" } else { &actor.name } }</h4>
                        <p class="muted">{ &actor.description }</p>
                        <div class="skills-grid">
                            { for sorted_skills(&actor.skills).into_iter().map(|(k, v)| html! {
                                <span key={k.clone()}>{ format!("{k}: {v:+}") }</span>
                            }) }
                        </div>
                    </div>
                }) }
                <div class="state-entries">
                    { for state_entries.iter().map(|entry| {
                        let label = format!("{:?}", entry.kind).to_lowercase();
                        let value_text = if matches!(entry.kind, StateKind::Resource | StateKind::Clock) {
                            format!(" {}/{}", entry.num_value.unwrap_or(0), entry.max_value.unwrap_or(0))
                        } else {
                            format!(" {}", entry.value)
                        };
                        html! {
                            <div class="state-entry" key={entry.id}>
                                <span class="state-key">{ format!("{label}: {}", entry.key) }</span>
                                <span>{ value_text }</span>
                            </div>
                        }
                    }) }
                </div>
                <label class="step-mode-toggle">
                    <input
                        type="checkbox"
                        checked={game_detail.game.step_mode}
                        onchange={{
                            let detail_state = detail_state.clone();
                            Callback::from(move |e: Event| {
                                let input: HtmlInputElement = e.target_unchecked_into();
                                let detail_state = detail_state.clone();
                                wasm_bindgen_futures::spawn_local(async move {
                                    let payload = GameUpdate {
                                        step_mode: Some(input.checked()),
                                        ..Default::default()
                                    };
                                    if let Ok(d) = api::update_game(game_id, &payload).await {
                                        detail_state.emit(d);
                                    }
                                });
                            })
                        }}
                    />
                    {" Step mode (pause between phases)"}
                </label>
                <details class="game-settings-panel">
                    <summary>{"Game settings"}</summary>
                    <div class="game-settings-fields">
                        <label class="muted">{"Resolution"}</label>
                        <div>{"PbtA 2d6"}</div>
                        <label class="muted">{"Modifier range (situational)"}</label>
                        <div class="modifier-range">
                            <input
                                class="input input-compact"
                                type="number"
                                value={game_detail.game.modifier_min.to_string()}
                                onchange={{
                                    let detail_state = detail_state.clone();
                                    let modifier_max = game_detail.game.modifier_max;
                                    Callback::from(move |e: Event| {
                                        let input: HtmlInputElement = e.target_unchecked_into();
                                        if let Ok(min) = input.value().parse::<i64>() {
                                            let detail_state = detail_state.clone();
                                            wasm_bindgen_futures::spawn_local(async move {
                                                let payload = GameUpdate {
                                                    modifier_min: Some(min),
                                                    modifier_max: Some(modifier_max),
                                                    ..Default::default()
                                                };
                                                if let Ok(d) = api::update_game(game_id, &payload).await {
                                                    detail_state.emit(d);
                                                }
                                            });
                                        }
                                    })
                                }}
                            />
                            <span>{" to "}</span>
                            <input
                                class="input input-compact"
                                type="number"
                                value={game_detail.game.modifier_max.to_string()}
                                onchange={{
                                    let detail_state = detail_state.clone();
                                    let modifier_min = game_detail.game.modifier_min;
                                    Callback::from(move |e: Event| {
                                        let input: HtmlInputElement = e.target_unchecked_into();
                                        if let Ok(max) = input.value().parse::<i64>() {
                                            let detail_state = detail_state.clone();
                                            wasm_bindgen_futures::spawn_local(async move {
                                                let payload = GameUpdate {
                                                    modifier_min: Some(modifier_min),
                                                    modifier_max: Some(max),
                                                    ..Default::default()
                                                };
                                                if let Ok(d) = api::update_game(game_id, &payload).await {
                                                    detail_state.emit(d);
                                                }
                                            });
                                        }
                                    })
                                }}
                            />
                        </div>
                        <label class="step-mode-toggle">
                            <input
                                type="checkbox"
                                checked={game_detail.game.merge_resolve_scene}
                                onchange={{
                                    let detail_state = detail_state.clone();
                                    Callback::from(move |e: Event| {
                                        let input: HtmlInputElement = e.target_unchecked_into();
                                        let detail_state = detail_state.clone();
                                        wasm_bindgen_futures::spawn_local(async move {
                                            let payload = GameUpdate {
                                                merge_resolve_scene: Some(input.checked()),
                                                ..Default::default()
                                            };
                                            if let Ok(d) = api::update_game(game_id, &payload).await {
                                                detail_state.emit(d);
                                            }
                                        });
                                    })
                                }}
                            />
                            {" Merge resolve + scene (faster)"}
                        </label>
                        <label class="muted">{"Model overrides (blank = global)"}</label>
                        <input
                            class="input"
                            type="text"
                            placeholder="Checks phase model"
                            value={game_detail.game.model_checks.clone()}
                            onchange={{
                                let detail_state = detail_state.clone();
                                Callback::from(move |e: Event| {
                                    let input: HtmlInputElement = e.target_unchecked_into();
                                    let detail_state = detail_state.clone();
                                    wasm_bindgen_futures::spawn_local(async move {
                                        let payload = GameUpdate {
                                            model_checks: Some(input.value()),
                                            ..Default::default()
                                        };
                                        if let Ok(d) = api::update_game(game_id, &payload).await {
                                            detail_state.emit(d);
                                        }
                                    });
                                })
                            }}
                        />
                        <input
                            class="input"
                            type="text"
                            placeholder="Resolve phase model"
                            value={game_detail.game.model_resolve.clone()}
                            onchange={{
                                let detail_state = detail_state.clone();
                                Callback::from(move |e: Event| {
                                    let input: HtmlInputElement = e.target_unchecked_into();
                                    let detail_state = detail_state.clone();
                                    wasm_bindgen_futures::spawn_local(async move {
                                        let payload = GameUpdate {
                                            model_resolve: Some(input.value()),
                                            ..Default::default()
                                        };
                                        if let Ok(d) = api::update_game(game_id, &payload).await {
                                            detail_state.emit(d);
                                        }
                                    });
                                })
                            }}
                        />
                        <input
                            class="input"
                            type="text"
                            placeholder="Prose phase model"
                            value={game_detail.game.model_prose.clone()}
                            onchange={{
                                let detail_state = detail_state.clone();
                                Callback::from(move |e: Event| {
                                    let input: HtmlInputElement = e.target_unchecked_into();
                                    let detail_state = detail_state.clone();
                                    wasm_bindgen_futures::spawn_local(async move {
                                        let payload = GameUpdate {
                                            model_prose: Some(input.value()),
                                            ..Default::default()
                                        };
                                        if let Ok(d) = api::update_game(game_id, &payload).await {
                                            detail_state.emit(d);
                                        }
                                    });
                                })
                            }}
                        />
                    </div>
                </details>
            </div>
        </div>
    }
}

#[allow(dead_code)]
pub fn merge_game_list(detail: &GameDetail, games: &[Game]) -> Vec<Game> {
    game_sync::game_list_with_detail(games, detail)
}

mod game_sync {
    pub use crate::game_sync::*;
}
