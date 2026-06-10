use std::rc::Rc;

use dreamwell_types::*;
use wasm_bindgen::closure::Closure;
use wasm_bindgen::JsCast;
use web_sys::HtmlInputElement;
use yew::prelude::*;

use crate::api;
use crate::queue_ui::QueueBar;
use crate::router::{AppRoute, StoryNav};

#[derive(Clone, Copy, PartialEq)]
pub enum StorySelection {
    Basics,
    Chapter(i64),
    Beat { chapter_id: i64, beat_id: i64 },
}

fn story_nav_from_selection(selection: StorySelection) -> StoryNav {
    match selection {
        StorySelection::Basics => StoryNav::Basics,
        StorySelection::Chapter(id) => StoryNav::Chapter(id),
        StorySelection::Beat {
            chapter_id,
            beat_id,
        } => StoryNav::Beat {
            chapter_id,
            beat_id,
        },
    }
}

fn selection_from_story_nav(nav: StoryNav) -> StorySelection {
    match nav {
        StoryNav::Basics => StorySelection::Basics,
        StoryNav::Chapter(id) => StorySelection::Chapter(id),
        StoryNav::Beat {
            chapter_id,
            beat_id,
        } => StorySelection::Beat {
            chapter_id,
            beat_id,
        },
    }
}

fn story_id_from_route(route: &AppRoute) -> Option<i64> {
    match route {
        AppRoute::Stories { story_id, .. } => *story_id,
        _ => None,
    }
}

fn story_nav_from_route(route: &AppRoute) -> StoryNav {
    match route {
        AppRoute::Stories { nav, .. } => *nav,
        _ => StoryNav::Basics,
    }
}

fn sidebar_open_from_route(route: &AppRoute) -> bool {
    matches!(route, AppRoute::Stories { sidebar: true, .. })
}

#[derive(Properties, PartialEq)]
pub struct StoriesShellProps {
    pub route: AppRoute,
    pub on_navigate: Callback<(AppRoute, bool)>,
    pub queue: Option<QueueStatus>,
    pub on_open_queue: Callback<()>,
}

#[function_component(StoriesShell)]
pub fn stories_shell(props: &StoriesShellProps) -> Html {
    let stories = use_state(Vec::<Story>::new);
    let detail = use_state(|| None::<StoryDetail>);
    let guidance = use_state(String::new);
    let loading = use_state(|| true);
    let refresh_generation = use_state(|| 0u32);
    let selected_story_id = story_id_from_route(&props.route);
    let selection = selection_from_story_nav(story_nav_from_route(&props.route));
    let sidebar_open = sidebar_open_from_route(&props.route);

    {
        let stories = stories.clone();
        let loading = loading.clone();
        let on_navigate = props.on_navigate.clone();
        let route = props.route.clone();
        use_effect_with((), move |_| {
            wasm_bindgen_futures::spawn_local(async move {
                if let Ok(list) = api::list_stories().await {
                    stories.set(list);
                    loading.set(false);
                    if let AppRoute::Stories { story_id: None, .. } = route {
                        if let Some(first) = (*stories).first() {
                            on_navigate.emit((
                                AppRoute::Stories {
                                    story_id: Some(first.id),
                                    nav: StoryNav::Basics,
                                    overlay: None,
                                    sidebar: false,
                                },
                                false,
                            ));
                        }
                    }
                } else {
                    loading.set(false);
                }
            });
            || ()
        });
    }

    {
        let detail = detail.clone();
        let stories = stories.clone();
        let refresh_generation = *refresh_generation;
        let route = props.route.clone();
        use_effect_with((route.clone(), refresh_generation), move |(route, _)| {
            let story_id = story_id_from_route(route);
            let mut stream_holder = None::<api::StoryStream>;
            if let Some(story_id) = story_id {
                let detail_for_fetch = detail.clone();
                let stories = stories.clone();
                wasm_bindgen_futures::spawn_local(async move {
                    if let Ok(d) = api::get_story(story_id).await {
                        detail_for_fetch.set(Some(d));
                    }
                });
                stream_holder = Some(api::StoryStream::new(story_id, move |payload| {
                    detail.set(Some(payload.detail.clone()));
                    let current = (*stories).clone();
                    stories.set(
                        current
                            .into_iter()
                            .map(|s| {
                                if s.id == payload.detail.story.id {
                                    payload.detail.story.clone()
                                } else {
                                    s
                                }
                            })
                            .collect(),
                    );
                }));
            } else {
                detail.set(None);
            }
            move || drop(stream_holder)
        });
    }

    {
        let stories = stories.clone();
        let on_navigate = props.on_navigate.clone();
        let route = props.route.clone();
        use_effect_with(
            (route.clone(), (*stories).clone()),
            move |(route, stories)| {
                if let AppRoute::Stories {
                    story_id: Some(id), ..
                } = route
                {
                    if !stories.iter().any(|s| s.id == *id) {
                        on_navigate.emit((
                            AppRoute::Stories {
                                story_id: stories.first().map(|s| s.id),
                                nav: StoryNav::Basics,
                                overlay: None,
                                sidebar: false,
                            },
                            false,
                        ));
                    }
                }
                || ()
            },
        );
    }

    {
        let refresh_generation = refresh_generation.clone();
        let stories = stories.clone();
        use_effect_with((), move |_| {
            let refresh_generation = refresh_generation.clone();
            let stories = stories.clone();
            let resume: Rc<dyn Fn()> = Rc::new(move || {
                refresh_generation.set(*refresh_generation + 1);
                let stories = stories.clone();
                wasm_bindgen_futures::spawn_local(async move {
                    if let Ok(list) = api::list_stories().await {
                        stories.set(list);
                    }
                });
            });

            let resume_visibility = resume.clone();
            let visibility_callback = Closure::wrap(Box::new(move |_event: web_sys::Event| {
                if web_sys::window()
                    .and_then(|window| window.document())
                    .is_some_and(|document| {
                        document.visibility_state() == web_sys::VisibilityState::Visible
                    })
                {
                    resume_visibility();
                }
            }) as Box<dyn FnMut(_)>);

            let document = web_sys::window().and_then(|window| window.document());
            if let Some(document) = document.as_ref() {
                let _ = document.add_event_listener_with_callback(
                    "visibilitychange",
                    visibility_callback.as_ref().unchecked_ref(),
                );
            }

            let resume_online = resume.clone();
            let online_callback = Closure::wrap(Box::new(move |_event: web_sys::Event| {
                resume_online();
            }) as Box<dyn FnMut(_)>);

            let resume_focus = resume.clone();
            let focus_callback = Closure::wrap(Box::new(move |_event: web_sys::Event| {
                resume_focus();
            }) as Box<dyn FnMut(_)>);

            let window = web_sys::window();
            if let Some(window) = window.as_ref() {
                let _ = window.add_event_listener_with_callback(
                    "online",
                    online_callback.as_ref().unchecked_ref(),
                );
                let _ = window.add_event_listener_with_callback(
                    "focus",
                    focus_callback.as_ref().unchecked_ref(),
                );
            }

            move || {
                if let Some(document) = document.as_ref() {
                    let _ = document.remove_event_listener_with_callback(
                        "visibilitychange",
                        visibility_callback.as_ref().unchecked_ref(),
                    );
                }
                if let Some(window) = window.as_ref() {
                    let _ = window.remove_event_listener_with_callback(
                        "online",
                        online_callback.as_ref().unchecked_ref(),
                    );
                    let _ = window.remove_event_listener_with_callback(
                        "focus",
                        focus_callback.as_ref().unchecked_ref(),
                    );
                }
            }
        });
    }

    {
        let stories = stories.clone();
        use_effect_with((), move |_| {
            let stories = stories.clone();
            let handle = gloo_timers::callback::Interval::new(3000, move || {
                let stories = stories.clone();
                wasm_bindgen_futures::spawn_local(async move {
                    if let Ok(list) = api::list_stories().await {
                        stories.set(list);
                    }
                });
            });
            move || drop(handle)
        });
    }

    if *loading {
        return html! { <div class="loading-screen muted">{"Loading stories…"}</div> };
    }

    let active_story_id = selected_story_id.or_else(|| (*detail).as_ref().map(|d| d.story.id));

    let navigate_story = {
        let on_navigate = props.on_navigate.clone();
        let route = props.route.clone();
        Callback::from(move |(story_id, nav): (Option<i64>, StoryNav)| {
            let overlay = route.overlay();
            on_navigate.emit((
                AppRoute::Stories {
                    story_id,
                    nav,
                    overlay,
                    sidebar: false,
                },
                true,
            ));
        })
    };

    html! {
        <>
            if sidebar_open {
                <div class="drawer-backdrop" onclick={Callback::from({
                    let on_navigate = props.on_navigate.clone();
                    let route = props.route.clone();
                    move |_| on_navigate.emit((route.clone().without_sidebar(), true))
                })} />
            }
            <div class={classes!(
                "app-shell",
                sidebar_open.then_some("pane-sidebar"),
            )}>
            <StorySidebar
                stories={(*stories).clone()}
                selected_id={selected_story_id}
                on_select_story={Callback::from({
                    let navigate_story = navigate_story.clone();
                    move |id| navigate_story.emit((Some(id), StoryNav::Basics))
                })}
                on_new={Callback::from({
                    let stories = stories.clone();
                    let on_navigate = props.on_navigate.clone();
                    move |_| {
                        let stories = stories.clone();
                        let on_navigate = on_navigate.clone();
                        wasm_bindgen_futures::spawn_local(async move {
                            let payload = StoryCreate {
                                title: format!("Story {}", stories.len() + 1),
                                ..Default::default()
                            };
                            if let Ok(d) = api::create_story(&payload).await {
                                if let Ok(list) = api::list_stories().await {
                                    stories.set(list);
                                }
                                on_navigate.emit((
                                    AppRoute::Stories {
                                        story_id: Some(d.story.id),
                                        nav: StoryNav::Basics,
                                        overlay: None,
                                        sidebar: false,
                                    },
                                    true,
                                ));
                            }
                        });
                    }
                })}
                on_delete={Callback::from({
                    let stories = stories.clone();
                    let on_navigate = props.on_navigate.clone();
                    let route = props.route.clone();
                    move |id| {
                        let stories = stories.clone();
                        let on_navigate = on_navigate.clone();
                        let route = route.clone();
                        wasm_bindgen_futures::spawn_local(async move {
                            let _ = api::delete_story(id).await;
                            if let Ok(list) = api::list_stories().await {
                                if story_id_from_route(&route) == Some(id) {
                                    on_navigate.emit((
                                        AppRoute::Stories {
                                            story_id: list.first().map(|s| s.id),
                                            nav: StoryNav::Basics,
                                            overlay: None,
                                            sidebar: false,
                                        },
                                        false,
                                    ));
                                }
                                stories.set(list);
                            }
                        });
                    }
                })}
            />
            <main class="main">
                <QueueBar queue={props.queue.clone()} on_open={props.on_open_queue.clone()} />
                <StoryEditor
                    detail={(*detail).clone()}
                    selection={selection}
                    guidance={(*guidance).clone()}
                    on_guidance={Callback::from({
                        let guidance = guidance.clone();
                        move |v| guidance.set(v)
                    })}
                    on_detail={Callback::from({
                        let detail = detail.clone();
                        let stories = stories.clone();
                        move |d: StoryDetail| {
                            stories.set(
                                (*stories)
                                    .clone()
                                    .into_iter()
                                    .map(|s| {
                                        if s.id == d.story.id {
                                            d.story.clone()
                                        } else {
                                            s
                                        }
                                    })
                                    .collect(),
                            );
                            detail.set(Some(d));
                        }
                    })}
                    on_selection={Callback::from({
                        let navigate_story = navigate_story.clone();
                        let story_id = active_story_id;
                        move |s| {
                            if let Some(story_id) = story_id {
                                navigate_story.emit((
                                    Some(story_id),
                                    story_nav_from_selection(s),
                                ));
                            }
                        }
                    })}
                    on_open_sidebar={Callback::from({
                        let on_navigate = props.on_navigate.clone();
                        let route = props.route.clone();
                        move |_| on_navigate.emit((route.clone().with_sidebar(true), true))
                    })}
                />
            </main>
        </div>
        </>
    }
}

#[derive(Properties, PartialEq)]
struct StorySidebarProps {
    stories: Vec<Story>,
    selected_id: Option<i64>,
    on_select_story: Callback<i64>,
    on_new: Callback<()>,
    on_delete: Callback<i64>,
}

#[function_component(StorySidebar)]
fn story_sidebar(props: &StorySidebarProps) -> Html {
    html! {
        <aside class="sidebar">
            <div class="header sidebar-header">
                <div>
                    <div class="muted" style="text-transform:uppercase;letter-spacing:0.2em;font-size:0.7rem;">{"Dreamwell"}</div>
                    <strong>{"Stories"}</strong>
                </div>
                <button class="btn" onclick={props.on_new.reform(|_| ())}>{"New"}</button>
            </div>
            <div class="sidebar-scroll">
                { for props.stories.iter().map(|story| {
                    let id = story.id;
                    let selected = props.selected_id == Some(story.id);
                    let status = story_status(story);
                    html! {
                        <div class={classes!("chat-item", selected.then_some("selected"))}>
                            <div style="display:flex;gap:0.5rem;">
                                <div style="flex:1;min-width:0;" onclick={props.on_select_story.reform(move |_| id)}>
                                    <div style="overflow:hidden;text-overflow:ellipsis;white-space:nowrap;">{ &story.title }</div>
                                    if let Some(label) = status {
                                        <span class="badge">{ label }</span>
                                    }
                                </div>
                                <button class="btn secondary btn-compact" onclick={props.on_delete.reform(move |_| id)}>{"✕"}</button>
                            </div>
                        </div>
                    }
                }) }
            </div>
        </aside>
    }
}

fn story_status(story: &Story) -> Option<String> {
    let job = story.active_job.as_ref()?;
    match job.status {
        JobStatus::Running => Some("generating…".to_string()),
        JobStatus::Queued => {
            if story.queued_jobs > 1 {
                Some(format!("queued ({})", story.queued_jobs))
            } else {
                Some("queued".to_string())
            }
        }
        _ => None,
    }
}

#[derive(Properties, PartialEq)]
struct StoryEditorProps {
    detail: Option<StoryDetail>,
    selection: StorySelection,
    guidance: String,
    on_guidance: Callback<String>,
    on_detail: Callback<StoryDetail>,
    on_selection: Callback<StorySelection>,
    on_open_sidebar: Callback<()>,
}

#[function_component(StoryEditor)]
fn story_editor(props: &StoryEditorProps) -> Html {
    let Some(detail) = props.detail.clone() else {
        return html! {
            <>
                <header class="header">
                    <div class="mobile-toolbar">
                        <button class="btn secondary" onclick={props.on_open_sidebar.reform(|_| ())}>{"Stories"}</button>
                    </div>
                    <h1 class="header-title">{"Select or create a story"}</h1>
                </header>
                <div class="loading-screen muted" style="text-align:center;">{"Stories are built chapter by chapter, beat by beat."}</div>
            </>
        };
    };

    let queued = detail.story.queued_jobs;

    html! {
        <>
            <header class="header">
                <div class="mobile-toolbar">
                    <button class="btn secondary" onclick={props.on_open_sidebar.reform(|_| ())}>{"Stories"}</button>
                </div>
                <h1 class="header-title">{ detail.story.title.clone() }</h1>
                <p class="header-subtitle muted">
                    { format!(
                        "{} · {} of {} chapters",
                        detail.story.length_preset.label(),
                        detail.chapters.len(),
                        detail.story.length_preset.ref_chapters(),
                    ) }
                    if queued > 0 {
                        { format!(" · {} queued", queued) }
                    }
                </p>
            </header>
            <div class="story-editor">
                <StoryBlockList
                    detail={detail}
                    selection={props.selection}
                    guidance={props.guidance.clone()}
                    on_guidance={props.on_guidance.clone()}
                    on_detail={props.on_detail.clone()}
                    on_selection={props.on_selection.clone()}
                />
            </div>
        </>
    }
}

#[derive(Properties, PartialEq)]
struct StoryBlockListProps {
    detail: StoryDetail,
    selection: StorySelection,
    guidance: String,
    on_guidance: Callback<String>,
    on_detail: Callback<StoryDetail>,
    on_selection: Callback<StorySelection>,
}

#[function_component(StoryBlockList)]
fn story_block_list(props: &StoryBlockListProps) -> Html {
    let story_id = props.detail.story.id;
    let target = props.detail.story.length_preset.ref_chapters();
    let chapter_count = props.detail.chapters.len() as i64;
    let proposing_chapters = props.detail.story.active_job.as_ref().is_some_and(|job| {
        job.job_type == JobType::StoryProposeChapters
            && matches!(job.status, JobStatus::Queued | JobStatus::Running)
    });

    html! {
        <div class="story-blocks">
            <StoryBlockHeader
                label={"Story basics".to_string()}
                subtitle={props.detail.story.title.clone()}
                open={props.selection == StorySelection::Basics}
                on_toggle={props.on_selection.reform(|_| StorySelection::Basics)}
            />
            if props.selection == StorySelection::Basics {
                <div class="story-block-body">
                    <StoryBasicsForm
                        story={props.detail.story.clone()}
                        on_save={Callback::from({
                            let on_detail = props.on_detail.clone();
                            move |updated: StoryBasics| {
                                let on_detail = on_detail.clone();
                                wasm_bindgen_futures::spawn_local(async move {
                                    if let Ok(d) = api::update_story(updated.id, &StoryUpdate {
                                        title: Some(updated.title),
                                        premise: Some(updated.premise),
                                        tone: Some(updated.tone),
                                        genre: Some(updated.genre),
                                        pov: Some(updated.pov),
                                        length_preset: Some(updated.length_preset),
                                        notes: Some(updated.notes),
                                    }).await {
                                        on_detail.emit(d);
                                    }
                                });
                            }
                        })}
                    />
                    <label class="field" style="margin-top:1rem;">
                        <span class="muted">{"Guidance for proposal"}</span>
                        <textarea
                            placeholder="Optional notes for the AI — e.g. keep chapter 2 as-is, add a flashback chapter…"
                            value={props.guidance.clone()}
                            rows="3"
                            oninput={guidance_input(props.on_guidance.clone())}
                        />
                    </label>
                    <div class="story-actions">
                        <button class="btn" disabled={proposing_chapters} onclick={propose_chapters_action(story_id, props.guidance.clone(), props.on_detail.clone())}>
                            { if proposing_chapters {
                                "Proposing chapters…".to_string()
                            } else if chapter_count == 0 {
                                format!("Propose chapters (~{target})")
                            } else {
                                "Propose chapters".to_string()
                            }}
                        </button>
                        <button class="btn secondary" onclick={add_chapter_action(story_id, props.on_detail.clone(), props.on_selection.clone())}>
                            {"Add chapter manually"}
                        </button>
                    </div>
                    <p class="muted" style="font-size:0.85rem;margin-top:0.75rem;">
                        {"Propose chapters reviews your story and returns a full chapter list — it may add, remove, reorder, or rewrite chapters. Existing beat prose is noted in the prompt but may be replaced."}
                    </p>
                </div>
            }

            { for props.detail.chapters.iter().map(|ch| {
                let ch_id = ch.id;
                let ch_open = props.selection == StorySelection::Chapter(ch_id);
                let ch_label = format!("Chapter {}", ch.sort_order + 1);
                let ch_subtitle = if ch.title.is_empty() { "…".to_string() } else { ch.title.clone() };
                let generating = ch.title.is_empty() && ch.synopsis.is_empty()
                    && props.detail.story.active_job.is_some();
                html! {
                    <>
                        <StoryBlockHeader
                            label={ch_label}
                            subtitle={ch_subtitle}
                            open={ch_open}
                            badge={generating.then_some("generating…".to_string())}
                            on_toggle={props.on_selection.reform(move |_| StorySelection::Chapter(ch_id))}
                        />
                        if ch_open {
                            <div class="story-block-body">
                                <ChapterEditor
                                    story_id={story_id}
                                    chapter={Some(ch.clone())}
                                    proposing_beats={props.detail.story.active_job.as_ref().is_some_and(|job| {
                                        job.job_type == JobType::StoryProposeBeats
                                            && job.chapter_id == Some(ch_id)
                                            && matches!(job.status, JobStatus::Queued | JobStatus::Running)
                                    })}
                                    guidance={props.guidance.clone()}
                                    on_guidance={props.on_guidance.clone()}
                                    on_detail={props.on_detail.clone()}
                                />
                            </div>
                        }
                        { for ch.beats.iter().map(|beat| {
                            let beat_id = beat.id;
                            let beat_open = props.selection == StorySelection::Beat { chapter_id: ch_id, beat_id };
                            let beat_label = format!("Beat {}", beat.sort_order + 1);
                            let beat_subtitle = if beat.title.is_empty() { "…".to_string() } else { beat.title.clone() };
                            let streaming = matches!(beat.job_status, Some(JobStatus::Running) | Some(JobStatus::Queued));
                            html! {
                                <>
                                    <StoryBlockHeader
                                        label={beat_label}
                                        subtitle={beat_subtitle}
                                        open={beat_open}
                                        indent={true}
                                        badge={streaming.then_some("…".to_string())}
                                        on_toggle={props.on_selection.reform(move |_| StorySelection::Beat { chapter_id: ch_id, beat_id })}
                                    />
                                    if beat_open {
                                        <div class="story-block-body story-block-body-nested">
                                            <BeatEditor
                                                story_id={story_id}
                                                chapter_id={ch_id}
                                                beat={Some(beat.clone())}
                                                guidance={props.guidance.clone()}
                                                on_guidance={props.on_guidance.clone()}
                                                on_detail={props.on_detail.clone()}
                                            />
                                        </div>
                                    }
                                </>
                            }
                        }) }
                    </>
                }
            }) }
        </div>
    }
}

fn propose_chapters_action(
    story_id: i64,
    guidance: String,
    on_detail: Callback<StoryDetail>,
) -> Callback<MouseEvent> {
    Callback::from(move |_| {
        let on_detail = on_detail.clone();
        let notes = guidance.clone();
        wasm_bindgen_futures::spawn_local(async move {
            if let Ok(d) = api::propose_chapters(story_id, &notes).await {
                on_detail.emit(d);
            }
        });
    })
}

fn add_chapter_action(
    story_id: i64,
    on_detail: Callback<StoryDetail>,
    on_selection: Callback<StorySelection>,
) -> Callback<MouseEvent> {
    Callback::from(move |_| {
        let on_detail = on_detail.clone();
        let on_selection = on_selection.clone();
        wasm_bindgen_futures::spawn_local(async move {
            if let Ok(d) = api::create_chapter(story_id, &StoryChapterCreate::default()).await {
                if let Some(ch) = d.chapters.last() {
                    on_selection.emit(StorySelection::Chapter(ch.id));
                }
                on_detail.emit(d);
            }
        });
    })
}

fn guidance_input(on_guidance: Callback<String>) -> Callback<InputEvent> {
    Callback::from(move |e: InputEvent| {
        let input: HtmlInputElement = e.target_unchecked_into();
        on_guidance.emit(input.value());
    })
}

#[derive(Properties, PartialEq)]
struct StoryBlockHeaderProps {
    label: String,
    subtitle: String,
    open: bool,
    #[prop_or_default]
    indent: bool,
    #[prop_or_default]
    badge: Option<String>,
    on_toggle: Callback<()>,
}

#[function_component(StoryBlockHeader)]
fn story_block_header(props: &StoryBlockHeaderProps) -> Html {
    html! {
        <button
            type="button"
            class={classes!(
                "story-block-header",
                props.open.then_some("open"),
                props.indent.then_some("indented"),
            )}
            onclick={props.on_toggle.reform(|_| ())}
        >
            <span class="story-block-chevron">{ if props.open { "▾" } else { "▸" } }</span>
            <span class="story-block-label">{ &props.label }</span>
            <span class="story-block-subtitle muted">{ &props.subtitle }</span>
            if let Some(badge) = &props.badge {
                <span class="badge">{ badge }</span>
            }
        </button>
    }
}

#[derive(Clone, PartialEq)]
struct StoryBasics {
    id: i64,
    title: String,
    premise: String,
    tone: String,
    genre: String,
    pov: String,
    length_preset: LengthPreset,
    notes: String,
}

impl From<Story> for StoryBasics {
    fn from(s: Story) -> Self {
        Self {
            id: s.id,
            title: s.title,
            premise: s.premise,
            tone: s.tone,
            genre: s.genre,
            pov: s.pov,
            length_preset: s.length_preset,
            notes: s.notes,
        }
    }
}

#[derive(Properties, PartialEq)]
struct StoryBasicsFormProps {
    story: Story,
    on_save: Callback<StoryBasics>,
}

#[function_component(StoryBasicsForm)]
fn story_basics_form(props: &StoryBasicsFormProps) -> Html {
    let draft = use_state(|| StoryBasics::from(props.story.clone()));

    {
        let draft = draft.clone();
        let story = props.story.clone();
        use_effect_with(story.id, move |_| {
            draft.set(StoryBasics::from(story));
            || ()
        });
    }

    html! {
        <div class="story-form">
            <label class="field"><span class="muted">{"Title"}</span>
                <input type="text" value={draft.title.clone()} oninput={basics_input(draft.clone(), "title")} />
            </label>
            <label class="field"><span class="muted">{"Premise"}</span>
                <textarea value={draft.premise.clone()} rows="3" oninput={basics_input(draft.clone(), "premise")} />
            </label>
            <div style="display:grid;grid-template-columns:1fr 1fr;gap:0.75rem;">
                <label class="field"><span class="muted">{"Tone"}</span>
                    <input type="text" value={draft.tone.clone()} oninput={basics_input(draft.clone(), "tone")} />
                </label>
                <label class="field"><span class="muted">{"Genre"}</span>
                    <input type="text" value={draft.genre.clone()} oninput={basics_input(draft.clone(), "genre")} />
                </label>
            </div>
            <label class="field"><span class="muted">{"POV"}</span>
                <input type="text" value={draft.pov.clone()} placeholder="e.g. third person limited" oninput={basics_input(draft.clone(), "pov")} />
            </label>
            <label class="field"><span class="muted">{"Length"}</span>
                <select onchange={preset_select(draft.clone())}>
                    { for [LengthPreset::Flash, LengthPreset::Short, LengthPreset::Novella, LengthPreset::Novel].iter().map(|p| {
                        let selected = draft.length_preset == *p;
                        html! { <option value={preset_value(*p)} selected={selected}>{ p.label() }</option> }
                    }) }
                </select>
            </label>
            <label class="field"><span class="muted">{"Notes"}</span>
                <textarea value={draft.notes.clone()} rows="2" oninput={basics_input(draft.clone(), "notes")} />
            </label>
            <button class="btn" onclick={{
                let draft = draft.clone();
                let on_save = props.on_save.clone();
                Callback::from(move |_| on_save.emit((*draft).clone()))
            }}>{"Save basics"}</button>
        </div>
    }
}

fn preset_value(p: LengthPreset) -> &'static str {
    match p {
        LengthPreset::Flash => "flash",
        LengthPreset::Short => "short",
        LengthPreset::Novella => "novella",
        LengthPreset::Novel => "novel",
    }
}

fn preset_select(draft: UseStateHandle<StoryBasics>) -> Callback<Event> {
    Callback::from(move |e: Event| {
        let input: HtmlInputElement = e.target_unchecked_into();
        let preset = match input.value().as_str() {
            "flash" => LengthPreset::Flash,
            "novella" => LengthPreset::Novella,
            "novel" => LengthPreset::Novel,
            _ => LengthPreset::Short,
        };
        let mut next = (*draft).clone();
        next.length_preset = preset;
        draft.set(next);
    })
}

fn basics_input(draft: UseStateHandle<StoryBasics>, field: &'static str) -> Callback<InputEvent> {
    Callback::from(move |e: InputEvent| {
        let input: HtmlInputElement = e.target_unchecked_into();
        let mut next = (*draft).clone();
        match field {
            "title" => next.title = input.value(),
            "premise" => next.premise = input.value(),
            "tone" => next.tone = input.value(),
            "genre" => next.genre = input.value(),
            "pov" => next.pov = input.value(),
            "notes" => next.notes = input.value(),
            _ => {}
        }
        draft.set(next);
    })
}

#[derive(Properties, PartialEq)]
struct ChapterEditorProps {
    story_id: i64,
    chapter: Option<StoryChapter>,
    #[prop_or(false)]
    proposing_beats: bool,
    guidance: String,
    on_guidance: Callback<String>,
    on_detail: Callback<StoryDetail>,
}

#[function_component(ChapterEditor)]
fn chapter_editor(props: &ChapterEditorProps) -> Html {
    let Some(chapter) = props.chapter.clone() else {
        return html! { <p class="muted">{"Chapter not found."}</p> };
    };
    let title = use_state(|| chapter.title.clone());
    let synopsis = use_state(|| chapter.synopsis.clone());

    {
        let title = title.clone();
        let synopsis = synopsis.clone();
        let chapter = chapter.clone();
        use_effect_with(chapter.id, move |_| {
            title.set(chapter.title.clone());
            synopsis.set(chapter.synopsis.clone());
            || ()
        });
    }

    let story_id = props.story_id;
    let chapter_id = chapter.id;
    let proposing_beats = props.proposing_beats;

    html! {
        <div class="story-form">
            <label class="field"><span class="muted">{"Title"}</span>
                <input type="text" value={(*title).clone()} oninput={string_input(title.clone())} />
            </label>
            <label class="field"><span class="muted">{"Synopsis"}</span>
                <textarea value={(*synopsis).clone()} rows="5" oninput={string_input(synopsis.clone())} />
            </label>
            <label class="field">
                <span class="muted">{"Guidance for proposal"}</span>
                <textarea
                    placeholder="Optional notes — e.g. split the confrontation into two beats…"
                    value={props.guidance.clone()}
                    rows="3"
                    oninput={guidance_input(props.on_guidance.clone())}
                />
            </label>
            <div class="story-actions">
                <button class="btn secondary" onclick={{
                    let on_detail = props.on_detail.clone();
                    let title = title.clone();
                    let synopsis = synopsis.clone();
                    Callback::from(move |_| {
                        let on_detail = on_detail.clone();
                        let title = (*title).clone();
                        let synopsis = (*synopsis).clone();
                        wasm_bindgen_futures::spawn_local(async move {
                            if let Ok(d) = api::update_chapter(story_id, chapter_id, &StoryChapterUpdate {
                                title: Some(title),
                                synopsis: Some(synopsis),
                                sort_order: None,
                            }).await {
                                on_detail.emit(d);
                            }
                        });
                    })
                }}>{"Save chapter"}</button>
                <button class="btn" disabled={proposing_beats} onclick={{
                    let on_detail = props.on_detail.clone();
                    let guidance = props.guidance.clone();
                    Callback::from(move |_| {
                        let on_detail = on_detail.clone();
                        let notes = guidance.clone();
                        wasm_bindgen_futures::spawn_local(async move {
                            if let Ok(d) = api::propose_beats(story_id, chapter_id, &notes).await {
                                on_detail.emit(d);
                            }
                        });
                    })
                }}>{ if proposing_beats { "Proposing beats…" } else { "Propose beats" } }</button>
                <button class="btn secondary" onclick={{
                    let on_detail = props.on_detail.clone();
                    Callback::from(move |_| {
                        let on_detail = on_detail.clone();
                        wasm_bindgen_futures::spawn_local(async move {
                            if let Ok(d) = api::create_beat(story_id, chapter_id, &StoryBeatCreate::default()).await {
                                on_detail.emit(d);
                            }
                        });
                    })
                }}>{"Add beat manually"}</button>
                <button class="btn secondary text-danger btn-compact" onclick={{
                    let on_detail = props.on_detail.clone();
                    Callback::from(move |_| {
                        let on_detail = on_detail.clone();
                        wasm_bindgen_futures::spawn_local(async move {
                            let _ = api::delete_chapter(story_id, chapter_id).await;
                            if let Ok(d) = api::get_story(story_id).await {
                                on_detail.emit(d);
                            }
                        });
                    })
                }}>{"Delete chapter"}</button>
            </div>
            <p class="muted" style="font-size:0.85rem;margin-top:0.75rem;">
                {"Propose beats reviews this chapter and returns a full beat list — it may add, remove, reorder, or rewrite beats. Existing prose is noted but may be replaced."}
            </p>
        </div>
    }
}

#[derive(Properties, PartialEq)]
struct BeatEditorProps {
    story_id: i64,
    chapter_id: i64,
    beat: Option<StoryBeat>,
    guidance: String,
    on_guidance: Callback<String>,
    on_detail: Callback<StoryDetail>,
}

#[function_component(BeatEditor)]
fn beat_editor(props: &BeatEditorProps) -> Html {
    let Some(beat) = props.beat.clone() else {
        return html! { <p class="muted">{"Beat not found."}</p> };
    };
    let title = use_state(|| beat.title.clone());
    let synopsis = use_state(|| beat.synopsis.clone());
    let content = use_state(|| beat.content.clone());

    {
        let title = title.clone();
        let synopsis = synopsis.clone();
        let content = content.clone();
        let beat = beat.clone();
        use_effect_with(beat.id, move |_| {
            title.set(beat.title.clone());
            synopsis.set(beat.synopsis.clone());
            content.set(beat.content.clone());
            || ()
        });
    }

    let streaming = matches!(
        beat.job_status,
        Some(JobStatus::Running) | Some(JobStatus::Queued)
    );
    let story_id = props.story_id;
    let chapter_id = props.chapter_id;
    let beat_id = beat.id;

    html! {
        <div class="story-form">
            <label class="field"><span class="muted">{"Title"}</span>
                <input type="text" value={(*title).clone()} oninput={string_input(title.clone())} />
            </label>
            <label class="field"><span class="muted">{"Synopsis"}</span>
                <textarea value={(*synopsis).clone()} rows="3" oninput={string_input(synopsis.clone())} />
            </label>
            <label class="field"><span class="muted">{"Prose"}{ if streaming { " · streaming on server" } else { "" } }</span>
                <textarea
                    class="prose-editor"
                    value={ if (*content).is_empty() && streaming { "…".to_string() } else { (*content).clone() } }
                    rows="12"
                    oninput={string_input(content.clone())}
                />
            </label>
            <label class="field">
                <span class="muted">{"Guidance for generation"}</span>
                <textarea
                    placeholder="Optional notes for the AI…"
                    value={props.guidance.clone()}
                    rows="3"
                    oninput={guidance_input(props.on_guidance.clone())}
                />
            </label>
            <div class="story-actions">
                <button class="btn secondary" onclick={{
                    let on_detail = props.on_detail.clone();
                    let title = title.clone();
                    let synopsis = synopsis.clone();
                    let content = content.clone();
                    Callback::from(move |_| {
                        let on_detail = on_detail.clone();
                        let title = (*title).clone();
                        let synopsis = (*synopsis).clone();
                        let content = (*content).clone();
                        wasm_bindgen_futures::spawn_local(async move {
                            if let Ok(d) = api::update_beat(story_id, chapter_id, beat_id, &StoryBeatUpdate {
                                title: Some(title),
                                synopsis: Some(synopsis),
                                content: Some(content),
                                sort_order: None,
                            }).await {
                                on_detail.emit(d);
                            }
                        });
                    })
                }}>{"Save beat"}</button>
                <button class="btn" onclick={{
                    let on_detail = props.on_detail.clone();
                    let guidance = props.guidance.clone();
                    Callback::from(move |_| {
                        let on_detail = on_detail.clone();
                        let notes = guidance.clone();
                        wasm_bindgen_futures::spawn_local(async move {
                            if let Ok(d) = api::generate_prose(story_id, chapter_id, beat_id, &notes).await {
                                on_detail.emit(d);
                            }
                        });
                    })
                }}>{"Generate prose"}</button>
                <button class="btn secondary text-danger btn-compact" onclick={{
                    let on_detail = props.on_detail.clone();
                    Callback::from(move |_| {
                        let on_detail = on_detail.clone();
                        wasm_bindgen_futures::spawn_local(async move {
                            let _ = api::delete_beat(story_id, chapter_id, beat_id).await;
                            if let Ok(d) = api::get_story(story_id).await {
                                on_detail.emit(d);
                            }
                        });
                    })
                }}>{"Delete beat"}</button>
            </div>
        </div>
    }
}

fn string_input(state: UseStateHandle<String>) -> Callback<InputEvent> {
    Callback::from(move |e: InputEvent| {
        let input: HtmlInputElement = e.target_unchecked_into();
        state.set(input.value());
    })
}
