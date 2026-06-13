use wasm_bindgen::closure::Closure;
use wasm_bindgen::JsCast;
use yew::prelude::*;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Overlay {
    Character,
    Variables,
    Settings,
    NewChat,
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
    Queue {
        overlay: Option<Overlay>,
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
            Self::Queue { .. } => crate::queue_ui::AppMode::Queue,
        }
    }

    pub fn overlay(&self) -> Option<Overlay> {
        match self {
            Self::Chats { overlay, .. }
            | Self::Stories { overlay, .. }
            | Self::Queue { overlay } => *overlay,
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
            Self::Queue { .. } => Self::Queue { overlay: None },
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
            Self::Queue { .. } => Self::Queue {
                overlay: Some(overlay),
            },
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
            other => other,
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
            Self::Queue {
                overlay: Some(overlay),
            } => format!("/queue/{}", overlay_segment(*overlay)),
            Self::Queue { overlay: None } => "/queue".to_string(),
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
        Overlay::Character => "character",
        Overlay::Variables => "variables",
        Overlay::Settings => "settings",
        Overlay::NewChat => "new",
    }
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
        Some("queue") => parse_queue(&segments[1..]),
        Some("stories") => parse_stories(&segments[1..]),
        Some("chats") | None => parse_chats(segments.get(1..).unwrap_or(&[])),
        _ => AppRoute::default(),
    }
}

fn parse_id(value: &str) -> Option<i64> {
    value.parse().ok()
}

fn parse_overlay(value: &str) -> Option<Overlay> {
    match value {
        "character" => Some(Overlay::Character),
        "variables" => Some(Overlay::Variables),
        "settings" => Some(Overlay::Settings),
        "new" => Some(Overlay::NewChat),
        _ => None,
    }
}

fn parse_chats(segments: &[&str]) -> AppRoute {
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

fn parse_stories(segments: &[&str]) -> AppRoute {
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
    match segments {
        [] => AppRoute::Queue { overlay: None },
        [overlay] if parse_overlay(overlay).is_some() => AppRoute::Queue {
            overlay: parse_overlay(overlay),
        },
        _ => AppRoute::Queue { overlay: None },
    }
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
                overlay: Some(Overlay::Character),
                sidebar: false,
            },
            AppRoute::Chats {
                chat_id: Some(42),
                overlay: Some(Overlay::Variables),
                sidebar: false,
            },
            AppRoute::Chats {
                chat_id: Some(42),
                overlay: Some(Overlay::Settings),
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
            AppRoute::Chats {
                chat_id: Some(42),
                overlay: Some(Overlay::Character),
                sidebar: true,
            },
            AppRoute::Chats {
                chat_id: None,
                overlay: Some(Overlay::Settings),
                sidebar: true,
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
                overlay: Some(Overlay::Settings),
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
    fn round_trip_queue() {
        let routes = [
            AppRoute::Queue { overlay: None },
            AppRoute::Queue {
                overlay: Some(Overlay::Settings),
            },
        ];
        for route in routes {
            let path = route.to_path();
            assert_eq!(parse_path(&path), route, "path: {path}");
        }
    }
}
