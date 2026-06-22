use dreamwell_types::*;
use yew::prelude::*;

use crate::generation_ui::job_status_badge;

#[derive(Properties, PartialEq)]
pub struct ChatListProps {
    pub chats: Vec<Chat>,
    pub on_select: Callback<i64>,
}

#[function_component(ChatList)]
pub fn chat_list(props: &ChatListProps) -> Html {
    html! {
        <div class="main-list">
            { for props.chats.iter().map(|chat| {
                let id = chat.id;
                let status = chat
                    .active_job
                    .as_ref()
                    .and_then(|job| job_status_badge(job, chat.queued_jobs));
                html! {
                    <div key={id} class="chat-item" onclick={props.on_select.reform(move |_| id)}>
                        <div class="chat-item-title">{ &chat.title }</div>
                        <div class="chat-character">{ &chat.character_name }</div>
                        if let Some(status) = status {
                            <span class={classes!("badge", status.variant_class())}>{ status.label }</span>
                        }
                    </div>
                }
            }) }
        </div>
    }
}

#[derive(Properties, PartialEq)]
pub struct StoryListProps {
    pub stories: Vec<Story>,
    pub on_select: Callback<i64>,
}

#[function_component(StoryList)]
pub fn story_list(props: &StoryListProps) -> Html {
    html! {
        <div class="main-list">
            { for props.stories.iter().map(|story| {
                let id = story.id;
                let status = story
                    .active_job
                    .as_ref()
                    .and_then(|job| job_status_badge(job, story.queued_jobs));
                html! {
                    <div key={id} class="chat-item" onclick={props.on_select.reform(move |_| id)}>
                        <div class="chat-item-title">{ &story.title }</div>
                        if let Some(status) = status {
                            <span class={classes!("badge", status.variant_class())}>{ status.label }</span>
                        }
                    </div>
                }
            }) }
        </div>
    }
}

#[derive(Properties, PartialEq)]
pub struct GameListProps {
    pub games: Vec<Game>,
    pub on_select: Callback<i64>,
}

#[function_component(GameList)]
pub fn game_list(props: &GameListProps) -> Html {
    html! {
        <div class="main-list">
            { for props.games.iter().map(|game| {
                let id = game.id;
                let status = game
                    .active_job
                    .as_ref()
                    .and_then(|job| job_status_badge(job, game.queued_jobs));
                html! {
                    <div key={id} class="chat-item" onclick={props.on_select.reform(move |_| id)}>
                        <div class="chat-item-title">{ &game.title }</div>
                        if let Some(status) = status {
                            <span class={classes!("badge", status.variant_class())}>{ status.label }</span>
                        }
                    </div>
                }
            }) }
        </div>
    }
}
