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
}

#[derive(Clone, Copy)]
struct PrimaryMode {
    mode: AppMode,
    label: &'static str,
    glyph: &'static str,
}

const PRIMARY_MODES: [PrimaryMode; 3] = [
    PrimaryMode {
        mode: AppMode::Chats,
        label: "Chats",
        glyph: "💬",
    },
    PrimaryMode {
        mode: AppMode::Stories,
        label: "Stories",
        glyph: "📖",
    },
    PrimaryMode {
        mode: AppMode::Game,
        label: "Games",
        glyph: "🎲",
    },
];

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
            <div class="mode-bar-lead">
                <div class="mode-bar-brand">
                    <span class="mode-bar-title">{"Dreamwell"}</span>
                    <BuildInfo />
                </div>
                <nav class="mode-bar-primary" aria-label="Primary navigation">
                    { for PRIMARY_MODES.iter().map(|entry| {
                        html! {
                            <button
                                type="button"
                                class={classes!(
                                    "mode-btn",
                                    "mode-btn-icon",
                                    "mode-tab",
                                    primary_mode_active(props.mode, entry.mode).then_some("active"),
                                )}
                                title={entry.label}
                                aria-label={entry.label}
                                onclick={props.on_mode.reform(move |_| entry.mode)}
                            >
                                <span class="mode-btn-icon-glyph">{ entry.glyph }</span>
                            </button>
                        }
                    }) }
                </nav>
            </div>
            <div class="mode-bar-actions">
                <TopBarQueueButton
                    queue={props.queue.clone()}
                    active={props.mode == AppMode::Queue}
                    on_open={props.on_open_queue.clone()}
                />
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
            { for PRIMARY_MODES.iter().map(|entry| {
                html! {
                    <button
                        type="button"
                        class={classes!(
                            "bottom-nav-btn",
                            primary_mode_active(props.mode, entry.mode).then_some("active"),
                        )}
                        title={entry.label}
                        aria-label={entry.label}
                        onclick={props.on_mode.reform(move |_| entry.mode)}
                    >
                        <span class="bottom-nav-glyph">{ entry.glyph }</span>
                    </button>
                }
            }) }
        </nav>
    }
}

#[function_component(BuildInfo)]
fn build_info() -> Html {
    let sha = crate::build_info::GIT_SHA;
    let short = &sha[..sha.len().min(7)];
    html! {
        <span class="mode-bar-version muted" title={format!("Build {sha}")}>{ short }</span>
    }
}
