use dreamwell_types::Scenario;
use yew::prelude::*;

use crate::api;

#[derive(Properties, PartialEq)]
pub struct GameCreateModalProps {
    pub on_close: Callback<()>,
    pub on_start: Callback<Scenario>,
    pub on_open_scenarios: Callback<()>,
}

#[function_component(GameCreateModal)]
pub fn game_create_modal(props: &GameCreateModalProps) -> Html {
    let scenarios = use_state(Vec::<Scenario>::new);
    let loading = use_state(|| true);

    {
        let scenarios = scenarios.clone();
        let loading = loading.clone();
        use_effect_with((), move |_| {
            let scenarios = scenarios.clone();
            let loading = loading.clone();
            wasm_bindgen_futures::spawn_local(async move {
                loading.set(true);
                if let Ok(list) = api::list_scenarios().await {
                    scenarios.set(list);
                }
                loading.set(false);
            });
            || ()
        });
    }

    html! {
        <>
            <div class="modal-backdrop" onclick={props.on_close.reform(|_| ())} />
            <div class="modal modal-wide">
                <h2>{"Start a game"}</h2>
                <p class="muted game-create-lead">{"Pick a scenario to play. Manage and import scenarios from the Scenarios page."}</p>

                if *loading {
                    <p class="muted">{"Loading scenarios…"}</p>
                } else if scenarios.is_empty() {
                    <div class="empty-state muted">
                        <p>{"No scenarios yet. Create or import one, then start a game from here."}</p>
                        <button class="btn" style="margin-top:0.75rem;" onclick={props.on_open_scenarios.reform(|_| ())}>
                            {"Open Scenarios"}
                        </button>
                    </div>
                } else {
                    <div class="modal-list game-create-scenario-list">
                        { for scenarios.iter().map(|scenario| {
                            let id = scenario.id;
                            let play = scenario.clone();
                            html! {
                                <div key={id} class="modal-item game-create-scenario-item">
                                    <div class="game-create-scenario-copy">
                                        <div class="chat-item-title">{ &scenario.title }</div>
                                        if !scenario.premise.trim().is_empty() {
                                            <p class="muted game-create-scenario-premise">{ &scenario.premise }</p>
                                        }
                                    </div>
                                    <button
                                        type="button"
                                        class="btn btn-compact"
                                        onclick={props.on_start.reform(move |_| play.clone())}
                                    >
                                        {"Start"}
                                    </button>
                                </div>
                            }
                        }) }
                    </div>
                }

                <div style="display:flex;gap:0.5rem;margin-top:0.75rem;flex-wrap:wrap;">
                    <button class="btn secondary" onclick={props.on_open_scenarios.reform(|_| ())}>{"Manage scenarios"}</button>
                    <button class="btn secondary" onclick={props.on_close.reform(|_| ())}>{"Cancel"}</button>
                </div>
            </div>
        </>
    }
}
