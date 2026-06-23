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
        sidebar: bool,
    },
    Stories {
        story_id: Option<i64>,
        nav: StoryNav,
        overlay: Option<Overlay>,
        sidebar: bool,
    },
    Games {
        game_id: Option<i64>,
        overlay: Option<Overlay>,
        sidebar: bool,
    },
    Queue {
        sidebar: bool,
    },
    Settings {
        sidebar: bool,
    },
    Characters {
        character_id: Option<i64>,
        chat_id: Option<i64>,
        sidebar: bool,
    },
    Scenarios {
        scenario_id: Option<i64>,
        game_id: Option<i64>,
        sidebar: bool,
    },
}

impl Default for AppRoute {
    fn default() -> Self {
        Self::Chats {
            chat_id: None,
            overlay: None,
            sidebar: false,
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
            Self::Chats {
                chat_id, sidebar, ..
            } => Self::Chats {
                chat_id: *chat_id,
                overlay: None,
                sidebar: *sidebar,
            },
            Self::Stories {
                story_id,
                nav,
                sidebar,
                ..
            } => Self::Stories {
                story_id: *story_id,
                nav: *nav,
                overlay: None,
                sidebar: *sidebar,
            },
            Self::Games {
                game_id, sidebar, ..
            } => Self::Games {
                game_id: *game_id,
                overlay: None,
                sidebar: *sidebar,
            },
            Self::Queue { sidebar } => Self::Queue { sidebar: *sidebar },
            Self::Settings { sidebar } => Self::Settings { sidebar: *sidebar },
            Self::Characters {
                character_id,
                chat_id,
                sidebar,
            } => Self::Characters {
                character_id: *character_id,
                chat_id: *chat_id,
                sidebar: *sidebar,
            },
            Self::Scenarios {
                scenario_id,
                game_id,
                sidebar,
            } => Self::Scenarios {
                scenario_id: *scenario_id,
                game_id: *game_id,
                sidebar: *sidebar,
            },
        }
    }

    pub fn with_overlay(self, overlay: Overlay) -> Self {
        match self {
            Self::Chats {
                chat_id, sidebar, ..
            } => Self::Chats {
                chat_id,
                overlay: Some(overlay),
                sidebar,
            },
            Self::Stories {
                story_id,
                nav,
                sidebar,
                ..
            } => Self::Stories {
                story_id,
                nav,
                overlay: Some(overlay),
                sidebar,
            },
            Self::Games {
                game_id, sidebar, ..
            } => Self::Games {
                game_id,
                overlay: Some(overlay),
                sidebar,
            },
            Self::Queue { .. }
            | Self::Settings { .. }
            | Self::Characters { .. }
            | Self::Scenarios { .. } => self,
        }
    }

    pub fn with_sidebar(self, sidebar: bool) -> Self {
        match self {
            Self::Chats {
                chat_id, overlay, ..
            } => Self::Chats {
                chat_id,
                overlay,
                sidebar,
            },
            Self::Stories {
                story_id,
                nav,
                overlay,
                ..
            } => Self::Stories {
                story_id,
                nav,
                overlay,
                sidebar,
            },
            Self::Games {
                game_id, overlay, ..
            } => Self::Games {
                game_id,
                overlay,
                sidebar,
            },
            Self::Queue { .. } => Self::Queue { sidebar },
            Self::Settings { .. } => Self::Settings { sidebar },
            Self::Characters {
                character_id,
                chat_id,
                ..
            } => Self::Characters {
                character_id,
                chat_id,
                sidebar,
            },
            Self::Scenarios {
                scenario_id,
                game_id,
                ..
            } => Self::Scenarios {
                scenario_id,
                game_id,
                sidebar,
            },
        }
    }

    pub fn to_path(&self) -> String {
        match self {
            Self::Chats {
                chat_id,
                overlay,
                sidebar,
            } => chats_to_path(*chat_id, *overlay, *sidebar),
            Self::Stories {
                story_id: None,
                nav: StoryNav::None | StoryNav::Basics,
                overlay: Some(overlay),
                sidebar: false,
            } => format!("/stories/{}", overlay_segment(*overlay)),
            Self::Stories {
                story_id: None,
                nav: StoryNav::None | StoryNav::Basics,
                overlay: None,
                sidebar: true,
            } => "/stories/sidebar".to_string(),
            Self::Stories {
                story_id: None,
                nav: StoryNav::None | StoryNav::Basics,
                overlay: None,
                sidebar: false,
            } => "/stories".to_string(),
            Self::Stories {
                story_id: Some(id),
                nav: StoryNav::None,
                overlay: Some(overlay),
                sidebar: false,
            } => format!("/stories/{id}/{}", overlay_segment(*overlay)),
            Self::Stories {
                story_id: Some(id),
                nav: StoryNav::None,
                overlay: None,
                sidebar: true,
            } => format!("/stories/{id}/sidebar"),
            Self::Stories {
                story_id: Some(id),
                nav: StoryNav::None,
                overlay: None,
                sidebar: false,
            } => format!("/stories/{id}"),
            Self::Stories {
                story_id: Some(id),
                nav: StoryNav::Basics,
                overlay: Some(overlay),
                sidebar: false,
            } => format!("/stories/{id}/basics/{}", overlay_segment(*overlay)),
            Self::Stories {
                story_id: Some(id),
                nav: StoryNav::Basics,
                overlay: None,
                sidebar: true,
            } => format!("/stories/{id}/basics/sidebar"),
            Self::Stories {
                story_id: Some(id),
                nav: StoryNav::Basics,
                overlay: None,
                sidebar: false,
            } => format!("/stories/{id}/basics"),
            Self::Stories {
                story_id: Some(id),
                nav: StoryNav::Chapter(chapter_id),
                overlay: Some(overlay),
                sidebar: false,
            } => format!(
                "/stories/{id}/chapters/{chapter_id}/{}",
                overlay_segment(*overlay)
            ),
            Self::Stories {
                story_id: Some(id),
                nav: StoryNav::Chapter(chapter_id),
                overlay: None,
                sidebar: true,
            } => format!("/stories/{id}/chapters/{chapter_id}/sidebar"),
            Self::Stories {
                story_id: Some(id),
                nav: StoryNav::Chapter(chapter_id),
                overlay: None,
                sidebar: false,
            } => format!("/stories/{id}/chapters/{chapter_id}"),
            Self::Stories {
                story_id: Some(id),
                nav:
                    StoryNav::Beat {
                        chapter_id,
                        beat_id,
                    },
                overlay: Some(overlay),
                sidebar: false,
            } => format!(
                "/stories/{id}/chapters/{chapter_id}/beats/{beat_id}/{}",
                overlay_segment(*overlay)
            ),
            Self::Stories {
                story_id: Some(id),
                nav:
                    StoryNav::Beat {
                        chapter_id,
                        beat_id,
                    },
                overlay: None,
                sidebar: true,
            } => format!("/stories/{id}/chapters/{chapter_id}/beats/{beat_id}/sidebar"),
            Self::Stories {
                story_id: Some(id),
                nav:
                    StoryNav::Beat {
                        chapter_id,
                        beat_id,
                    },
                overlay: None,
                sidebar: false,
            } => format!("/stories/{id}/chapters/{chapter_id}/beats/{beat_id}"),
            Self::Games {
                game_id: None,
                overlay: None,
                sidebar: false,
            } => "/games".to_string(),
            Self::Games {
                game_id: None,
                overlay: Some(Overlay::NewGame),
                sidebar: false,
            } => "/games/new".to_string(),
            Self::Games {
                game_id: None,
                overlay: Some(Overlay::NewGame),
                sidebar: true,
            } => "/games/new/sidebar".to_string(),
            Self::Games {
                game_id: None,
                overlay: None,
                sidebar: true,
            } => "/games/sidebar".to_string(),
            Self::Games {
                game_id: Some(id),
                overlay: None,
                sidebar: false,
            } => format!("/games/{id}"),
            Self::Games {
                game_id: Some(id),
                overlay: None,
                sidebar: true,
            } => format!("/games/{id}/sidebar"),
            Self::Games {
                game_id: Some(id),
                overlay: Some(overlay),
                sidebar: false,
            } => format!("/games/{id}/{}", overlay_segment(*overlay)),
            Self::Games {
                game_id: Some(id),
                overlay: Some(overlay),
                sidebar: true,
            } => format!("/games/{id}/{}/sidebar", overlay_segment(*overlay)),
            Self::Queue { sidebar: false } => "/queue".to_string(),
            Self::Queue { sidebar: true } => "/queue/sidebar".to_string(),
            Self::Settings { sidebar: false } => "/settings".to_string(),
            Self::Settings { sidebar: true } => "/settings/sidebar".to_string(),
            Self::Characters {
                character_id: None,
                chat_id: None,
                sidebar: false,
            } => "/characters".to_string(),
            Self::Characters {
                character_id: None,
                chat_id: None,
                sidebar: true,
            } => "/characters/sidebar".to_string(),
            Self::Characters {
                character_id: Some(id),
                chat_id: None,
                sidebar: false,
            } => format!("/characters/{id}"),
            Self::Characters {
                character_id: Some(id),
                chat_id: None,
                sidebar: true,
            } => format!("/characters/{id}/sidebar"),
            Self::Characters {
                character_id: None,
                chat_id: Some(chat_id),
                sidebar: false,
            } => format!("/characters/chat/{chat_id}"),
            Self::Characters {
                character_id: None,
                chat_id: Some(chat_id),
                sidebar: true,
            } => format!("/characters/chat/{chat_id}/sidebar"),
            Self::Characters {
                character_id: Some(id),
                chat_id: Some(chat_id),
                sidebar: false,
            } => format!("/characters/{id}/chat/{chat_id}"),
            Self::Characters {
                character_id: Some(id),
                chat_id: Some(chat_id),
                sidebar: true,
            } => format!("/characters/{id}/chat/{chat_id}/sidebar"),
            Self::Scenarios {
                scenario_id: None,
                game_id: None,
                sidebar: false,
            } => "/scenarios".to_string(),
            Self::Scenarios {
                scenario_id: None,
                game_id: None,
                sidebar: true,
            } => "/scenarios/sidebar".to_string(),
            Self::Scenarios {
                scenario_id: Some(id),
                game_id: None,
                sidebar: false,
            } => format!("/scenarios/{id}"),
            Self::Scenarios {
                scenario_id: Some(id),
                game_id: None,
                sidebar: true,
            } => format!("/scenarios/{id}/sidebar"),
            Self::Scenarios {
                scenario_id: None,
                game_id: Some(game_id),
                sidebar: false,
            } => format!("/scenarios/game/{game_id}"),
            Self::Scenarios {
                scenario_id: None,
                game_id: Some(game_id),
                sidebar: true,
            } => format!("/scenarios/game/{game_id}/sidebar"),
            Self::Scenarios {
                scenario_id: Some(id),
                game_id: Some(game_id),
                sidebar: false,
            } => format!("/scenarios/{id}/game/{game_id}"),
            Self::Scenarios {
                scenario_id: Some(id),
                game_id: Some(game_id),
                sidebar: true,
            } => format!("/scenarios/{id}/game/{game_id}/sidebar"),
            Self::Stories {
                story_id: None,
                nav: StoryNav::Chapter(_) | StoryNav::Beat { .. },
                overlay: None,
                sidebar: false,
            } => "/stories".to_string(),
            Self::Stories {
                story_id: None,
                nav: StoryNav::None | StoryNav::Basics,
                overlay: Some(overlay),
                sidebar: true,
            } => format!("/stories/{}/sidebar", overlay_segment(*overlay)),
            Self::Stories {
                story_id: Some(id),
                nav: StoryNav::None,
                overlay: Some(overlay),
                sidebar: true,
            } => format!("/stories/{id}/{}/sidebar", overlay_segment(*overlay)),
            Self::Stories {
                story_id: Some(id),
                nav: StoryNav::Basics,
                overlay: Some(overlay),
                sidebar: true,
            } => format!("/stories/{id}/basics/{}/sidebar", overlay_segment(*overlay)),
            Self::Stories {
                story_id: Some(id),
                nav: StoryNav::Chapter(chapter_id),
                overlay: Some(overlay),
                sidebar: true,
            } => format!(
                "/stories/{id}/chapters/{chapter_id}/{}/sidebar",
                overlay_segment(*overlay)
            ),
            Self::Stories {
                story_id: Some(id),
                nav:
                    StoryNav::Beat {
                        chapter_id,
                        beat_id,
                    },
                overlay: Some(overlay),
                sidebar: true,
            } => format!(
                "/stories/{id}/chapters/{chapter_id}/beats/{beat_id}/{}/sidebar",
                overlay_segment(*overlay)
            ),
            _ => "/chats".to_string(),
        }
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

fn scenarios_route(scenario_id: Option<i64>, game_id: Option<i64>, sidebar: bool) -> AppRoute {
    AppRoute::Scenarios {
        scenario_id,
        game_id,
        sidebar,
    }
}

fn characters_route(character_id: Option<i64>, chat_id: Option<i64>, sidebar: bool) -> AppRoute {
    AppRoute::Characters {
        character_id,
        chat_id,
        sidebar,
    }
}

fn route_if_settings(segments: &[&str]) -> Option<AppRoute> {
    segments
        .contains(&"settings")
        .then_some(AppRoute::Settings { sidebar: false })
}

fn chats_to_path(chat_id: Option<i64>, overlay: Option<Overlay>, sidebar: bool) -> String {
    let base = match (chat_id, overlay) {
        (None, Some(Overlay::NewChat)) => "/chats/new".to_string(),
        (None, None) => "/chats".to_string(),
        (None, Some(overlay)) => format!("/chats/{}", overlay_segment(overlay)),
        (Some(id), None) => format!("/chats/{id}"),
        (Some(id), Some(overlay)) => format!("/chats/{id}/{}", overlay_segment(overlay)),
    };
    if sidebar {
        format!("{base}/sidebar")
    } else {
        base
    }
}

pub fn parse_path(path: &str) -> AppRoute {
    let path = path.trim();
    let path = path.strip_prefix('/').unwrap_or(path);
    if path.is_empty() {
        return AppRoute::default();
    }

    let segments: Vec<&str> = path.split('/').filter(|s| !s.is_empty()).collect();
    match segments.first().copied() {
        Some("settings") => AppRoute::Settings {
            sidebar: segments.get(1) == Some(&"sidebar"),
        },
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
    let (segments, sidebar) = strip_sidebar_suffix(segments);
    match segments {
        [] => scenarios_route(None, None, sidebar),
        [id] if parse_id(id).is_some() => scenarios_route(parse_id(id), None, sidebar),
        ["game", game_id] if parse_id(game_id).is_some() => {
            scenarios_route(None, parse_id(game_id), sidebar)
        }
        [id, "game", game_id] if parse_id(id).is_some() && parse_id(game_id).is_some() => {
            scenarios_route(parse_id(id), parse_id(game_id), sidebar)
        }
        _ => scenarios_route(None, None, sidebar),
    }
}

fn parse_characters(segments: &[&str]) -> AppRoute {
    let (segments, sidebar) = strip_sidebar_suffix(segments);
    match segments {
        [] => characters_route(None, None, sidebar),
        [id] if parse_id(id).is_some() => characters_route(parse_id(id), None, sidebar),
        ["chat", chat_id] if parse_id(chat_id).is_some() => {
            characters_route(None, parse_id(chat_id), sidebar)
        }
        [id, "chat", chat_id] if parse_id(id).is_some() && parse_id(chat_id).is_some() => {
            characters_route(parse_id(id), parse_id(chat_id), sidebar)
        }
        _ => characters_route(None, None, sidebar),
    }
}

fn parse_chats(segments: &[&str]) -> AppRoute {
    if let Some(route) = route_if_settings(segments) {
        return route;
    }
    match segments {
        [] => AppRoute::Chats {
            chat_id: None,
            overlay: None,
            sidebar: false,
        },
        ["sidebar"] => AppRoute::Chats {
            chat_id: None,
            overlay: None,
            sidebar: true,
        },
        ["new"] => AppRoute::Chats {
            chat_id: None,
            overlay: Some(Overlay::NewChat),
            sidebar: false,
        },
        ["new", "sidebar"] => AppRoute::Chats {
            chat_id: None,
            overlay: Some(Overlay::NewChat),
            sidebar: true,
        },
        ["character"] => characters_route(None, None, false),
        ["character", "sidebar"] => characters_route(None, None, true),
        [overlay] if parse_overlay(overlay).is_some() => AppRoute::Chats {
            chat_id: None,
            overlay: parse_overlay(overlay),
            sidebar: false,
        },
        [overlay, "sidebar"] if parse_overlay(overlay).is_some() => AppRoute::Chats {
            chat_id: None,
            overlay: parse_overlay(overlay),
            sidebar: true,
        },
        [id] if parse_id(id).is_some() => AppRoute::Chats {
            chat_id: parse_id(id),
            overlay: None,
            sidebar: false,
        },
        [id, "sidebar"] if parse_id(id).is_some() => AppRoute::Chats {
            chat_id: parse_id(id),
            overlay: None,
            sidebar: true,
        },
        [id, "character"] if parse_id(id).is_some() => characters_route(None, parse_id(id), false),
        [id, "character", "sidebar"] if parse_id(id).is_some() => {
            characters_route(None, parse_id(id), true)
        }
        [id, overlay] if parse_id(id).is_some() && parse_overlay(overlay).is_some() => {
            AppRoute::Chats {
                chat_id: parse_id(id),
                overlay: parse_overlay(overlay),
                sidebar: false,
            }
        }
        [id, overlay, "sidebar"] if parse_id(id).is_some() && parse_overlay(overlay).is_some() => {
            AppRoute::Chats {
                chat_id: parse_id(id),
                overlay: parse_overlay(overlay),
                sidebar: true,
            }
        }
        _ => AppRoute::default(),
    }
}

fn parse_games(segments: &[&str]) -> AppRoute {
    let (segments, sidebar) = strip_sidebar_suffix(segments);
    match segments {
        [] => AppRoute::Games {
            game_id: None,
            overlay: None,
            sidebar,
        },
        ["scenario"] => scenarios_route(None, None, false),
        ["scenario", "sidebar"] => scenarios_route(None, None, true),
        ["new"] => AppRoute::Games {
            game_id: None,
            overlay: Some(Overlay::NewGame),
            sidebar,
        },
        [id] if parse_id(id).is_some() => AppRoute::Games {
            game_id: parse_id(id),
            overlay: None,
            sidebar,
        },
        [id, "scenario"] if parse_id(id).is_some() => scenarios_route(None, parse_id(id), false),
        [id, "scenario", "sidebar"] if parse_id(id).is_some() => {
            scenarios_route(None, parse_id(id), true)
        }
        [id, overlay] if parse_id(id).is_some() && parse_game_overlay(overlay).is_some() => {
            AppRoute::Games {
                game_id: parse_id(id),
                overlay: parse_game_overlay(overlay),
                sidebar,
            }
        }
        _ => AppRoute::Games {
            game_id: None,
            overlay: None,
            sidebar: false,
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

fn strip_sidebar_suffix<'a>(segments: &'a [&'a str]) -> (&'a [&'a str], bool) {
    if segments.last() == Some(&"sidebar") {
        (&segments[..segments.len() - 1], true)
    } else {
        (segments, false)
    }
}

fn parse_stories(segments: &[&str]) -> AppRoute {
    if let Some(route) = route_if_settings(segments) {
        return route;
    }
    match segments {
        [] => AppRoute::Stories {
            story_id: None,
            nav: StoryNav::None,
            overlay: None,
            sidebar: false,
        },
        ["sidebar"] => AppRoute::Stories {
            story_id: None,
            nav: StoryNav::None,
            overlay: None,
            sidebar: true,
        },
        ["character"] => characters_route(None, None, false),
        ["character", "sidebar"] => characters_route(None, None, true),
        [overlay] if parse_overlay(overlay).is_some() => AppRoute::Stories {
            story_id: None,
            nav: StoryNav::None,
            overlay: parse_overlay(overlay),
            sidebar: false,
        },
        [id] if parse_id(id).is_some() => AppRoute::Stories {
            story_id: parse_id(id),
            nav: StoryNav::None,
            overlay: None,
            sidebar: false,
        },
        [id, "sidebar"] if parse_id(id).is_some() => AppRoute::Stories {
            story_id: parse_id(id),
            nav: StoryNav::None,
            overlay: None,
            sidebar: true,
        },
        [id, "character"] if parse_id(id).is_some() => characters_route(None, parse_id(id), false),
        [id, "character", "sidebar"] if parse_id(id).is_some() => {
            characters_route(None, parse_id(id), true)
        }
        [id, overlay] if parse_id(id).is_some() && parse_overlay(overlay).is_some() => {
            AppRoute::Stories {
                story_id: parse_id(id),
                nav: StoryNav::None,
                overlay: parse_overlay(overlay),
                sidebar: false,
            }
        }
        [id, overlay, "sidebar"] if parse_id(id).is_some() && parse_overlay(overlay).is_some() => {
            AppRoute::Stories {
                story_id: parse_id(id),
                nav: StoryNav::None,
                overlay: parse_overlay(overlay),
                sidebar: true,
            }
        }
        [id, "basics"] if parse_id(id).is_some() => AppRoute::Stories {
            story_id: parse_id(id),
            nav: StoryNav::Basics,
            overlay: None,
            sidebar: false,
        },
        [id, "basics", "sidebar"] if parse_id(id).is_some() => AppRoute::Stories {
            story_id: parse_id(id),
            nav: StoryNav::Basics,
            overlay: None,
            sidebar: true,
        },
        [id, "basics", overlay] if parse_id(id).is_some() && parse_overlay(overlay).is_some() => {
            AppRoute::Stories {
                story_id: parse_id(id),
                nav: StoryNav::Basics,
                overlay: parse_overlay(overlay),
                sidebar: false,
            }
        }
        [id, "basics", overlay, "sidebar"]
            if parse_id(id).is_some() && parse_overlay(overlay).is_some() =>
        {
            AppRoute::Stories {
                story_id: parse_id(id),
                nav: StoryNav::Basics,
                overlay: parse_overlay(overlay),
                sidebar: true,
            }
        }
        [id, "chapters", chapter_id]
            if parse_id(id).is_some() && parse_id(chapter_id).is_some() =>
        {
            AppRoute::Stories {
                story_id: parse_id(id),
                nav: StoryNav::Chapter(parse_id(chapter_id).unwrap()),
                overlay: None,
                sidebar: false,
            }
        }
        [id, "chapters", chapter_id, "sidebar"]
            if parse_id(id).is_some() && parse_id(chapter_id).is_some() =>
        {
            AppRoute::Stories {
                story_id: parse_id(id),
                nav: StoryNav::Chapter(parse_id(chapter_id).unwrap()),
                overlay: None,
                sidebar: true,
            }
        }
        [id, "chapters", chapter_id, overlay]
            if parse_id(id).is_some()
                && parse_id(chapter_id).is_some()
                && parse_overlay(overlay).is_some() =>
        {
            AppRoute::Stories {
                story_id: parse_id(id),
                nav: StoryNav::Chapter(parse_id(chapter_id).unwrap()),
                overlay: parse_overlay(overlay),
                sidebar: false,
            }
        }
        [id, "chapters", chapter_id, overlay, "sidebar"]
            if parse_id(id).is_some()
                && parse_id(chapter_id).is_some()
                && parse_overlay(overlay).is_some() =>
        {
            AppRoute::Stories {
                story_id: parse_id(id),
                nav: StoryNav::Chapter(parse_id(chapter_id).unwrap()),
                overlay: parse_overlay(overlay),
                sidebar: true,
            }
        }
        [id, "chapters", chapter_id, "beats", beat_id]
            if parse_id(id).is_some()
                && parse_id(chapter_id).is_some()
                && parse_id(beat_id).is_some() =>
        {
            AppRoute::Stories {
                story_id: parse_id(id),
                nav: StoryNav::Beat {
                    chapter_id: parse_id(chapter_id).unwrap(),
                    beat_id: parse_id(beat_id).unwrap(),
                },
                overlay: None,
                sidebar: false,
            }
        }
        [id, "chapters", chapter_id, "beats", beat_id, "sidebar"]
            if parse_id(id).is_some()
                && parse_id(chapter_id).is_some()
                && parse_id(beat_id).is_some() =>
        {
            AppRoute::Stories {
                story_id: parse_id(id),
                nav: StoryNav::Beat {
                    chapter_id: parse_id(chapter_id).unwrap(),
                    beat_id: parse_id(beat_id).unwrap(),
                },
                overlay: None,
                sidebar: true,
            }
        }
        [id, "chapters", chapter_id, "beats", beat_id, overlay]
            if parse_id(id).is_some()
                && parse_id(chapter_id).is_some()
                && parse_id(beat_id).is_some()
                && parse_overlay(overlay).is_some() =>
        {
            AppRoute::Stories {
                story_id: parse_id(id),
                nav: StoryNav::Beat {
                    chapter_id: parse_id(chapter_id).unwrap(),
                    beat_id: parse_id(beat_id).unwrap(),
                },
                overlay: parse_overlay(overlay),
                sidebar: false,
            }
        }
        [id, "chapters", chapter_id, "beats", beat_id, overlay, "sidebar"]
            if parse_id(id).is_some()
                && parse_id(chapter_id).is_some()
                && parse_id(beat_id).is_some()
                && parse_overlay(overlay).is_some() =>
        {
            AppRoute::Stories {
                story_id: parse_id(id),
                nav: StoryNav::Beat {
                    chapter_id: parse_id(chapter_id).unwrap(),
                    beat_id: parse_id(beat_id).unwrap(),
                },
                overlay: parse_overlay(overlay),
                sidebar: true,
            }
        }
        _ => AppRoute::Stories {
            story_id: None,
            nav: StoryNav::None,
            overlay: None,
            sidebar: false,
        },
    }
}

fn parse_queue(segments: &[&str]) -> AppRoute {
    if let Some(route) = route_if_settings(segments) {
        return route;
    }
    let sidebar = segments == ["sidebar"];
    AppRoute::Queue { sidebar }
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
                sidebar: false,
            },
            AppRoute::Chats {
                chat_id: Some(42),
                overlay: None,
                sidebar: false,
            },
            AppRoute::Chats {
                chat_id: Some(42),
                overlay: Some(Overlay::Variables),
                sidebar: false,
            },
            AppRoute::Chats {
                chat_id: None,
                overlay: Some(Overlay::NewChat),
                sidebar: false,
            },
            AppRoute::Chats {
                chat_id: Some(7),
                overlay: None,
                sidebar: true,
            },
            AppRoute::Characters {
                character_id: None,
                chat_id: None,
                sidebar: false,
            },
            AppRoute::Characters {
                character_id: Some(5),
                chat_id: None,
                sidebar: false,
            },
            AppRoute::Characters {
                character_id: None,
                chat_id: Some(42),
                sidebar: false,
            },
            AppRoute::Characters {
                character_id: Some(5),
                chat_id: Some(42),
                sidebar: false,
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
                overlay: None,
                sidebar: false,
            },
            AppRoute::Stories {
                story_id: Some(3),
                nav: StoryNav::Basics,
                overlay: None,
                sidebar: false,
            },
            AppRoute::Stories {
                story_id: Some(3),
                nav: StoryNav::Chapter(9),
                overlay: None,
                sidebar: false,
            },
            AppRoute::Stories {
                story_id: Some(3),
                nav: StoryNav::Beat {
                    chapter_id: 9,
                    beat_id: 12,
                },
                overlay: Some(Overlay::Variables),
                sidebar: true,
            },
            AppRoute::Stories {
                story_id: Some(3),
                nav: StoryNav::Chapter(9),
                overlay: Some(Overlay::Variables),
                sidebar: true,
            },
            AppRoute::Stories {
                story_id: Some(3),
                nav: StoryNav::Basics,
                overlay: None,
                sidebar: true,
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
                sidebar: false,
            },
            AppRoute::Games {
                game_id: None,
                overlay: None,
                sidebar: true,
            },
            AppRoute::Games {
                game_id: None,
                overlay: Some(Overlay::NewGame),
                sidebar: false,
            },
            AppRoute::Games {
                game_id: Some(7),
                overlay: None,
                sidebar: false,
            },
            AppRoute::Games {
                game_id: Some(7),
                overlay: None,
                sidebar: true,
            },
            AppRoute::Games {
                game_id: Some(7),
                overlay: Some(Overlay::State),
                sidebar: false,
            },
            AppRoute::Games {
                game_id: Some(7),
                overlay: Some(Overlay::State),
                sidebar: true,
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
                sidebar: false,
            },
            AppRoute::Scenarios {
                scenario_id: Some(4),
                game_id: None,
                sidebar: false,
            },
            AppRoute::Scenarios {
                scenario_id: None,
                game_id: Some(9),
                sidebar: true,
            },
            AppRoute::Scenarios {
                scenario_id: Some(4),
                game_id: Some(9),
                sidebar: true,
            },
        ];
        for route in routes {
            let path = route.to_path();
            assert_eq!(parse_path(&path), route, "path: {path}");
        }
    }

    #[test]
    fn round_trip_queue() {
        let routes = [
            AppRoute::Queue { sidebar: false },
            AppRoute::Queue { sidebar: true },
        ];
        for route in routes {
            let path = route.to_path();
            assert_eq!(parse_path(&path), route, "path: {path}");
        }
    }

    #[test]
    fn round_trip_settings() {
        let route = AppRoute::Settings { sidebar: false };
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
                    sidebar: false,
                },
            ),
            (
                "/chats/42/character",
                AppRoute::Characters {
                    character_id: None,
                    chat_id: Some(42),
                    sidebar: false,
                },
            ),
            (
                "/stories/character",
                AppRoute::Characters {
                    character_id: None,
                    chat_id: None,
                    sidebar: false,
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
            assert_eq!(
                parse_path(path),
                AppRoute::Settings { sidebar: false },
                "path: {path}"
            );
        }
    }
}
