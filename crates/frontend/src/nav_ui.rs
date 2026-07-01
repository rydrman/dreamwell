use yew::prelude::*;

use crate::queue_ui::{AppMode, TopBarQueueButton};
use dreamwell_types::QueueStatus;

#[derive(Properties, PartialEq)]
pub struct ModeBarProps {
    pub mode: AppMode,
    pub queue: Option<QueueStatus>,
    pub on_mode: Callback<AppMode>,
    pub on_open_settings: Callback<()>,
    pub on_open_queue: Callback<()>,
    pub on_open_characters: Callback<()>,
    pub on_open_scenarios: Callback<()>,
}

fn primary_mode_active(mode: AppMode, target: AppMode) -> bool {
    matches!(
        (mode, target),
        (AppMode::Chats, AppMode::Chats)
            | (AppMode::Stories, AppMode::Stories)
            | (AppMode::Game, AppMode::Game)
    )
}

#[function_component(ModeBar)]
pub fn mode_bar(props: &ModeBarProps) -> Html {
    html! {
        <div class="mode-bar">
            <div class="mode-bar-start">
                <div class="mode-bar-brand">
                    <span class="mode-bar-title">{"Dreamwell"}</span>
                    <BuildInfo />
                </div>
            </div>
            <nav class="mode-bar-primary" aria-label="Primary navigation">
                <button
                    type="button"
                    class={classes!("mode-tab", primary_mode_active(props.mode, AppMode::Chats).then_some("active"))}
                    onclick={props.on_mode.reform(|_| AppMode::Chats)}
                >
                    {"Chats"}
                </button>
                <button
                    type="button"
                    class={classes!("mode-tab", primary_mode_active(props.mode, AppMode::Stories).then_some("active"))}
                    onclick={props.on_mode.reform(|_| AppMode::Stories)}
                >
                    {"Stories"}
                </button>
                <button
                    type="button"
                    class={classes!("mode-tab", primary_mode_active(props.mode, AppMode::Game).then_some("active"))}
                    onclick={props.on_mode.reform(|_| AppMode::Game)}
                >
                    {"Games"}
                </button>
            </nav>
            <div class="mode-bar-actions">
                <TopBarQueueButton
                    queue={props.queue.clone()}
                    active={props.mode == AppMode::Queue}
                    on_open={props.on_open_queue.clone()}
                />
                <button
                    type="button"
                    class={classes!("mode-btn", "mode-btn-icon", (props.mode == AppMode::Characters).then_some("active"))}
                    title="Characters"
                    aria-label="Characters"
                    onclick={props.on_open_characters.reform(|_| ())}
                >
                    <span class="mode-btn-icon-glyph">{"🎭"}</span>
                </button>
                <button
                    type="button"
                    class={classes!("mode-btn", "mode-btn-icon", (props.mode == AppMode::Scenarios).then_some("active"))}
                    title="Scenarios"
                    aria-label="Scenarios"
                    onclick={props.on_open_scenarios.reform(|_| ())}
                >
                    <span class="mode-btn-icon-glyph">{"🗺"}</span>
                </button>
                <button
                    type="button"
                    class={classes!("mode-btn", "mode-btn-icon", (props.mode == AppMode::Settings).then_some("active"))}
                    title="Settings"
                    aria-label="Settings"
                    onclick={props.on_open_settings.reform(|_| ())}
                >
                    <span class="mode-btn-icon-glyph">{"⚙"}</span>
                </button>
            </div>
        </div>
    }
}

#[derive(Properties, PartialEq)]
pub struct BottomNavProps {
    pub mode: AppMode,
    pub on_mode: Callback<AppMode>,
}

#[function_component(BottomNav)]
pub fn bottom_nav(props: &BottomNavProps) -> Html {
    html! {
        <nav class="bottom-nav" aria-label="Primary navigation">
            <button
                type="button"
                class={classes!("bottom-nav-btn", primary_mode_active(props.mode, AppMode::Chats).then_some("active"))}
                onclick={props.on_mode.reform(|_| AppMode::Chats)}
            >
                {"Chats"}
            </button>
            <button
                type="button"
                class={classes!("bottom-nav-btn", primary_mode_active(props.mode, AppMode::Stories).then_some("active"))}
                onclick={props.on_mode.reform(|_| AppMode::Stories)}
            >
                {"Stories"}
            </button>
            <button
                type="button"
                class={classes!("bottom-nav-btn", primary_mode_active(props.mode, AppMode::Game).then_some("active"))}
                onclick={props.on_mode.reform(|_| AppMode::Game)}
            >
                {"Games"}
            </button>
        </nav>
    }
}

#[function_component(BuildInfo)]
fn build_info() -> Html {
    let sha = option_env!("GIT_SHA").unwrap_or("dev");
    let short = &sha[..sha.len().min(7)];
    html! {
        <span class="mode-bar-version muted" title={format!("Build {sha}")}>{ short }</span>
    }
}
