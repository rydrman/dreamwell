use dreamwell_types::*;
use yew::prelude::*;

use crate::generation_ui::job_status_badge;

fn archive_panel(archive_open: &UseStateHandle<bool>, archived_count: usize, body: Html) -> Html {
    if archived_count == 0 {
        return html! {};
    }
    html! {
        <div class="archive-panel main-list-archive">
            <button class="archive-toggle" onclick={{
                let archive_open = archive_open.clone();
                Callback::from(move |_| archive_open.set(!*archive_open))
            }}>
                <span>{ if **archive_open { "▾" } else { "▸" } }</span>
                <span>{ format!("Archive ({archived_count})") }</span>
            </button>
            if **archive_open {
                <div class="archive-list">{ body }</div>
            }
        </div>
    }
}

#[derive(Properties, PartialEq)]
pub struct ChatListProps {
    pub chats: Vec<Chat>,
    pub archived: Vec<Chat>,
    pub on_select: Callback<i64>,
    pub on_archive: Callback<i64>,
    pub on_restore: Callback<i64>,
    pub on_permanent_delete: Callback<i64>,
}

#[function_component(ChatList)]
pub fn chat_list(props: &ChatListProps) -> Html {
    let archive_open = use_state(|| false);
    html! {
        <div class="main-list">
            { for props.chats.iter().map(|chat| {
                let id = chat.id;
                let status = chat.active_job.as_ref().and_then(|job| job_status_badge(job, chat.queued_jobs));
                html! {
                    <div key={id} class="chat-item list-item-row">
                        <div class="list-item-main" onclick={props.on_select.reform(move |_| id)}>
                            <div class="chat-item-title">{ &chat.title }</div>
                            <div class="chat-character">{ &chat.character_name }</div>
                            if let Some(status) = status {
                                <span class={classes!("badge", status.variant_class())}>{ status.label }</span>
                            }
                        </div>
                        <button class="btn secondary btn-compact" title="Archive chat" onclick={props.on_archive.reform(move |_| id)}>{"✕"}</button>
                    </div>
                }
            }) }
            { archive_panel(
                &archive_open,
                props.archived.len(),
                html! {
                    { for props.archived.iter().map(|chat| {
                        let id = chat.id;
                        let days_left = chat.archived_at.map(days_until_chat_archive_purge);
                        html! {
                            <div key={id} class="chat-item archived">
                                <div class="archive-item-title">{ &chat.title }</div>
                                <div class="chat-character">{ &chat.character_name }</div>
                                if let Some(days) = days_left {
                                    <div class="archive-meta muted">{ format!("{days} days left") }</div>
                                }
                                <div class="archive-actions">
                                    <button class="btn secondary btn-compact" onclick={props.on_restore.reform(move |_| id)}>{"Restore"}</button>
                                    <button class="btn secondary btn-compact text-danger" onclick={props.on_permanent_delete.reform(move |_| id)}>{"Delete"}</button>
                                </div>
                            </div>
                        }
                    }) }
                },
            ) }
        </div>
    }
}

#[derive(Properties, PartialEq)]
pub struct StoryListProps {
    pub stories: Vec<Story>,
    pub archived: Vec<Story>,
    pub on_select: Callback<i64>,
    pub on_archive: Callback<i64>,
    pub on_restore: Callback<i64>,
    pub on_permanent_delete: Callback<i64>,
}

#[function_component(StoryList)]
pub fn story_list(props: &StoryListProps) -> Html {
    let archive_open = use_state(|| false);
    html! {
        <div class="main-list">
            { for props.stories.iter().map(|story| {
                let id = story.id;
                let status = story.active_job.as_ref().and_then(|job| job_status_badge(job, story.queued_jobs));
                html! {
                    <div key={id} class="chat-item list-item-row">
                        <div class="list-item-main" onclick={props.on_select.reform(move |_| id)}>
                            <div class="chat-item-title">{ &story.title }</div>
                            if let Some(status) = status {
                                <span class={classes!("badge", status.variant_class())}>{ status.label }</span>
                            }
                        </div>
                        <button class="btn secondary btn-compact" title="Archive story" onclick={props.on_archive.reform(move |_| id)}>{"✕"}</button>
                    </div>
                }
            }) }
            { archive_panel(
                &archive_open,
                props.archived.len(),
                html! {
                    { for props.archived.iter().map(|story| {
                        let id = story.id;
                        let days_left = story.archived_at.map(days_until_chat_archive_purge);
                        html! {
                            <div key={id} class="chat-item archived">
                                <div class="archive-item-title">{ &story.title }</div>
                                if let Some(days) = days_left {
                                    <div class="archive-meta muted">{ format!("{days} days left") }</div>
                                }
                                <div class="archive-actions">
                                    <button class="btn secondary btn-compact" onclick={props.on_restore.reform(move |_| id)}>{"Restore"}</button>
                                    <button class="btn secondary btn-compact text-danger" onclick={props.on_permanent_delete.reform(move |_| id)}>{"Delete"}</button>
                                </div>
                            </div>
                        }
                    }) }
                },
            ) }
        </div>
    }
}

#[derive(Properties, PartialEq)]
pub struct GameListProps {
    pub games: Vec<Game>,
    pub archived: Vec<Game>,
    pub on_select: Callback<i64>,
    pub on_archive: Callback<i64>,
    pub on_restore: Callback<i64>,
    pub on_permanent_delete: Callback<i64>,
}

#[function_component(GameList)]
pub fn game_list(props: &GameListProps) -> Html {
    let archive_open = use_state(|| false);
    html! {
        <div class="main-list">
            { for props.games.iter().map(|game| {
                let id = game.id;
                let status = game.active_job.as_ref().and_then(|job| job_status_badge(job, game.queued_jobs));
                html! {
                    <div key={id} class="chat-item list-item-row">
                        <div class="list-item-main" onclick={props.on_select.reform(move |_| id)}>
                            <div class="chat-item-title">{ &game.title }</div>
                            if let Some(status) = status {
                                <span class={classes!("badge", status.variant_class())}>{ status.label }</span>
                            }
                        </div>
                        <button class="btn secondary btn-compact" title="Archive game" onclick={props.on_archive.reform(move |_| id)}>{"✕"}</button>
                    </div>
                }
            }) }
            { archive_panel(
                &archive_open,
                props.archived.len(),
                html! {
                    { for props.archived.iter().map(|game| {
                        let id = game.id;
                        let days_left = game.archived_at.map(days_until_chat_archive_purge);
                        html! {
                            <div key={id} class="chat-item archived">
                                <div class="archive-item-title">{ &game.title }</div>
                                if let Some(days) = days_left {
                                    <div class="archive-meta muted">{ format!("{days} days left") }</div>
                                }
                                <div class="archive-actions">
                                    <button class="btn secondary btn-compact" onclick={props.on_restore.reform(move |_| id)}>{"Restore"}</button>
                                    <button class="btn secondary btn-compact text-danger" onclick={props.on_permanent_delete.reform(move |_| id)}>{"Delete"}</button>
                                </div>
                            </div>
                        }
                    }) }
                },
            ) }
        </div>
    }
}
