use std::cell::RefCell;
use std::collections::HashSet;
use std::rc::Rc;

use dreamwell_types::*;
use gloo_timers::callback::Timeout;
use web_sys::{HtmlElement, HtmlInputElement, HtmlTextAreaElement};
use yew::prelude::*;

use crate::api;
use crate::dice_ui::DiceRollDisplay;
use crate::game_presets_ui::GmTonePresetPicker;
use crate::game_sync::{detail_stale_vs_sse, should_replace_detail_from_sse};
use crate::generation_ui::{game_notice, GenerationErrorAlert, GenerationStatusBar};
use crate::item_list::GameList;
use crate::markdown::render_message_content;
use crate::message_menu::MessageOptionsMenu;
use crate::router::{AppRoute, Overlay};
use crate::state_ui::{
    sort_state_rows, InlineStateChangesGroup, PhaseSection, PlanBeatsList, StateEntriesPanel,
    StateEntryRow,
};
use crate::title_editor::TitleEditor;
use crate::view_scroll::scroll_content_view_to_bottom;

#[derive(Properties, PartialEq)]
pub struct GameShellProps {
    pub route: AppRoute,
    pub on_navigate: Callback<(AppRoute, bool)>,
    pub settings: Option<Settings>,
    pub games: Vec<Game>,
    pub on_select_game: Callback<i64>,
    pub on_new_game: Callback<()>,
    pub on_games_refresh: Callback<()>,
}

fn game_id_from_route(route: &AppRoute) -> Option<i64> {
    match route {
        AppRoute::Games { game_id, .. } => *game_id,
        _ => None,
    }
}

fn phase_label(phase: &str) -> &str {
    match phase {
        "checks" | "checks_pause" => "Checks",
        "rolled" | "rolled_pause" => "Roll",
        "resolved" | "resolved_pause" => "State",
        "scene" | "scene_pause" => "Scene",
        "prose" => "Prose",
        "structured" => "Structured agent",
        "done" => "Done",
        "failed" => "Failed",
        _ => phase,
    }
}

fn sorted_skills(skills: &std::collections::HashMap<String, i64>) -> Vec<(String, i64)> {
    let mut rows: Vec<_> = skills.iter().map(|(k, v)| (k.clone(), *v)).collect();
    rows.sort_by(|left, right| left.0.cmp(&right.0));
    rows
}

#[function_component(GameShell)]
pub fn game_shell(props: &GameShellProps) -> Html {
    let game_id = game_id_from_route(&props.route);
    let detail = use_state(|| None::<GameDetail>);
    let detail_loading = use_state(|| false);
    let action_input = use_state(String::new);
    let guidance_input = use_state(String::new);
    let submitting = use_state(|| false);
    let expanded_phases = use_state(HashSet::<(i64, String)>::new);
    let turn_feed_ref = use_node_ref();

    {
        let detail = detail.clone();
        let detail_loading = detail_loading.clone();
        use_effect_with(game_id, move |id| {
            let mut stream = None;
            if let Some(game_id) = *id {
                detail.set(None);
                detail_loading.set(true);
                let detail_fetch = detail.clone();
                let detail_loading_fetch = detail_loading.clone();
                wasm_bindgen_futures::spawn_local(async move {
                    if let Ok(d) = api::get_game(game_id).await {
                        detail_fetch.set(Some(d));
                    }
                    detail_loading_fetch.set(false);
                });
                stream = Some(api::GameStream::new(game_id, {
                    let detail = detail.clone();
                    let detail_loading = detail_loading.clone();
                    let had_active_job = Rc::new(RefCell::new(false));
                    let had_active_job = had_active_job.clone();
                    move |payload| {
                        detail_loading.set(false);
                        let was_active = *had_active_job.borrow();
                        let now_active = payload.active_job.is_some();
                        if now_active {
                            *had_active_job.borrow_mut() = true;
                        }
                        let job_just_finished = (was_active && !now_active)
                            || (*detail).as_ref().is_some_and(|d| {
                                detail_stale_vs_sse(d, payload.active_job.as_ref())
                            });
                        if job_just_finished {
                            let detail_ref = detail.clone();
                            wasm_bindgen_futures::spawn_local(async move {
                                if let Ok(d) = api::get_game(game_id).await {
                                    detail_ref.set(Some(d));
                                }
                            });
                            *had_active_job.borrow_mut() = false;
                        } else if should_replace_detail_from_sse(payload.active_job.as_ref())
                            || (*detail).is_none()
                        {
                            detail.set(Some(payload.detail));
                        }
                    }
                }));
            } else {
                detail.set(None);
                detail_loading.set(false);
            }
            move || {
                drop(stream);
            }
        });
    }

    {
        let turn_feed_ref = turn_feed_ref.clone();
        let detail_loaded = (*detail).is_some();
        use_effect_with(
            (game_id, *detail_loading, detail_loaded),
            move |(game_id, detail_loading, detail_loaded)| {
                if game_id.is_some() && !*detail_loading && *detail_loaded {
                    let turn_feed_ref = turn_feed_ref.clone();
                    Timeout::new(0, move || {
                        let el = turn_feed_ref.cast::<HtmlElement>();
                        scroll_content_view_to_bottom(el.as_ref());
                    })
                    .forget();
                }
                || ()
            },
        );
    }

    let on_submit = {
        let action_input = action_input.clone();
        let guidance_input = guidance_input.clone();
        let detail = detail.clone();
        let submitting = submitting.clone();
        Callback::from(move |_| {
            let Some(game_id) = game_id else { return };
            let action = (*action_input).clone();
            let guidance = (*guidance_input).clone();
            if action.trim().is_empty() && guidance.trim().is_empty() {
                return;
            }
            submitting.set(true);
            let detail = detail.clone();
            let action_input = action_input.clone();
            let guidance_input = guidance_input.clone();
            let submitting = submitting.clone();
            wasm_bindgen_futures::spawn_local(async move {
                let payload = SubmitTurnRequest {
                    player_action: action,
                    guidance_notes: guidance,
                };
                match api::submit_turn(game_id, &payload).await {
                    Ok(d) => {
                        detail.set(Some(d));
                        action_input.set(String::new());
                        guidance_input.set(String::new());
                    }
                    Err(err) => {
                        if let Some(window) = web_sys::window() {
                            let _ =
                                window.alert_with_message(&format!("Could not submit turn: {err}"));
                        }
                    }
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
                match api::regenerate_turn(game_id, turn_id).await {
                    Ok(d) => detail.set(Some(d)),
                    Err(err) => {
                        if let Some(window) = web_sys::window() {
                            let _ =
                                window.alert_with_message(&format!("Could not retry turn: {err}"));
                        }
                    }
                }
            });
        })
    };

    let on_fork = {
        let on_navigate = props.on_navigate.clone();
        let on_games_refresh = props.on_games_refresh.clone();
        Callback::from(move |turn_id: i64| {
            let Some(game_id) = game_id else { return };
            let on_navigate = on_navigate.clone();
            let on_games_refresh = on_games_refresh.clone();
            wasm_bindgen_futures::spawn_local(async move {
                match api::fork_turn(game_id, turn_id).await {
                    Ok(detail) => {
                        on_games_refresh.emit(());
                        on_navigate.emit((
                            AppRoute::Games {
                                game_id: Some(detail.game.id),
                                overlay: Some(Overlay::State),
                                sidebar: false,
                            },
                            true,
                        ));
                    }
                    Err(err) => {
                        if let Some(window) = web_sys::window() {
                            let _ =
                                window.alert_with_message(&format!("Could not fork game: {err}"));
                        }
                    }
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
    let model_missing = props
        .settings
        .as_ref()
        .is_some_and(|s| s.model.trim().is_empty());
    let composer_empty = (*action_input).trim().is_empty() && (*guidance_input).trim().is_empty();
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
                            <h2>{"World & state"}</h2>
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
                                    title="World & state"
                                    onclick={open_state_overlay.reform(|_| ())}
                                >
                                    {"State"}
                                </button>
                            </div>
                        </div>
                    </header>

                    <div class="content-scroll">
                        <div class="messages game-turn-feed" ref={turn_feed_ref.clone()}>
                            { for game_detail.turns.iter().map(|turn| {
                                let turn_id = turn.id;
                                let is_opening = turn.is_opening;
                                let step_paused = turn.phase.ends_with("_pause");
                                let show_continue = step_paused;
                                let show_regenerate = turn.phase == "done";
                                let show_retry = turn.phase == "failed"
                                    || (!is_opening
                                        && turn.phase != "done"
                                        && turn.phase != "pending"
                                        && !turn.phase.ends_with("_pause"));
                                let show_align_prose = turn.phase == "done"
                                    && !turn.prose.is_empty()
                                    && !turn.scene_beats.is_empty();
                                let show_recheck_state = turn.phase == "done" && !turn.prose.is_empty();
                                let show_fork = turn.phase == "done";
                                let can_menu = !is_opening && (show_continue
                                    || show_regenerate
                                    || show_retry
                                    || show_fork
                                    || show_align_prose
                                    || show_recheck_state);
                                let display_prose = if turn.prose.is_empty() {
                                    String::new()
                                } else if is_opening {
                                    let (user_name, persona) = props
                                        .settings
                                        .as_ref()
                                        .map(|settings| {
                                            (
                                                settings.user_name.as_str(),
                                                settings.persona_description.as_str(),
                                            )
                                        })
                                        .unwrap_or(("", ""));
                                    substitute_macros(
                                        turn.prose.as_str(),
                                        &MacroContext::from_game_detail(
                                            &game_detail,
                                            user_name,
                                            persona,
                                        ),
                                    )
                                } else {
                                    turn.prose.clone()
                                };
                                let prose_has_inline_mech =
                                    display_prose.contains(PROSE_MECH_MARKER_OPEN);
                                let prose_has_inline_state =
                                    display_prose.contains(PROSE_STATE_MARKER_OPEN);
                                let prose_has_inline_check =
                                    display_prose.contains(PROSE_CHECK_MARKER_OPEN);
                                let prose_has_inline_blocks = prose_has_inline_mech
                                    || prose_has_inline_state
                                    || prose_has_inline_check;
                                let hide_detached_phases = prose_has_inline_blocks;
                                html! {
                                    <div key={turn_id} class={classes!("game-turn-pair", is_opening.then_some("game-opening"))}>
                                        if !is_opening
                                            && (!turn.player_action.trim().is_empty()
                                                || !turn.guidance_notes.trim().is_empty())
                                        {
                                            <div class="message user">
                                                if !turn.player_action.trim().is_empty() {
                                                    <div class="game-prose markdown-body">
                                                        { render_message_content(&turn.player_action) }
                                                    </div>
                                                }
                                                if !turn.guidance_notes.trim().is_empty() {
                                                    <div class="game-turn-guidance">
                                                        <span class="game-turn-guidance-label muted">{"GM guidance"}</span>
                                                        <div class="game-turn-guidance-body">
                                                            { render_message_content(&turn.guidance_notes) }
                                                        </div>
                                                    </div>
                                                }
                                            </div>
                                        }
                                        <div class="message assistant">
                                            if !is_opening {
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
                                                            if show_fork {
                                                                <button
                                                                    type="button"
                                                                    class="message-menu-item"
                                                                    onclick={on_fork.reform(move |_| turn_id)}
                                                                >
                                                                    {"Fork game here"}
                                                                </button>
                                                            }
                                                            if show_retry {
                                                                <button
                                                                    type="button"
                                                                    class="message-menu-item"
                                                                    onclick={on_regenerate.reform(move |_| turn_id)}
                                                                >
                                                                    { if turn.phase != "failed" {
                                                                        "Re-run turn"
                                                                    } else {
                                                                        "Retry"
                                                                    } }
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
                                            }
                                            if turn.phase == "failed" {
                                                if let Some(error) = turn.generation_error.clone() {
                                                    <GenerationErrorAlert
                                                        error={error}
                                                        on_retry={Some(on_regenerate.reform(move |_| turn_id))}
                                                    />
                                                } else {
                                                    <div class="message-error" role="alert">
                                                        <strong>{"Generation failed"}</strong>
                                                        <span>{"The turn did not complete. Use Retry to try again."}</span>
                                                        <div class="generation-error-actions">
                                                            <button
                                                                type="button"
                                                                class="btn secondary btn-compact"
                                                                onclick={on_regenerate.reform(move |_| turn_id)}
                                                            >
                                                                {"Retry"}
                                                            </button>
                                                        </div>
                                                    </div>
                                                }
                                            }
                                            if turn.plan.as_ref().map(|p| !p.summary_beats.is_empty()).unwrap_or(false) {
                                                <PhaseSection
                                                    label={"Plan".to_string()}
                                                    expanded={Some(expanded_phases.contains(&(turn_id, "plan".to_string())))}
                                                    on_toggle={Some(toggle_phase.reform(move |_: web_sys::MouseEvent| (turn_id, "plan".to_string())))}
                                                >
                                                    <PlanBeatsList
                                                        beats={turn.plan.as_ref().map(|p| p.summary_beats.clone()).unwrap_or_default()}
                                                        label={"Plan".to_string()}
                                                        inline={true}
                                                    />
                                                </PhaseSection>
                                            }

                                            if !turn.mechanical_results.is_empty() && !prose_has_inline_mech {
                                                <PhaseSection
                                                    label={"Mechanics".to_string()}
                                                    expanded={Some(expanded_phases.contains(&(turn_id, "mechanics".to_string())))}
                                                    on_toggle={Some(toggle_phase.reform(move |_: web_sys::MouseEvent| (turn_id, "mechanics".to_string())))}
                                                >
                                                    { for turn.mechanical_results.iter().map(|r| html! {
                                                        <div class="mechanical-result-item muted small" key={format!("{}-{}", turn_id, r.sort_order)}>
                                                            { mechanical_result_summary(r) }
                                                        </div>
                                                    }) }
                                                    if turn.observability.llm_call_count > 0 || turn.observability.tool_call_count > 0 {
                                                        <div class="muted small observability-summary">
                                                            { format!(
                                                                "Observability: {} LLM calls, {} tool calls ({:?})",
                                                                turn.observability.llm_call_count,
                                                                turn.observability.tool_call_count,
                                                                turn.observability.engine_mode
                                                            ) }
                                                        </div>
                                                    }
                                                </PhaseSection>
                                            }

                                            if !turn.system_rolls.is_empty() && !hide_detached_phases {
                                                <PhaseSection
                                                    label={"System rolls".to_string()}
                                                    expanded={Some(expanded_phases.contains(&(turn_id, "system".to_string())))}
                                                    on_toggle={Some(toggle_phase.reform(move |_: web_sys::MouseEvent| (turn_id, "system".to_string())))}
                                                >
                                                    { for turn.system_rolls.iter().map(|r| html! {
                                                        <DiceRollDisplay
                                                            rolls={r.rolls.clone()}
                                                            dice_expr={Some(r.dice_expr.clone())}
                                                            label={Some(r.label.clone())}
                                                            class="roll-result system-roll-result"
                                                        />
                                                    }) }
                                                </PhaseSection>
                                            }

                                            if !turn.checks.is_empty() && !hide_detached_phases {
                                                <PhaseSection
                                                    label={"Checks".to_string()}
                                                    expanded={Some(
                                                        expanded_phases.contains(&(turn_id, "checks".to_string()))
                                                            || expanded_phases.contains(&(turn_id, "roll".to_string())),
                                                    )}
                                                    on_toggle={Some(toggle_phase.reform(move |_: web_sys::MouseEvent| (turn_id, "checks".to_string())))}
                                                >
                                                    { for turn.checks.iter().map(|c| html! {
                                                        <div class="check-item">
                                                            <div class="check-item-header">
                                                                <div class="check-label">{ &c.label }</div>
                                                                if !c.rolls.is_empty() {
                                                                    <DiceRollDisplay
                                                                        rolls={c.rolls.clone()}
                                                                        dice_expr={Some(c.dice_expr.clone())}
                                                                        modifier={Some(c.modifier)}
                                                                        total={Some(c.total)}
                                                                        tier={c.tier}
                                                                        class="check-roll"
                                                                    />
                                                                }
                                                            </div>
                                                            <div class="muted">{ format!("{} {}", c.skill, crate::dice_ui::format_modifier(c.modifier)) }</div>
                                                            <div class="muted">{ &c.stakes }</div>
                                                            <div class="muted small">{ &c.justification }</div>
                                                        </div>
                                                    }) }
                                                </PhaseSection>
                                            }

                                            if !turn.scene_beats.is_empty() {
                                                <PhaseSection
                                                    label={"Scene".to_string()}
                                                    expanded={Some(expanded_phases.contains(&(turn_id, "scene".to_string())))}
                                                    on_toggle={Some(toggle_phase.reform(move |_: web_sys::MouseEvent| (turn_id, "scene".to_string())))}
                                                >
                                                    <PlanBeatsList
                                                        beats={turn.scene_beats.clone()}
                                                        label={"Scene".to_string()}
                                                        inline={true}
                                                    />
                                                </PhaseSection>
                                            }

                                            if !turn.state_changes.is_empty() && !hide_detached_phases {
                                                <InlineStateChangesGroup changes={turn.state_changes.clone()} />
                                            }

                                            if !display_prose.is_empty() || turn.phase == "prose" {
                                                <div class="game-prose markdown-body">
                                                    { render_prose_with_blocks(&display_prose, turn) }
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
                        if model_missing {
                            <div class="message-error composer-notice" role="alert">
                                <strong>{"No model configured"}</strong>
                                <span>{"Open Settings and choose a model before taking actions."}</span>
                            </div>
                        }
                        <div class="composer game-composer">
                            <div class="composer-input-stack">
                                <textarea
                                    class="composer-input-stack__primary input"
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
                                <textarea
                                    class="composer-input-stack__secondary input"
                                    placeholder="Optional guidance for the GM"
                                    rows="1"
                                    value={(*guidance_input).clone()}
                                    oninput={Callback::from({
                                        let guidance_input = guidance_input.clone();
                                        move |e: InputEvent| {
                                            let input: HtmlTextAreaElement = e.target_unchecked_into();
                                            guidance_input.set(input.value());
                                        }
                                    })}
                                />
                            </div>
                            <button class="btn" disabled={*submitting || model_missing || composer_empty} onclick={on_submit}>
                                { if *submitting { "Submitting…" } else { "Take action" } }
                            </button>
                        </div>
                    </div>
                } else if game_id.is_some() {
                    <>
                        <header class="header content-header">
                            <h1 class="header-title">{"Loading game…"}</h1>
                        </header>
                        <div class="loading-screen muted">{"Loading game…"}</div>
                    </>
                } else {
                    <>
                        <header class="header content-header">
                            <h1 class="header-title">{"Games"}</h1>
                            if props.games.is_empty() {
                                <p class="header-subtitle muted">{"Start a game from Scenarios or create one from the sidebar."}</p>
                            } else {
                                <p class="header-subtitle muted">{"Pick a game below to continue."}</p>
                            }
                        </header>
                        <div class="content-scroll">
                            if props.games.is_empty() {
                                <div class="empty-state muted">
                                    <p>{"No games yet. Start one from Scenarios or click New game in the sidebar."}</p>
                                    <button class="btn" style="margin-top:0.75rem;" onclick={props.on_new_game.reform(|_| ())}>
                                        {"New game"}
                                    </button>
                                </div>
                            } else {
                                <GameList
                                    games={props.games.clone()}
                                    on_select={props.on_select_game.clone()}
                                />
                            }
                        </div>
                    </>
                }
            </div>
        </>
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
    let state_rows = sort_state_rows(game_detail.state.iter().map(StateEntryRow::from).collect());

    html! {
        <div id="game-state-panel" class="settings-popover panel-overlay">
            <div class="settings-header">
                <h2>{"World & state"}</h2>
                <button class="btn secondary btn-compact" onclick={props.on_close.reform(|_| ())}>{"Close"}</button>
            </div>
            <div class="panel-overlay-body">
                <details class="game-world-panel" open=true>
                    <summary>{"Scenario & world"}</summary>
                    <div class="game-world-fields">
                        <label class="field"><span class="muted">{"Opening message"}</span>
                            <textarea
                                class="input"
                                rows="4"
                                value={game_detail.game.opening_message.clone()}
                                onchange={{
                                    let detail_state = detail_state.clone();
                                    let premise = game_detail.game.premise.clone();
                                    let setting = game_detail.game.setting.clone();
                                    let gm_style = game_detail.game.gm_style.clone();
                                    Callback::from(move |e: Event| {
                                        let input: HtmlTextAreaElement = e.target_unchecked_into();
                                        let detail_state = detail_state.clone();
                                        let premise = premise.clone();
                                        let setting = setting.clone();
                                        let gm_style = gm_style.clone();
                                        wasm_bindgen_futures::spawn_local(async move {
                                            let payload = GameUpdate {
                                                opening_message: Some(input.value()),
                                                premise: Some(premise),
                                                setting: Some(setting),
                                                gm_style: Some(gm_style),
                                                ..Default::default()
                                            };
                                            if let Ok(d) = api::update_game(game_id, &payload).await {
                                                detail_state.emit(d);
                                            }
                                        });
                                    })
                                }}
                            />
                        </label>
                        <label class="field"><span class="muted">{"Premise / scenario"}</span>
                            <textarea
                                class="input"
                                rows="3"
                                value={game_detail.game.premise.clone()}
                                onchange={{
                                    let detail_state = detail_state.clone();
                                    let setting = game_detail.game.setting.clone();
                                    let gm_style = game_detail.game.gm_style.clone();
                                    Callback::from(move |e: Event| {
                                        let input: HtmlTextAreaElement = e.target_unchecked_into();
                                        let detail_state = detail_state.clone();
                                        let setting = setting.clone();
                                        let gm_style = gm_style.clone();
                                        wasm_bindgen_futures::spawn_local(async move {
                                            let payload = GameUpdate {
                                                premise: Some(input.value()),
                                                setting: Some(setting),
                                                gm_style: Some(gm_style),
                                                ..Default::default()
                                            };
                                            if let Ok(d) = api::update_game(game_id, &payload).await {
                                                detail_state.emit(d);
                                            }
                                        });
                                    })
                                }}
                            />
                        </label>
                        <GmTonePresetPicker on_apply={Callback::from({
                            let detail_state = detail_state.clone();
                            let premise = game_detail.game.premise.clone();
                            let opening_message = game_detail.game.opening_message.clone();
                            move |(setting, gm_style)| {
                                let detail_state = detail_state.clone();
                                let premise = premise.clone();
                                let opening_message = opening_message.clone();
                                wasm_bindgen_futures::spawn_local(async move {
                                    let payload = GameUpdate {
                                        premise: Some(premise),
                                        opening_message: Some(opening_message),
                                        setting: Some(setting),
                                        gm_style: Some(gm_style),
                                        ..Default::default()
                                    };
                                    if let Ok(d) = api::update_game(game_id, &payload).await {
                                        detail_state.emit(d);
                                    }
                                });
                            }
                        })} />
                        <label class="field"><span class="muted">{"Setting / tone"}</span>
                            <textarea
                                class="input"
                                rows="3"
                                value={game_detail.game.setting.clone()}
                                onchange={{
                                    let detail_state = detail_state.clone();
                                    let premise = game_detail.game.premise.clone();
                                    let gm_style = game_detail.game.gm_style.clone();
                                    Callback::from(move |e: Event| {
                                        let input: HtmlTextAreaElement = e.target_unchecked_into();
                                        let detail_state = detail_state.clone();
                                        let premise = premise.clone();
                                        let gm_style = gm_style.clone();
                                        wasm_bindgen_futures::spawn_local(async move {
                                            let payload = GameUpdate {
                                                premise: Some(premise),
                                                setting: Some(input.value()),
                                                gm_style: Some(gm_style),
                                                ..Default::default()
                                            };
                                            if let Ok(d) = api::update_game(game_id, &payload).await {
                                                detail_state.emit(d);
                                            }
                                        });
                                    })
                                }}
                            />
                        </label>
                        <label class="field"><span class="muted">{"GM style"}</span>
                            <textarea
                                class="input"
                                rows="2"
                                value={game_detail.game.gm_style.clone()}
                                onchange={{
                                    let detail_state = detail_state.clone();
                                    let premise = game_detail.game.premise.clone();
                                    let setting = game_detail.game.setting.clone();
                                    Callback::from(move |e: Event| {
                                        let input: HtmlTextAreaElement = e.target_unchecked_into();
                                        let detail_state = detail_state.clone();
                                        let premise = premise.clone();
                                        let setting = setting.clone();
                                        wasm_bindgen_futures::spawn_local(async move {
                                            let payload = GameUpdate {
                                                premise: Some(premise),
                                                setting: Some(setting),
                                                gm_style: Some(input.value()),
                                                ..Default::default()
                                            };
                                            if let Ok(d) = api::update_game(game_id, &payload).await {
                                                detail_state.emit(d);
                                            }
                                        });
                                    })
                                }}
                            />
                        </label>
                    </div>
                </details>
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
                if !game_detail.game.state_schema.is_empty() {
                    <details class="state-schema-ref">
                        <summary class="muted">{ "Tracked variables" }</summary>
                        <ul class="state-schema-list">
                            { for game_detail.game.state_schema.iter().map(|def| html! {
                                <li key={def.key.clone()}>
                                    <strong>{ &def.key }</strong>
                                    if !def.description.is_empty() {
                                        { ": " }{ &def.description }
                                    }
                                </li>
                            }) }
                        </ul>
                    </details>
                }
                <StateEntriesPanel entries={state_rows} />
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

/// Render turn prose, expanding inline `⟦mech:N⟧`, `⟦state:N⟧`, and `⟦check:N⟧` markers
/// into styled blocks at the exact point in the narration where they fired.
fn render_prose_with_blocks(prose: &str, turn: &GameTurn) -> Html {
    let has_inline = prose.contains(PROSE_MECH_MARKER_OPEN)
        || prose.contains(PROSE_STATE_MARKER_OPEN)
        || prose.contains(PROSE_CHECK_MARKER_OPEN);
    if !has_inline {
        return render_message_content(prose);
    }

    let mut nodes: Vec<Html> = Vec::new();
    let mut rest = prose;
    while let Some((open_idx, tag, open_tag)) = next_inline_marker(rest) {
        let before = &rest[..open_idx];
        let after_open = &rest[open_idx + open_tag.len()..];
        let Some(close_idx) = after_open.find(PROSE_INLINE_MARKER_CLOSE) else {
            break;
        };
        let num_str = after_open[..close_idx].trim();
        let remainder = &after_open[close_idx + PROSE_INLINE_MARKER_CLOSE.len()..];

        let trimmed_before = before.trim();
        if !trimmed_before.is_empty() {
            nodes.push(render_message_content(trimmed_before));
        }
        if let Ok(index) = num_str.parse::<i64>() {
            if tag == InlineMarkerTag::State {
                let mut indices = vec![index];
                rest = remainder;
                while let Some((next_idx, consumed)) = parse_state_marker_at(rest) {
                    indices.push(next_idx);
                    rest = &rest[consumed..];
                }
                nodes.push(render_inline_state_group(&indices, turn));
            } else {
                nodes.push(match tag {
                    InlineMarkerTag::Mech => turn
                        .mechanical_results
                        .iter()
                        .find(|r| r.sort_order == index)
                        .map(render_inline_mechanic)
                        .unwrap_or_default(),
                    InlineMarkerTag::State => html! {},
                    InlineMarkerTag::Check => turn
                        .checks
                        .iter()
                        .find(|c| c.sort_order == index)
                        .map(render_inline_check)
                        .unwrap_or_default(),
                });
                rest = remainder;
            }
        } else {
            rest = remainder;
        }
    }
    let trimmed_rest = rest.trim();
    if !trimmed_rest.is_empty() {
        nodes.push(render_message_content(trimmed_rest));
    }
    html! { <>{ for nodes }</> }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum InlineMarkerTag {
    Mech,
    State,
    Check,
}

fn next_inline_marker(rest: &str) -> Option<(usize, InlineMarkerTag, &'static str)> {
    let candidates = [
        (InlineMarkerTag::Mech, PROSE_MECH_MARKER_OPEN),
        (InlineMarkerTag::State, PROSE_STATE_MARKER_OPEN),
        (InlineMarkerTag::Check, PROSE_CHECK_MARKER_OPEN),
    ];
    candidates
        .iter()
        .filter_map(|(tag, open)| rest.find(open).map(|idx| (idx, *tag, *open)))
        .min_by_key(|(idx, _, _)| *idx)
}

fn parse_state_marker_at(s: &str) -> Option<(i64, usize)> {
    let leading_ws = s.len() - s.trim_start().len();
    let trimmed = s.trim_start();
    if !trimmed.starts_with(PROSE_STATE_MARKER_OPEN) {
        return None;
    }
    let after_open = &trimmed[PROSE_STATE_MARKER_OPEN.len()..];
    let close_idx = after_open.find(PROSE_INLINE_MARKER_CLOSE)?;
    let index: i64 = after_open[..close_idx].trim().parse().ok()?;
    let consumed =
        leading_ws + PROSE_STATE_MARKER_OPEN.len() + close_idx + PROSE_INLINE_MARKER_CLOSE.len();
    Some((index, consumed))
}

fn render_inline_state_group(indices: &[i64], turn: &GameTurn) -> Html {
    let changes: Vec<AppliedStateChange> = indices
        .iter()
        .filter_map(|i| turn.state_changes.get(*i as usize).cloned())
        .collect();
    html! {
        <InlineStateChangesGroup changes={changes} />
    }
}

/// A single mechanic result rendered as an inline block inside the narration.
fn render_inline_mechanic(result: &MechanicalResult) -> Html {
    match (&result.kind, &result.data) {
        (
            MechanicalKind::DiceRoll,
            MechanicalData::DiceRoll {
                dice_expr,
                rolls,
                total,
            },
        ) => html! {
            <div class="game-inline-mechanic game-inline-dice">
                <DiceRollDisplay
                    rolls={rolls.clone()}
                    dice_expr={Some(dice_expr.clone())}
                    total={Some(*total)}
                    label={Some(result.label.clone())}
                    class="roll-result"
                />
            </div>
        },
        (MechanicalKind::CardDraw, MechanicalData::CardDraw { name, text, .. }) => html! {
            <div class="game-inline-mechanic game-inline-card">
                <div class="inline-mechanic-label muted small">{ "Card drawn" }</div>
                <div class="inline-card-name">{ name }</div>
                <div class="inline-card-text">{ text }</div>
            </div>
        },
        (
            MechanicalKind::BoardMove,
            MechanicalData::BoardMove {
                actor,
                roll,
                from_space,
                to_space,
                space_tags,
                ..
            },
        ) => html! {
            <div class="game-inline-mechanic game-inline-board">
                <DiceRollDisplay
                    rolls={vec![*roll]}
                    dice_expr={Some("1d6".to_string())}
                    label={Some(format!("{actor} moves"))}
                    class="roll-result"
                />
                <span class="inline-board-move">
                    { format!("space {from_space} → {to_space}") }
                </span>
                if !space_tags.is_empty() {
                    <span class="muted small">{ format!(" ({})", space_tags.join(", ")) }</span>
                }
            </div>
        },
        _ => html! {
            <div class="game-inline-mechanic muted small">{ mechanical_result_summary(result) }</div>
        },
    }
}

fn render_inline_check(check: &GameTurnCheck) -> Html {
    html! {
        <div class="game-inline-mechanic game-inline-check">
            <div class="check-item">
                <div class="check-item-header">
                    <div class="check-label">{ &check.label }</div>
                    if !check.rolls.is_empty() {
                        <DiceRollDisplay
                            rolls={check.rolls.clone()}
                            dice_expr={Some(check.dice_expr.clone())}
                            modifier={Some(check.modifier)}
                            total={Some(check.total)}
                            tier={check.tier}
                            class="check-roll"
                        />
                    }
                </div>
                <div class="muted">{ format!("{} {}", check.skill, crate::dice_ui::format_modifier(check.modifier)) }</div>
                if !check.stakes.is_empty() {
                    <div class="muted">{ &check.stakes }</div>
                }
            </div>
        </div>
    }
}

fn mechanical_result_summary(result: &MechanicalResult) -> String {
    match (&result.kind, &result.data) {
        (
            MechanicalKind::BoardMove,
            MechanicalData::BoardMove {
                actor,
                roll,
                from_space,
                to_space,
                space_tags,
                ..
            },
        ) => {
            format!(
                "{actor} rolled {roll}: space {from_space} → {to_space} ({})",
                space_tags.join(", ")
            )
        }
        (
            MechanicalKind::CardDraw,
            MechanicalData::CardDraw {
                name,
                text,
                deck_id,
                ..
            },
        ) => {
            let snippet: String = text.chars().take(120).collect();
            format!("Card ({deck_id}): {name} — {snippet}")
        }
        (
            MechanicalKind::DiceRoll,
            MechanicalData::DiceRoll {
                dice_expr,
                rolls,
                total,
                ..
            },
        ) => {
            format!("{} {dice_expr}: {rolls:?} = {total}", result.label)
        }
        _ => result.label.clone(),
    }
}

#[allow(dead_code)]
pub fn merge_game_list(detail: &GameDetail, games: &[Game]) -> Vec<Game> {
    game_sync::game_list_with_detail(games, detail)
}

mod game_sync {
    pub use crate::game_sync::*;
}
