use wasm_bindgen::closure::Closure;
use wasm_bindgen::JsCast;
use yew::prelude::*;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Overlay {
    Variables,
    NewChat,
    NewGame,
    State,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StoryNav {
    /// Story selected with all outline sections collapsed.
    None,
    Basics,
    Chapter(i64),
    Beat {
        chapter_id: i64,
        beat_id: i64,
    },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum AppRoute {
    Chats {
        chat_id: Option<i64>,
        overlay: Option<Overlay>,
    },
    Stories {
        story_id: Option<i64>,
        nav: StoryNav,
        reading: bool,
        overlay: Option<Overlay>,
    },
    Games {
        game_id: Option<i64>,
        overlay: Option<Overlay>,
    },
    Queue,
    Settings,
    Characters {
        character_id: Option<i64>,
        chat_id: Option<i64>,
    },
    Scenarios {
        scenario_id: Option<i64>,
        game_id: Option<i64>,
    },
}

impl Default for AppRoute {
    fn default() -> Self {
        Self::Chats {
            chat_id: None,
            overlay: None,
        }
    }
}

impl AppRoute {
    pub fn mode(&self) -> crate::queue_ui::AppMode {
        match self {
            Self::Chats { .. } => crate::queue_ui::AppMode::Chats,
            Self::Stories { .. } => crate::queue_ui::AppMode::Stories,
            Self::Games { .. } => crate::queue_ui::AppMode::Game,
            Self::Queue { .. } => crate::queue_ui::AppMode::Queue,
            Self::Settings { .. } => crate::queue_ui::AppMode::Settings,
            Self::Characters { .. } => crate::queue_ui::AppMode::Characters,
            Self::Scenarios { .. } => crate::queue_ui::AppMode::Scenarios,
        }
    }

    pub fn overlay(&self) -> Option<Overlay> {
        match self {
            Self::Chats { overlay, .. } | Self::Stories { overlay, .. } => *overlay,
            Self::Games { overlay, .. } => *overlay,
            Self::Queue { .. }
            | Self::Settings { .. }
            | Self::Characters { .. }
            | Self::Scenarios { .. } => None,
        }
    }

    #[allow(dead_code)]
    pub fn without_overlay(&self) -> Self {
        match self {
            Self::Chats { chat_id, .. } => Self::Chats {
                chat_id: *chat_id,
                overlay: None,
            },
            Self::Stories {
                story_id,
                nav,
                reading,
                ..
            } => Self::Stories {
                story_id: *story_id,
                nav: *nav,
                reading: *reading,
                overlay: None,
            },
            Self::Games { game_id, .. } => Self::Games {
                game_id: *game_id,
                overlay: None,
            },
            Self::Queue => Self::Queue,
            Self::Settings => Self::Settings,
            Self::Characters {
                character_id,
                chat_id,
            } => Self::Characters {
                character_id: *character_id,
                chat_id: *chat_id,
            },
            Self::Scenarios {
                scenario_id,
                game_id,
            } => Self::Scenarios {
                scenario_id: *scenario_id,
                game_id: *game_id,
            },
        }
    }

    pub fn with_overlay(self, overlay: Overlay) -> Self {
        match self {
            Self::Chats { chat_id, .. } => Self::Chats {
                chat_id,
                overlay: Some(overlay),
            },
            Self::Stories {
                story_id,
                nav,
                reading,
                ..
            } => Self::Stories {
                story_id,
                nav,
                reading,
                overlay: Some(overlay),
            },
            Self::Games { game_id, .. } => Self::Games {
                game_id,
                overlay: Some(overlay),
            },
            Self::Queue { .. }
            | Self::Settings { .. }
            | Self::Characters { .. }
            | Self::Scenarios { .. } => self,
        }
    }

    pub fn to_path(&self) -> String {
        match self {
            Self::Chats { chat_id, overlay } => chats_to_path(*chat_id, *overlay),
            Self::Stories {
                story_id,
                nav,
                reading,
                overlay,
            } => stories_to_path(*story_id, *nav, *reading, *overlay),
            Self::Games { game_id, overlay } => games_to_path(*game_id, *overlay),
            Self::Queue => "/queue".to_string(),
            Self::Settings => "/settings".to_string(),
            Self::Characters {
                character_id,
                chat_id,
            } => characters_to_path(*character_id, *chat_id),
            Self::Scenarios {
                scenario_id,
                game_id,
            } => scenarios_to_path(*scenario_id, *game_id),
        }
    }
}

fn stories_to_path(
    story_id: Option<i64>,
    nav: StoryNav,
    reading: bool,
    overlay: Option<Overlay>,
) -> String {
    match story_id {
        None => match overlay {
            Some(overlay) => format!("/stories/{}", overlay_segment(overlay)),
            None => "/stories".to_string(),
        },
        Some(id) => {
            let mut parts = vec![format!("/stories/{id}")];
            if reading {
                parts.push("reading".to_string());
            } else {
                match nav {
                    StoryNav::None => {}
                    StoryNav::Basics => parts.push("basics".to_string()),
                    StoryNav::Chapter(chapter_id) => {
                        parts.push("chapters".to_string());
                        parts.push(chapter_id.to_string());
                    }
                    StoryNav::Beat {
                        chapter_id,
                        beat_id,
                    } => {
                        parts.push("chapters".to_string());
                        parts.push(chapter_id.to_string());
                        parts.push("beats".to_string());
                        parts.push(beat_id.to_string());
                    }
                }
            }
            if let Some(overlay) = overlay {
                parts.push(overlay_segment(overlay).to_string());
            }
            parts.join("/")
        }
    }
}

fn games_to_path(game_id: Option<i64>, overlay: Option<Overlay>) -> String {
    match (game_id, overlay) {
        (None, None) => "/games".to_string(),
        (None, Some(Overlay::NewGame)) => "/games/new".to_string(),
        (None, Some(overlay)) => format!("/games/{}", overlay_segment(overlay)),
        (Some(id), None) => format!("/games/{id}"),
        (Some(id), Some(overlay)) => format!("/games/{id}/{}", overlay_segment(overlay)),
    }
}

fn characters_to_path(character_id: Option<i64>, chat_id: Option<i64>) -> String {
    match (character_id, chat_id) {
        (None, None) => "/characters".to_string(),
        (Some(id), None) => format!("/characters/{id}"),
        (None, Some(chat_id)) => format!("/characters/chat/{chat_id}"),
        (Some(id), Some(chat_id)) => format!("/characters/{id}/chat/{chat_id}"),
    }
}

fn scenarios_to_path(scenario_id: Option<i64>, game_id: Option<i64>) -> String {
    match (scenario_id, game_id) {
        (None, None) => "/scenarios".to_string(),
        (Some(id), None) => format!("/scenarios/{id}"),
        (None, Some(game_id)) => format!("/scenarios/game/{game_id}"),
        (Some(id), Some(game_id)) => format!("/scenarios/{id}/game/{game_id}"),
    }
}

fn stories_route(
    story_id: Option<i64>,
    nav: StoryNav,
    reading: bool,
    overlay: Option<Overlay>,
) -> AppRoute {
    AppRoute::Stories {
        story_id,
        nav,
        reading,
        overlay,
    }
}

fn overlay_segment(overlay: Overlay) -> &'static str {
    match overlay {
        Overlay::Variables => "variables",
        Overlay::NewChat => "new",
        Overlay::NewGame => "new",
        Overlay::State => "state",
    }
}

fn scenarios_route(scenario_id: Option<i64>, game_id: Option<i64>) -> AppRoute {
    AppRoute::Scenarios {
        scenario_id,
        game_id,
    }
}

fn characters_route(character_id: Option<i64>, chat_id: Option<i64>) -> AppRoute {
    AppRoute::Characters {
        character_id,
        chat_id,
    }
}

fn route_if_settings(segments: &[&str]) -> Option<AppRoute> {
    segments.contains(&"settings").then_some(AppRoute::Settings)
}

fn chats_to_path(chat_id: Option<i64>, overlay: Option<Overlay>) -> String {
    let base = match (chat_id, overlay) {
        (None, Some(Overlay::NewChat)) => "/chats/new".to_string(),
        (None, None) => "/chats".to_string(),
        (None, Some(overlay)) => format!("/chats/{}", overlay_segment(overlay)),
        (Some(id), None) => format!("/chats/{id}"),
        (Some(id), Some(overlay)) => format!("/chats/{id}/{}", overlay_segment(overlay)),
    };
    base
}

pub fn parse_path(path: &str) -> AppRoute {
    let path = path.trim();
    let path = path.strip_prefix('/').unwrap_or(path);
    if path.is_empty() {
        return AppRoute::default();
    }

    let segments: Vec<&str> = path.split('/').filter(|s| !s.is_empty()).collect();
    match segments.first().copied() {
        Some("settings") => AppRoute::Settings,
        Some("queue") => parse_queue(&segments[1..]),
        Some("characters") => parse_characters(&segments[1..]),
        Some("scenarios") => parse_scenarios(&segments[1..]),
        Some("stories") => parse_stories(&segments[1..]),
        Some("games") => parse_games(&segments[1..]),
        Some("chats") | None => parse_chats(segments.get(1..).unwrap_or(&[])),
        _ => AppRoute::default(),
    }
}

fn parse_id(value: &str) -> Option<i64> {
    value.parse().ok()
}

fn parse_overlay(value: &str) -> Option<Overlay> {
    match value {
        "variables" => Some(Overlay::Variables),
        "new" => Some(Overlay::NewChat),
        "state" => Some(Overlay::State),
        _ => None,
    }
}

fn parse_scenarios(segments: &[&str]) -> AppRoute {
    let segments = strip_legacy_suffix(segments);
    match segments {
        [] => scenarios_route(None, None),
        [id] if parse_id(id).is_some() => scenarios_route(parse_id(id), None),
        ["game", game_id] if parse_id(game_id).is_some() => {
            scenarios_route(None, parse_id(game_id))
        }
        [id, "game", game_id] if parse_id(id).is_some() && parse_id(game_id).is_some() => {
            scenarios_route(parse_id(id), parse_id(game_id))
        }
        _ => scenarios_route(None, None),
    }
}

fn parse_characters(segments: &[&str]) -> AppRoute {
    let segments = strip_legacy_suffix(segments);
    match segments {
        [] => characters_route(None, None),
        [id] if parse_id(id).is_some() => characters_route(parse_id(id), None),
        ["chat", chat_id] if parse_id(chat_id).is_some() => {
            characters_route(None, parse_id(chat_id))
        }
        [id, "chat", chat_id] if parse_id(id).is_some() && parse_id(chat_id).is_some() => {
            characters_route(parse_id(id), parse_id(chat_id))
        }
        _ => characters_route(None, None),
    }
}

fn parse_chats(segments: &[&str]) -> AppRoute {
    if let Some(route) = route_if_settings(segments) {
        return route;
    }
    let segments = strip_legacy_suffix(segments);
    match segments {
        [] => AppRoute::Chats {
            chat_id: None,
            overlay: None,
        },
        ["new"] => AppRoute::Chats {
            chat_id: None,
            overlay: Some(Overlay::NewChat),
        },
        ["character"] => characters_route(None, None),
        [overlay] if parse_overlay(overlay).is_some() => AppRoute::Chats {
            chat_id: None,
            overlay: parse_overlay(overlay),
        },
        [id] if parse_id(id).is_some() => AppRoute::Chats {
            chat_id: parse_id(id),
            overlay: None,
        },
        [id, "character"] if parse_id(id).is_some() => characters_route(None, parse_id(id)),
        [id, overlay] if parse_id(id).is_some() && parse_overlay(overlay).is_some() => {
            AppRoute::Chats {
                chat_id: parse_id(id),
                overlay: parse_overlay(overlay),
            }
        }
        _ => AppRoute::default(),
    }
}

fn parse_games(segments: &[&str]) -> AppRoute {
    let segments = strip_legacy_suffix(segments);
    match segments {
        [] => AppRoute::Games {
            game_id: None,
            overlay: None,
        },
        ["scenario"] => scenarios_route(None, None),
        ["new"] => AppRoute::Games {
            game_id: None,
            overlay: Some(Overlay::NewGame),
        },
        [id] if parse_id(id).is_some() => AppRoute::Games {
            game_id: parse_id(id),
            overlay: None,
        },
        [id, "scenario"] if parse_id(id).is_some() => scenarios_route(None, parse_id(id)),
        [id, overlay] if parse_id(id).is_some() && parse_game_overlay(overlay).is_some() => {
            AppRoute::Games {
                game_id: parse_id(id),
                overlay: parse_game_overlay(overlay),
            }
        }
        _ => AppRoute::Games {
            game_id: None,
            overlay: None,
        },
    }
}

fn parse_game_overlay(value: &str) -> Option<Overlay> {
    match value {
        "variables" => Some(Overlay::Variables),
        "state" => Some(Overlay::State),
        _ => None,
    }
}

fn strip_legacy_suffix<'a>(segments: &'a [&'a str]) -> &'a [&'a str] {
    if segments.last() == Some(&"sidebar") {
        &segments[..segments.len() - 1]
    } else {
        segments
    }
}

fn parse_stories(segments: &[&str]) -> AppRoute {
    if let Some(route) = route_if_settings(segments) {
        return route;
    }
    let segments = strip_legacy_suffix(segments);
    match segments {
        [] => stories_route(None, StoryNav::None, false, None),
        ["character"] => characters_route(None, None),
        [overlay] if parse_overlay(overlay).is_some() => {
            stories_route(None, StoryNav::None, false, parse_overlay(overlay))
        }
        [id, rest @ ..] if parse_id(id).is_some() => {
            parse_story_detail(parse_id(id).unwrap(), rest)
        }
        _ => stories_route(None, StoryNav::None, false, None),
    }
}

fn parse_story_detail(story_id: i64, segments: &[&str]) -> AppRoute {
    if segments.is_empty() {
        return stories_route(Some(story_id), StoryNav::None, false, None);
    }
    if segments[0] == "character" {
        return characters_route(None, Some(story_id));
    }
    let (segments, reading) = if segments[0] == "reading" {
        (&segments[1..], true)
    } else {
        (segments, false)
    };
    if segments.is_empty() {
        return stories_route(Some(story_id), StoryNav::None, reading, None);
    }
    if let Some(overlay) = parse_overlay(segments.last().copied().unwrap_or("")) {
        if segments.len() == 1 {
            return stories_route(Some(story_id), StoryNav::None, reading, Some(overlay));
        }
        if segments.len() == 2 && parse_overlay(segments[1]).is_some() {
            return stories_route(Some(story_id), StoryNav::None, reading, Some(overlay));
        }
    }
    if segments == ["basics"] {
        return stories_route(Some(story_id), StoryNav::Basics, reading, None);
    }
    if segments.len() == 2 && segments[0] == "basics" {
        if let Some(overlay) = parse_overlay(segments[1]) {
            return stories_route(Some(story_id), StoryNav::Basics, reading, Some(overlay));
        }
    }
    if segments.len() >= 2 && segments[0] == "chapters" {
        if let Some(chapter_id) = parse_id(segments[1]) {
            if segments.len() == 2 {
                return stories_route(Some(story_id), StoryNav::Chapter(chapter_id), reading, None);
            }
            if segments.len() == 3 {
                if let Some(overlay) = parse_overlay(segments[2]) {
                    return stories_route(
                        Some(story_id),
                        StoryNav::Chapter(chapter_id),
                        reading,
                        Some(overlay),
                    );
                }
            }
            if segments.len() >= 4 && segments[2] == "beats" {
                if let Some(beat_id) = parse_id(segments[3]) {
                    let overlay = segments.get(4).and_then(|s| parse_overlay(s));
                    return stories_route(
                        Some(story_id),
                        StoryNav::Beat {
                            chapter_id,
                            beat_id,
                        },
                        reading,
                        overlay,
                    );
                }
            }
        }
    }
    if let Some(overlay) = parse_overlay(segments[0]) {
        return stories_route(Some(story_id), StoryNav::None, reading, Some(overlay));
    }
    stories_route(Some(story_id), StoryNav::None, reading, None)
}

fn parse_queue(segments: &[&str]) -> AppRoute {
    if let Some(route) = route_if_settings(segments) {
        return route;
    }
    let _ = strip_legacy_suffix(segments);
    AppRoute::Queue
}

pub fn current_route() -> AppRoute {
    web_sys::window()
        .and_then(|w| w.location().pathname().ok())
        .map(|path| parse_path(&path))
        .unwrap_or_default()
}

pub fn set_path(route: &AppRoute, push: bool) {
    let path = route.to_path();
    if let Some(window) = web_sys::window() {
        let history = window.history().ok();
        let location = window.location();
        if let (Some(history), Ok(current)) = (history, location.pathname()) {
            if current == path {
                return;
            }
            let state = &wasm_bindgen::JsValue::NULL;
            let title = "";
            let _ = if push {
                history.push_state_with_url(state, title, Some(&path))
            } else {
                history.replace_state_with_url(state, title, Some(&path))
            };
        }
    }
}

pub fn history_back() {
    if let Some(window) = web_sys::window() {
        if let Ok(history) = window.history() {
            let _ = history.back();
        }
    }
}

#[derive(Clone)]
pub struct RouterHandle {
    route: UseStateHandle<AppRoute>,
}

impl RouterHandle {
    pub fn route(&self) -> AppRoute {
        (*self.route).clone()
    }

    pub fn navigate(&self, route: AppRoute, push: bool) {
        set_path(&route, push);
        self.route.set(route);
    }

    pub fn back(&self) {
        history_back();
    }
}

#[hook]
pub fn use_router() -> RouterHandle {
    let route = use_state(current_route);

    {
        let route = route.clone();
        use_effect_with((), move |_| {
            let route = route.clone();
            let callback = Closure::wrap(Box::new(move |_event: web_sys::Event| {
                route.set(current_route());
            }) as Box<dyn FnMut(_)>);

            if let Some(window) = web_sys::window() {
                let _ = window.add_event_listener_with_callback(
                    "popstate",
                    callback.as_ref().unchecked_ref(),
                );
            }

            let current = current_route();
            set_path(&current, false);

            move || {
                if let Some(window) = web_sys::window() {
                    let _ = window.remove_event_listener_with_callback(
                        "popstate",
                        callback.as_ref().unchecked_ref(),
                    );
                }
            }
        });
    }

    RouterHandle { route }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trip_chats() {
        let routes = [
            AppRoute::Chats {
                chat_id: None,
                overlay: None,
            },
            AppRoute::Chats {
                chat_id: Some(42),
                overlay: None,
            },
            AppRoute::Chats {
                chat_id: Some(42),
                overlay: Some(Overlay::Variables),
            },
            AppRoute::Chats {
                chat_id: None,
                overlay: Some(Overlay::NewChat),
            },
            AppRoute::Chats {
                chat_id: Some(7),
                overlay: None,
            },
            AppRoute::Characters {
                character_id: None,
                chat_id: None,
            },
            AppRoute::Characters {
                character_id: Some(5),
                chat_id: None,
            },
            AppRoute::Characters {
                character_id: None,
                chat_id: Some(42),
            },
            AppRoute::Characters {
                character_id: Some(5),
                chat_id: Some(42),
            },
        ];
        for route in routes {
            let path = route.to_path();
            assert_eq!(parse_path(&path), route, "path: {path}");
        }
    }

    #[test]
    fn round_trip_stories() {
        let routes = [
            AppRoute::Stories {
                story_id: Some(3),
                nav: StoryNav::None,
                reading: false,
                overlay: None,
            },
            AppRoute::Stories {
                story_id: Some(3),
                nav: StoryNav::Basics,
                reading: false,
                overlay: None,
            },
            AppRoute::Stories {
                story_id: Some(3),
                nav: StoryNav::Chapter(9),
                reading: false,
                overlay: None,
            },
            AppRoute::Stories {
                story_id: Some(3),
                nav: StoryNav::Beat {
                    chapter_id: 9,
                    beat_id: 12,
                },
                reading: false,
                overlay: Some(Overlay::Variables),
            },
            AppRoute::Stories {
                story_id: Some(3),
                nav: StoryNav::None,
                reading: true,
                overlay: None,
            },
            AppRoute::Stories {
                story_id: Some(3),
                nav: StoryNav::None,
                reading: true,
                overlay: Some(Overlay::Variables),
            },
        ];
        for route in routes {
            let path = route.to_path();
            assert_eq!(parse_path(&path), route, "path: {path}");
        }
    }

    #[test]
    fn round_trip_games() {
        let routes = [
            AppRoute::Games {
                game_id: None,
                overlay: None,
            },
            AppRoute::Games {
                game_id: None,
                overlay: None,
            },
            AppRoute::Games {
                game_id: None,
                overlay: Some(Overlay::NewGame),
            },
            AppRoute::Games {
                game_id: Some(7),
                overlay: None,
            },
            AppRoute::Games {
                game_id: Some(7),
                overlay: None,
            },
            AppRoute::Games {
                game_id: Some(7),
                overlay: Some(Overlay::State),
            },
            AppRoute::Games {
                game_id: Some(7),
                overlay: Some(Overlay::State),
            },
        ];
        for route in routes {
            let path = route.to_path();
            assert_eq!(parse_path(&path), route, "path: {path}");
        }
    }

    #[test]
    fn round_trip_scenarios() {
        let routes = [
            AppRoute::Scenarios {
                scenario_id: None,
                game_id: None,
            },
            AppRoute::Scenarios {
                scenario_id: Some(4),
                game_id: None,
            },
            AppRoute::Scenarios {
                scenario_id: None,
                game_id: Some(9),
            },
            AppRoute::Scenarios {
                scenario_id: Some(4),
                game_id: Some(9),
            },
        ];
        for route in routes {
            let path = route.to_path();
            assert_eq!(parse_path(&path), route, "path: {path}");
        }
    }

    #[test]
    fn round_trip_queue() {
        let route = AppRoute::Queue;
        let path = route.to_path();
        assert_eq!(parse_path(&path), route, "path: {path}");
        assert_eq!(parse_path("/queue/sidebar"), route);
    }

    #[test]
    fn round_trip_settings() {
        let route = AppRoute::Settings;
        let path = route.to_path();
        assert_eq!(parse_path(&path), route, "path: {path}");
    }

    #[test]
    fn legacy_character_paths_redirect() {
        let legacy = [
            (
                "/chats/character",
                AppRoute::Characters {
                    character_id: None,
                    chat_id: None,
                },
            ),
            (
                "/chats/42/character",
                AppRoute::Characters {
                    character_id: None,
                    chat_id: Some(42),
                },
            ),
            (
                "/stories/character",
                AppRoute::Characters {
                    character_id: None,
                    chat_id: None,
                },
            ),
        ];
        for (path, expected) in legacy {
            assert_eq!(parse_path(path), expected, "path: {path}");
        }
    }

    #[test]
    fn legacy_settings_paths_redirect() {
        let legacy = [
            "/chats/settings",
            "/chats/1/settings",
            "/queue/settings",
            "/stories/3/settings",
            "/stories/3/basics/settings",
        ];
        for path in legacy {
            assert_eq!(parse_path(path), AppRoute::Settings, "path: {path}");
        }
    }
}
