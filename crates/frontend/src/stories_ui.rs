use std::cell::RefCell;
use std::rc::Rc;

use dreamwell_types::*;
use wasm_bindgen::closure::Closure;
use wasm_bindgen::JsCast;
use web_sys::HtmlInputElement;
use yew::prelude::*;

use crate::api;
use crate::generation_ui::{
    beat_block_status, chapter_block_status, generation_error_from_content, story_notice,
    BlockGenerationStatus, GenerationStatusBar,
};
use crate::router::{AppRoute, Overlay, StoryNav};
use crate::story_save::{auto_save_status_html, AutoSaveController, AutoSavePhase};
use crate::summary_ui::{SummaryBreak, SummaryKind, SummaryView};
use crate::title_editor::{TitleEditTrigger, TitleEditor};
use crate::variables;

#[derive(Clone, Copy, PartialEq, Default)]
pub enum StorySelection {
    #[default]
    Closed,
    Basics,
    Chapter(i64),
    Beat {
        chapter_id: i64,
        beat_id: i64,
    },
}

fn toggle_selection(current: StorySelection, target: StorySelection) -> StorySelection {
    if current == target {
        StorySelection::Closed
    } else {
        target
    }
}

fn story_nav_from_selection(selection: StorySelection) -> StoryNav {
    match selection {
        StorySelection::Closed => StoryNav::None,
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
        StoryNav::None => StorySelection::Closed,
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
        _ => StoryNav::None,
    }
}

#[derive(Properties, PartialEq)]
pub struct StoriesShellProps {
    pub route: AppRoute,
    pub on_navigate: Callback<(AppRoute, bool)>,
    pub variables_enabled: bool,
}

#[function_component(StoriesShell)]
pub fn stories_shell(props: &StoriesShellProps) -> Html {
    let stories = use_state(Vec::<Story>::new);
    let detail = use_state(|| None::<StoryDetail>);
    let guidance = use_state(String::new);
    let loading = use_state(|| true);
    let detail_loading = use_state(|| false);
    let story_stream_nudge = use_mut_ref(|| None::<api::StreamNudge>);
    let selected_story_id = story_id_from_route(&props.route);
    let selection = selection_from_story_nav(story_nav_from_route(&props.route));
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
                                    nav: StoryNav::None,
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
        let detail_loading = detail_loading.clone();
        let story_stream_nudge = story_stream_nudge.clone();
        let route = props.route.clone();
        use_effect_with(route.clone(), move |route| {
            let story_id = story_id_from_route(route);
            let mut stream_holder = None::<api::StoryStream>;
            *story_stream_nudge.borrow_mut() = None;
            if let Some(story_id) = story_id {
                detail.set(None);
                detail_loading.set(true);
                let detail_for_fetch = detail.clone();
                let detail_loading_for_fetch = detail_loading.clone();
                let stories = stories.clone();
                wasm_bindgen_futures::spawn_local(async move {
                    if let Ok(d) = api::get_story(story_id).await {
                        detail_for_fetch.set(Some(d));
                    }
                    detail_loading_for_fetch.set(false);
                });
                let detail_loading_for_stream = detail_loading.clone();
                let detail_for_stream = detail.clone();
                let had_active_job = Rc::new(RefCell::new(false));
                let stream = api::StoryStream::new(story_id, move |payload| {
                    detail_loading_for_stream.set(false);
                    let was_active = *had_active_job.borrow();
                    let now_active = payload.active_job.is_some();
                    if now_active {
                        *had_active_job.borrow_mut() = true;
                    }
                    detail_for_stream.set(Some(payload.detail.clone()));
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
                    if was_active && !now_active {
                        let detail_ref = detail_for_stream.clone();
                        wasm_bindgen_futures::spawn_local(async move {
                            if let Ok(d) = api::get_story(story_id).await {
                                detail_ref.set(Some(d));
                            }
                        });
                        *had_active_job.borrow_mut() = false;
                    }
                });
                *story_stream_nudge.borrow_mut() = Some(stream.nudge());
                stream_holder = Some(stream);
            } else {
                detail.set(None);
                detail_loading.set(false);
            }
            move || {
                *story_stream_nudge.borrow_mut() = None;
                drop(stream_holder);
            }
        });
    }

    {
        let stories = stories.clone();
        let on_navigate = props.on_navigate.clone();
        let route = props.route.clone();
        let story_ids = {
            let mut ids: Vec<i64> = stories.iter().map(|s| s.id).collect();
            ids.sort_unstable();
            ids
        };
        use_effect_with((route.clone(), story_ids), move |(route, story_ids)| {
            if let AppRoute::Stories {
                story_id: Some(id), ..
            } = route
            {
                if !story_ids.is_empty() && !story_ids.contains(id) {
                    on_navigate.emit((
                        AppRoute::Stories {
                            story_id: story_ids.first().copied(),
                            nav: StoryNav::None,
                            overlay: None,
                            sidebar: false,
                        },
                        false,
                    ));
                }
            }
            || ()
        });
    }

    {
        let stories = stories.clone();
        let story_stream_nudge = story_stream_nudge.clone();
        use_effect_with((), move |_| {
            let stories = stories.clone();
            let story_stream_nudge = story_stream_nudge.clone();
            let resume: Rc<dyn Fn()> = Rc::new(move || {
                if let Some(nudge) = story_stream_nudge.borrow().clone() {
                    nudge.reconnect();
                }
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

            let window = web_sys::window();
            if let Some(window) = window.as_ref() {
                let _ = window.add_event_listener_with_callback(
                    "online",
                    online_callback.as_ref().unchecked_ref(),
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

    let bump_stream = {
        let story_stream_nudge = story_stream_nudge.clone();
        Callback::from(move |_| {
            if let Some(nudge) = story_stream_nudge.borrow().clone() {
                nudge.reconnect();
            }
        })
    };

    html! {
        <>
            if props.route.overlay() == Some(Overlay::Variables) {
                <StoryVariablesOverlay
                    story_id={selected_story_id}
                    detail={(*detail).clone()}
                    selection={selection}
                    on_close={Callback::from({
                        let on_navigate = props.on_navigate.clone();
                        let route = props.route.clone();
                        move |_| {
                            on_navigate.emit((route.clone().without_overlay(), true));
                        }
                    })}
                />
            }
            <StoryEditor
            detail={(*detail).clone()}
            detail_loading={*detail_loading}
            selection={selection}
            guidance={(*guidance).clone()}
            variables_enabled={props.variables_enabled}
            bump_stream={bump_stream.clone()}
            on_open_variables={Callback::from({
                let on_navigate = props.on_navigate.clone();
                let route = props.route.clone();
                move |_| {
                    on_navigate.emit((route.clone().with_overlay(Overlay::Variables), true));
                }
            })}
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
        />
        </>
    }
}

#[derive(Properties, PartialEq)]
struct StoryEditorProps {
    detail: Option<StoryDetail>,
    detail_loading: bool,
    selection: StorySelection,
    guidance: String,
    variables_enabled: bool,
    bump_stream: Callback<()>,
    on_open_variables: Callback<()>,
    on_guidance: Callback<String>,
    on_detail: Callback<StoryDetail>,
    on_selection: Callback<StorySelection>,
}

#[function_component(StoryEditor)]
fn story_editor(props: &StoryEditorProps) -> Html {
    if props.detail_loading {
        return html! {
            <>
                <header class="header content-header">
                    <h1 class="header-title">{"Loading story…"}</h1>
                </header>
                <div class="loading-screen muted">{"Loading story…"}</div>
            </>
        };
    }

    let Some(detail) = props.detail.clone() else {
        return html! {
            <>
                <header class="header content-header">
                    <h1 class="header-title">{"Select or create a story"}</h1>
                </header>
                <div class="loading-screen muted" style="text-align:center;">{"Stories are built chapter by chapter, beat by beat."}</div>
            </>
        };
    };

    let queued = detail.story.queued_jobs;
    let story_id = detail.story.id;
    let target = detail.story.length_preset.ref_chapters();
    let proposing_chapters = detail.story.active_job.as_ref().is_some_and(|job| {
        job.job_type == JobType::StoryProposeChapters
            && matches!(job.status, JobStatus::Queued | JobStatus::Running)
    });
    let generation_notice = story_notice(&detail);

    html! {
        <>
            <header class="header content-header">
                <div class="content-header-row">
                    <TitleEditor
                        title={detail.story.title.clone()}
                        class="header-title"
                        placeholder="Story title"
                        trigger={TitleEditTrigger::Button}
                        on_save={Callback::from({
                            let on_detail = props.on_detail.clone();
                            let story_id = detail.story.id;
                            move |title| {
                                let on_detail = on_detail.clone();
                                wasm_bindgen_futures::spawn_local(async move {
                                    if let Ok(d) = api::update_story(story_id, &StoryUpdate {
                                        title: Some(title),
                                        premise: None,
                                        tone: None,
                                        genre: None,
                                        pov: None,
                                        length_preset: None,
                                        notes: None,
                                    }).await {
                                        on_detail.emit(d);
                                    }
                                });
                            }
                        })}
                    />
                    <div class="header-actions">
                        if props.variables_enabled {
                            <button
                                class="btn secondary btn-compact header-icon-btn"
                                title="Story variables"
                                onclick={props.on_open_variables.reform(|_| ())}
                            >
                                {"Variables"}
                            </button>
                        }
                        <button
                            class="btn secondary btn-compact header-icon-btn"
                            title="Propose chapters from story basics"
                            disabled={proposing_chapters}
                            onclick={propose_chapters_action(
                                story_id,
                                props.guidance.clone(),
                                props.on_detail.clone(),
                                props.bump_stream.clone(),
                            )}
                        >
                            { if proposing_chapters { "Proposing…" } else { "Propose chapters" } }
                        </button>
                        <button
                            class="btn secondary btn-compact header-icon-btn"
                            title="Add chapter manually"
                            onclick={add_chapter_action(story_id, props.on_detail.clone(), props.on_selection.clone())}
                        >
                            {"+ Chapter"}
                        </button>
                    </div>
                </div>
                <p class="header-subtitle muted">
                    { format!(
                        "{} · {} of {} chapters",
                        detail.story.length_preset.label(),
                        detail.chapters.len(),
                        target,
                    ) }
                    if queued > 0 {
                        { format!(" · {} queued", queued) }
                    }
                </p>
            </header>
            if let Some(notice) = generation_notice {
                <GenerationStatusBar notice={notice} />
            }
            <div class="story-editor">
                <StoryBlockList
                    detail={detail}
                    selection={props.selection}
                    guidance={props.guidance.clone()}
                    variables_enabled={props.variables_enabled}
                    bump_stream={props.bump_stream.clone()}
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
    variables_enabled: bool,
    bump_stream: Callback<()>,
    on_guidance: Callback<String>,
    on_detail: Callback<StoryDetail>,
    on_selection: Callback<StorySelection>,
}

#[function_component(StoryBlockList)]
fn story_block_list(props: &StoryBlockListProps) -> Html {
    let story_id = props.detail.story.id;
    let active_job = props.detail.story.active_job.as_ref();
    let current_selection = props.selection;

    html! {
        <div class="story-blocks">
            <StoryBlockHeader
                label={"Story basics".to_string()}
                subtitle={props.detail.story.title.clone()}
                open={props.selection == StorySelection::Basics}
                on_toggle={props.on_selection.reform(move |_| {
                    toggle_selection(current_selection, StorySelection::Basics)
                })}
            />
            if props.selection == StorySelection::Basics {
                <div class="story-block-body">
                    <StoryBasicsForm
                        story={props.detail.story.clone()}
                        on_detail={props.on_detail.clone()}
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
                    <p class="muted" style="font-size:0.85rem;margin-top:0.75rem;">
                        {"Propose chapters reviews your story and returns a full chapter list — it may add, remove, reorder, or rewrite chapters. Existing beat prose is noted in the prompt but may be replaced."}
                    </p>
                    <div class="story-actions" style="margin-top:0.75rem;">
                        <button
                            class="btn"
                            disabled={props.detail.story.active_job.as_ref().is_some_and(|job| {
                                job.job_type == JobType::StoryProposeChapters
                                    && matches!(job.status, JobStatus::Queued | JobStatus::Running)
                            })}
                            onclick={propose_chapters_action(
                                story_id,
                                props.guidance.clone(),
                                props.on_detail.clone(),
                                props.bump_stream.clone(),
                            )}
                        >
                            { if props.detail.story.active_job.as_ref().is_some_and(|job| {
                                job.job_type == JobType::StoryProposeChapters
                                    && matches!(job.status, JobStatus::Queued | JobStatus::Running)
                            }) {
                                "Proposing chapters…"
                            } else {
                                "Propose chapters"
                            } }
                        </button>
                    </div>
                </div>
            }

            { for props.detail.chapters.iter().map(|ch| {
                let ch_id = ch.id;
                let ch_open = props.selection == StorySelection::Chapter(ch_id);
                let ch_label = format!("Chapter {}", ch.sort_order + 1);
                let ch_subtitle = if ch.title.is_empty() { "…".to_string() } else { ch.title.clone() };
                let chapter_status = chapter_block_status(ch, active_job);
                let chapter_target = StorySelection::Chapter(ch_id);
                html! {
                    <div key={ch_id} class="story-chapter-block">
                        <StoryBlockHeader
                            label={ch_label}
                            subtitle={ch_subtitle}
                            open={ch_open}
                            status_badge={chapter_status}
                            on_toggle={props.on_selection.reform(move |_| {
                                toggle_selection(current_selection, chapter_target)
                            })}
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
                                    summarizing_chapter={props.detail.story.active_job.as_ref().is_some_and(|job| {
                                        job.job_type == JobType::StoryChapterSummarize
                                            && job.chapter_id == Some(ch_id)
                                            && matches!(job.status, JobStatus::Queued | JobStatus::Running)
                                    })}
                                    guidance={props.guidance.clone()}
                                    bump_stream={props.bump_stream.clone()}
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
                            let beat_status = beat_block_status(beat);
                            let beat_target = StorySelection::Beat { chapter_id: ch_id, beat_id };
                            html! {
                                <div key={beat_id} class="story-beat-block">
                                    <StoryBlockHeader
                                        label={beat_label}
                                        subtitle={beat_subtitle}
                                        open={beat_open}
                                        indent={true}
                                        status_badge={beat_status}
                                        on_toggle={props.on_selection.reform(move |_| {
                                            toggle_selection(current_selection, beat_target)
                                        })}
                                    />
                                    if beat_open {
                                        <div class={classes!(
                                            "story-block-body",
                                            "story-block-body-nested",
                                            (beat.job_status == Some(JobStatus::Running)).then_some("story-block-body--streaming"),
                                        )}>
                                            <BeatEditor
                                                story_id={story_id}
                                                chapter_id={ch_id}
                                                beat={Some(beat.clone())}
                                                variables_enabled={props.variables_enabled}
                                                active_job={props.detail.story.active_job.clone()}
                                                guidance={props.guidance.clone()}
                                                bump_stream={props.bump_stream.clone()}
                                                on_guidance={props.on_guidance.clone()}
                                                on_detail={props.on_detail.clone()}
                                            />
                                        </div>
                                    }
                                </div>
                            }
                        }) }
                    </div>
                }
            }) }
        </div>
    }
}

fn alert_story_action_error(action: &str, err: String) {
    if let Some(window) = web_sys::window() {
        let _ = window.alert_with_message(&format!("Could not {action}: {err}"));
    }
}

fn propose_chapters_action(
    story_id: i64,
    guidance: String,
    on_detail: Callback<StoryDetail>,
    bump_stream: Callback<()>,
) -> Callback<MouseEvent> {
    Callback::from(move |_| {
        let on_detail = on_detail.clone();
        let bump_stream = bump_stream.clone();
        let notes = guidance.clone();
        wasm_bindgen_futures::spawn_local(async move {
            match api::propose_chapters(story_id, &notes).await {
                Ok(d) => {
                    on_detail.emit(d);
                    bump_stream.emit(());
                }
                Err(err) => alert_story_action_error("propose chapters", err),
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
            match api::create_chapter(story_id, &StoryChapterCreate::default()).await {
                Ok(d) => {
                    if let Some(ch) = d.chapters.last() {
                        on_selection.emit(StorySelection::Chapter(ch.id));
                    }
                    on_detail.emit(d);
                }
                Err(err) => alert_story_action_error("add chapter", err),
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
    status_badge: Option<BlockGenerationStatus>,
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
            if let Some(status) = props.status_badge {
                <span class={classes!("badge", status.variant_class())}>{ status.label() }</span>
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
    on_detail: Callback<StoryDetail>,
}

#[function_component(StoryBasicsForm)]
fn story_basics_form(props: &StoryBasicsFormProps) -> Html {
    let draft = use_state(|| StoryBasics::from(props.story.clone()));
    let save_phase = use_state(|| AutoSavePhase::Synced);
    let save_error = use_state(|| None::<String>);
    let save_controller = AutoSaveController::new(save_phase.clone(), save_error.clone());

    {
        let draft = draft.clone();
        let story = props.story.clone();
        use_effect_with(story.id, move |_| {
            draft.set(StoryBasics::from(story));
            || ()
        });
    }

    let schedule_save = {
        let draft = draft.clone();
        let on_detail = props.on_detail.clone();
        let save_controller = save_controller.clone();
        Callback::from(move |_| {
            let draft = (*draft).clone();
            let on_detail = on_detail.clone();
            let controller = save_controller.clone();
            let controller_for_save = controller.clone();
            controller.schedule(move || {
                let on_detail = on_detail.clone();
                let controller = controller_for_save.clone();
                let draft = draft.clone();
                wasm_bindgen_futures::spawn_local(async move {
                    match api::update_story(
                        draft.id,
                        &StoryUpdate {
                            title: Some(draft.title),
                            premise: Some(draft.premise),
                            tone: Some(draft.tone),
                            genre: Some(draft.genre),
                            pov: Some(draft.pov),
                            length_preset: Some(draft.length_preset),
                            notes: Some(draft.notes),
                        },
                    )
                    .await
                    {
                        Ok(d) => {
                            controller.mark_saved();
                            on_detail.emit(d);
                        }
                        Err(err) => controller.mark_failed(err),
                    }
                });
            });
        })
    };

    html! {
        <div class="story-form">
            <label class="field"><span class="muted">{"Title"}</span>
                <input type="text" value={draft.title.clone()} oninput={{
                    let draft = draft.clone();
                    let schedule_save = schedule_save.clone();
                    Callback::from(move |e: InputEvent| {
                        let input: HtmlInputElement = e.target_unchecked_into();
                        let mut next = (*draft).clone();
                        next.title = input.value();
                        draft.set(next);
                        schedule_save.emit(());
                    })
                }} />
            </label>
            <label class="field"><span class="muted">{"Premise"}</span>
                <textarea value={draft.premise.clone()} rows="3" oninput={{
                    let draft = draft.clone();
                    let schedule_save = schedule_save.clone();
                    Callback::from(move |e: InputEvent| {
                        let input: HtmlInputElement = e.target_unchecked_into();
                        let mut next = (*draft).clone();
                        next.premise = input.value();
                        draft.set(next);
                        schedule_save.emit(());
                    })
                }} />
            </label>
            <div style="display:grid;grid-template-columns:1fr 1fr;gap:0.75rem;">
                <label class="field"><span class="muted">{"Tone"}</span>
                    <input type="text" value={draft.tone.clone()} oninput={{
                        let draft = draft.clone();
                        let schedule_save = schedule_save.clone();
                        Callback::from(move |e: InputEvent| {
                            let input: HtmlInputElement = e.target_unchecked_into();
                            let mut next = (*draft).clone();
                            next.tone = input.value();
                            draft.set(next);
                            schedule_save.emit(());
                        })
                    }} />
                </label>
                <label class="field"><span class="muted">{"Genre"}</span>
                    <input type="text" value={draft.genre.clone()} oninput={{
                        let draft = draft.clone();
                        let schedule_save = schedule_save.clone();
                        Callback::from(move |e: InputEvent| {
                            let input: HtmlInputElement = e.target_unchecked_into();
                            let mut next = (*draft).clone();
                            next.genre = input.value();
                            draft.set(next);
                            schedule_save.emit(());
                        })
                    }} />
                </label>
            </div>
            <label class="field"><span class="muted">{"POV"}</span>
                <input type="text" value={draft.pov.clone()} placeholder="e.g. third person limited" oninput={{
                    let draft = draft.clone();
                    let schedule_save = schedule_save.clone();
                    Callback::from(move |e: InputEvent| {
                        let input: HtmlInputElement = e.target_unchecked_into();
                        let mut next = (*draft).clone();
                        next.pov = input.value();
                        draft.set(next);
                        schedule_save.emit(());
                    })
                }} />
            </label>
            <label class="field"><span class="muted">{"Length"}</span>
                <select onchange={{
                    let draft = draft.clone();
                    let schedule_save = schedule_save.clone();
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
                        schedule_save.emit(());
                    })
                }}>
                    { for [LengthPreset::Flash, LengthPreset::Short, LengthPreset::Novella, LengthPreset::Novel].iter().map(|p| {
                        let selected = draft.length_preset == *p;
                        html! { <option value={preset_value(*p)} selected={selected}>{ p.label() }</option> }
                    }) }
                </select>
            </label>
            <label class="field"><span class="muted">{"Notes"}</span>
                <textarea value={draft.notes.clone()} rows="2" oninput={{
                    let draft = draft.clone();
                    let schedule_save = schedule_save.clone();
                    Callback::from(move |e: InputEvent| {
                        let input: HtmlInputElement = e.target_unchecked_into();
                        let mut next = (*draft).clone();
                        next.notes = input.value();
                        draft.set(next);
                        schedule_save.emit(());
                    })
                }} />
            </label>
            { auto_save_status_html(*save_phase, (*save_error).as_deref()) }
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

fn chapter_has_substantial_prose(chapter: &StoryChapter) -> bool {
    chapter
        .beats
        .iter()
        .any(|beat| beat.content.chars().count() > 80)
}

const MANUAL_VARIABLE_SOURCE: i64 = -1;

#[derive(Clone, Copy, PartialEq, Eq)]
enum VariableWhen {
    Manual,
    Beat { chapter_order: i64, beat_order: i64 },
}

fn variable_when_from_selection(detail: &StoryDetail, selection: StorySelection) -> VariableWhen {
    match selection {
        StorySelection::Beat {
            chapter_id,
            beat_id,
        } => detail
            .chapters
            .iter()
            .find(|ch| ch.id == chapter_id)
            .and_then(|ch| {
                ch.beats
                    .iter()
                    .find(|beat| beat.id == beat_id)
                    .map(|beat| VariableWhen::Beat {
                        chapter_order: ch.sort_order,
                        beat_order: beat.sort_order,
                    })
            })
            .unwrap_or(VariableWhen::Manual),
        _ => VariableWhen::Manual,
    }
}

fn variable_when_label(when: VariableWhen, detail: &StoryDetail) -> String {
    match when {
        VariableWhen::Manual => "Story-wide (manual)".to_string(),
        VariableWhen::Beat {
            chapter_order,
            beat_order,
        } => {
            let chapter_num = chapter_order + 1;
            let beat_num = beat_order + 1;
            if let Some(ch) = detail
                .chapters
                .iter()
                .find(|ch| ch.sort_order == chapter_order)
            {
                let beat_title = ch
                    .beats
                    .iter()
                    .find(|beat| beat.sort_order == beat_order)
                    .map(|beat| {
                        if beat.title.is_empty() {
                            format!("Beat {beat_num}")
                        } else {
                            beat.title.clone()
                        }
                    })
                    .unwrap_or_else(|| format!("Beat {beat_num}"));
                format!("Ch {chapter_num} · {beat_title}")
            } else {
                format!("Ch {chapter_num} · Beat {beat_num}")
            }
        }
    }
}

fn variable_when_option_value(when: VariableWhen) -> String {
    match when {
        VariableWhen::Manual => "manual".to_string(),
        VariableWhen::Beat {
            chapter_order,
            beat_order,
        } => format!("{chapter_order}:{beat_order}"),
    }
}

fn variable_when_from_option(value: &str) -> Option<VariableWhen> {
    if value == "manual" {
        return Some(VariableWhen::Manual);
    }
    let (chapter_order, beat_order) = value.split_once(':')?;
    Some(VariableWhen::Beat {
        chapter_order: chapter_order.parse().ok()?,
        beat_order: beat_order.parse().ok()?,
    })
}

#[derive(Properties, PartialEq)]
struct ChapterEditorProps {
    story_id: i64,
    chapter: Option<StoryChapter>,
    #[prop_or(false)]
    proposing_beats: bool,
    #[prop_or(false)]
    summarizing_chapter: bool,
    guidance: String,
    bump_stream: Callback<()>,
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
    let save_phase = use_state(|| AutoSavePhase::Synced);
    let save_error = use_state(|| None::<String>);
    let save_controller = AutoSaveController::new(save_phase.clone(), save_error.clone());

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

    let schedule_save = {
        let title = title.clone();
        let synopsis = synopsis.clone();
        let on_detail = props.on_detail.clone();
        let save_controller = save_controller.clone();
        let story_id = props.story_id;
        let chapter_id = chapter.id;
        Callback::from(move |_| {
            let title = (*title).clone();
            let synopsis = (*synopsis).clone();
            let on_detail = on_detail.clone();
            let controller = save_controller.clone();
            let controller_for_save = controller.clone();
            controller.schedule(move || {
                let on_detail = on_detail.clone();
                let controller = controller_for_save.clone();
                wasm_bindgen_futures::spawn_local(async move {
                    match api::update_chapter(
                        story_id,
                        chapter_id,
                        &StoryChapterUpdate {
                            title: Some(title),
                            synopsis: Some(synopsis),
                            sort_order: None,
                        },
                    )
                    .await
                    {
                        Ok(d) => {
                            controller.mark_saved();
                            on_detail.emit(d);
                        }
                        Err(err) => controller.mark_failed(err),
                    }
                });
            });
        })
    };

    let story_id = props.story_id;
    let chapter_id = chapter.id;
    let proposing_beats = props.proposing_beats;
    let summarizing_chapter = props.summarizing_chapter;
    let summary_stale = chapter_has_substantial_prose(&chapter) && !chapter.prose_summary_valid;

    html! {
        <div class="story-form">
            <label class="field"><span class="muted">{"Title"}</span>
                <input type="text" value={(*title).clone()} oninput={{
                    let title = title.clone();
                    let schedule_save = schedule_save.clone();
                    Callback::from(move |e: InputEvent| {
                        let input: HtmlInputElement = e.target_unchecked_into();
                        title.set(input.value());
                        schedule_save.emit(());
                    })
                }} />
            </label>
            <label class="field"><span class="muted">{"Synopsis"}</span>
                <textarea value={(*synopsis).clone()} rows="5" oninput={{
                    let synopsis = synopsis.clone();
                    let schedule_save = schedule_save.clone();
                    Callback::from(move |e: InputEvent| {
                        let input: HtmlInputElement = e.target_unchecked_into();
                        synopsis.set(input.value());
                        schedule_save.emit(());
                    })
                }} />
            </label>
            { auto_save_status_html(*save_phase, (*save_error).as_deref()) }
            if summary_stale {
                <p class="message-error" style="font-size:0.85rem;margin-top:0.75rem;" role="alert">
                    {"Prose summary is out of date — summarize from prose to refresh context for later chapters."}
                </p>
            }
            if summarizing_chapter || chapter.prose_summary_valid {
                <div style="margin-top:0.75rem;">
                    <SummaryBreak
                        kind={SummaryKind::ChapterProse}
                        pending={summarizing_chapter}
                    />
                    <SummaryView
                        text={chapter.prose_summary.clone()}
                        pending={summarizing_chapter}
                        kind={SummaryKind::ChapterProse}
                    />
                </div>
            }
            <div class="story-actions" style="margin-top:0.75rem;">
                <button class="btn secondary" disabled={summarizing_chapter || !chapter_has_substantial_prose(&chapter)} onclick={{
                    let on_detail = props.on_detail.clone();
                    let bump_stream = props.bump_stream.clone();
                    Callback::from(move |_| {
                        let on_detail = on_detail.clone();
                        let bump_stream = bump_stream.clone();
                        wasm_bindgen_futures::spawn_local(async move {
                            match api::summarize_chapter_prose(story_id, chapter_id).await {
                                Ok(d) => {
                                    on_detail.emit(d);
                                    bump_stream.emit(());
                                }
                                Err(err) => alert_story_action_error("summarize chapter", err),
                            }
                        });
                    })
                }}>{ if summarizing_chapter { "Summarizing…" } else { "Summarize from prose" } }</button>
            </div>
            <label class="field" style="margin-top:1rem;">
                <span class="muted">{"Guidance for proposal"}</span>
                <textarea
                    placeholder="Optional notes — e.g. split the confrontation into two beats…"
                    value={props.guidance.clone()}
                    rows="3"
                    oninput={guidance_input(props.on_guidance.clone())}
                />
            </label>
            <div class="story-actions">
                <button class="btn" disabled={proposing_beats} onclick={{
                    let on_detail = props.on_detail.clone();
                    let bump_stream = props.bump_stream.clone();
                    let guidance = props.guidance.clone();
                    Callback::from(move |_| {
                        let on_detail = on_detail.clone();
                        let bump_stream = bump_stream.clone();
                        let notes = guidance.clone();
                        wasm_bindgen_futures::spawn_local(async move {
                            match api::propose_beats(story_id, chapter_id, &notes).await {
                                Ok(d) => {
                                    on_detail.emit(d);
                                    bump_stream.emit(());
                                }
                                Err(err) => alert_story_action_error("propose beats", err),
                            }
                        });
                    })
                }}>{ if proposing_beats { "Proposing beats…" } else { "Propose beats" } }</button>
                <button class="btn secondary" onclick={{
                    let on_detail = props.on_detail.clone();
                    Callback::from(move |_| {
                        let on_detail = on_detail.clone();
                        wasm_bindgen_futures::spawn_local(async move {
                            match api::create_beat(story_id, chapter_id, &StoryBeatCreate::default())
                                .await
                            {
                                Ok(d) => on_detail.emit(d),
                                Err(err) => alert_story_action_error("add beat", err),
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
    #[prop_or(false)]
    variables_enabled: bool,
    #[prop_or_default]
    active_job: Option<Job>,
    guidance: String,
    bump_stream: Callback<()>,
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
    let user_edited_prose = use_state(|| false);
    let save_phase = use_state(|| AutoSavePhase::Synced);
    let save_error = use_state(|| None::<String>);
    let save_controller = AutoSaveController::new(save_phase.clone(), save_error.clone());

    {
        let title = title.clone();
        let synopsis = synopsis.clone();
        let content = content.clone();
        let user_edited_prose = user_edited_prose.clone();
        let beat = beat.clone();
        use_effect_with(beat.id, move |_| {
            title.set(beat.title.clone());
            synopsis.set(beat.synopsis.clone());
            content.set(beat.content.clone());
            user_edited_prose.set(false);
            || ()
        });
    }

    {
        let content = content.clone();
        let user_edited_prose = user_edited_prose.clone();
        let server_content = beat.content.clone();
        use_effect_with(
            (beat.id, server_content.clone()),
            move |(_, server_content)| {
                if !*user_edited_prose {
                    content.set(server_content.clone());
                }
                || ()
            },
        );
    }

    let prose_generating = props.active_job.as_ref().is_some_and(|job| {
        job.job_type == JobType::StoryBeatProse
            && job.beat_id == Some(beat.id)
            && matches!(job.status, JobStatus::Queued | JobStatus::Running)
    });
    let rechecking_variables = props.active_job.as_ref().is_some_and(|job| {
        job.job_type == JobType::StoryBeatVariableRecheck
            && job.beat_id == Some(beat.id)
            && matches!(job.status, JobStatus::Queued | JobStatus::Running)
    });

    let queued = matches!(beat.job_status, Some(JobStatus::Queued));
    let streaming = matches!(beat.job_status, Some(JobStatus::Running));
    let generation_active = queued || streaming;
    let generation_error = generation_error_from_content(&beat.content);
    let prose_failure_only = generation_error.is_some();
    let story_id = props.story_id;
    let chapter_id = props.chapter_id;
    let beat_id = beat.id;

    let schedule_save = {
        let title = title.clone();
        let synopsis = synopsis.clone();
        let content = content.clone();
        let on_detail = props.on_detail.clone();
        let save_controller = save_controller.clone();
        Callback::from(move |include_content: bool| {
            let title = (*title).clone();
            let synopsis = (*synopsis).clone();
            let content = (*content).clone();
            let on_detail = on_detail.clone();
            let controller = save_controller.clone();
            let save_content = include_content && !prose_generating;
            let controller_for_save = controller.clone();
            controller.schedule(move || {
                let on_detail = on_detail.clone();
                let controller = controller_for_save.clone();
                wasm_bindgen_futures::spawn_local(async move {
                    let mut update = StoryBeatUpdate {
                        title: Some(title),
                        synopsis: Some(synopsis),
                        content: None,
                        sort_order: None,
                    };
                    if save_content {
                        update.content = Some(content);
                    }
                    match api::update_beat(story_id, chapter_id, beat_id, &update).await {
                        Ok(d) => {
                            controller.mark_saved();
                            on_detail.emit(d);
                        }
                        Err(err) => controller.mark_failed(err),
                    }
                });
            });
        })
    };

    let prose_display = if prose_failure_only || ((*content).is_empty() && queued) {
        String::new()
    } else if streaming {
        variables::strip_variables_for_display(&content, true)
    } else {
        variables::strip_variables_for_display(&content, false)
    };

    let prose_placeholder = if queued && (*content).is_empty() {
        "Waiting in queue…"
    } else if streaming && (*content).is_empty() {
        "…"
    } else {
        ""
    };

    let show_recheck = props.variables_enabled && !(*content).trim().is_empty();
    let variable_update_count = beat.variable_updates.len();

    html! {
        <div class="story-form">
            <label class="field"><span class="muted">{"Title"}</span>
                <input type="text" value={(*title).clone()} oninput={{
                    let title = title.clone();
                    let schedule_save = schedule_save.clone();
                    Callback::from(move |e: InputEvent| {
                        let input: HtmlInputElement = e.target_unchecked_into();
                        title.set(input.value());
                        schedule_save.emit(false);
                    })
                }} />
            </label>
            <label class="field"><span class="muted">{"Synopsis"}</span>
                <textarea value={(*synopsis).clone()} rows="3" oninput={{
                    let synopsis = synopsis.clone();
                    let schedule_save = schedule_save.clone();
                    Callback::from(move |e: InputEvent| {
                        let input: HtmlInputElement = e.target_unchecked_into();
                        synopsis.set(input.value());
                        schedule_save.emit(false);
                    })
                }} />
            </label>
            <label class="field"><span class="muted">{"Prose"}</span>
                <textarea
                    class={classes!(
                        "prose-editor",
                        streaming.then_some("story-prose--streaming"),
                    )}
                    value={prose_display}
                    placeholder={prose_placeholder}
                    rows="12"
                    readonly={generation_active && !*user_edited_prose}
                    oninput={{
                        let content = content.clone();
                        let user_edited_prose = user_edited_prose.clone();
                        let schedule_save = schedule_save.clone();
                        Callback::from(move |e: InputEvent| {
                            let input: HtmlInputElement = e.target_unchecked_into();
                            user_edited_prose.set(true);
                            content.set(input.value());
                            schedule_save.emit(true);
                        })
                    }}
                />
                if queued && (*content).is_empty() && !prose_failure_only {
                    <span class="muted" style="font-size:0.85rem;">{"Waiting in queue…"}</span>
                }
                if streaming && !prose_failure_only {
                    <div class="message-streaming-note muted">{"Still writing…"}</div>
                }
                if let Some(error) = generation_error {
                    <div class="message-error" role="alert">
                        <strong>{"Generation failed"}</strong>
                        <span>{ error }</span>
                    </div>
                }
            </label>
            if variable_update_count > 0 {
                <p class="muted" style="font-size:0.85rem;">
                    { format!("Updated variables ({variable_update_count})") }
                </p>
            }
            { auto_save_status_html(*save_phase, (*save_error).as_deref()) }
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
                <button class="btn" disabled={generation_active} onclick={{
                    let on_detail = props.on_detail.clone();
                    let bump_stream = props.bump_stream.clone();
                    let guidance = props.guidance.clone();
                    Callback::from(move |_| {
                        let on_detail = on_detail.clone();
                        let bump_stream = bump_stream.clone();
                        let notes = guidance.clone();
                        wasm_bindgen_futures::spawn_local(async move {
                            match api::generate_prose(story_id, chapter_id, beat_id, &notes).await
                            {
                                Ok(d) => {
                                    on_detail.emit(d);
                                    bump_stream.emit(());
                                }
                                Err(err) => alert_story_action_error("generate prose", err),
                            }
                        });
                    })
                }}>{"Generate prose"}</button>
                if show_recheck {
                    <button class="btn secondary" disabled={rechecking_variables || generation_active} onclick={{
                        let on_detail = props.on_detail.clone();
                        let bump_stream = props.bump_stream.clone();
                        Callback::from(move |_| {
                            let on_detail = on_detail.clone();
                            let bump_stream = bump_stream.clone();
                            wasm_bindgen_futures::spawn_local(async move {
                                match api::recheck_beat_variables(story_id, chapter_id, beat_id).await
                                {
                                    Ok(_) => {
                                        bump_stream.emit(());
                                        match api::get_story(story_id).await {
                                            Ok(d) => on_detail.emit(d),
                                            Err(err) => {
                                                alert_story_action_error("refresh story", err)
                                            }
                                        }
                                    }
                                    Err(err) => alert_story_action_error("recheck variables", err),
                                }
                            });
                        })
                    }}>{ if rechecking_variables { "Rechecking…" } else { "Recheck variables" } }</button>
                }
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
                }} disabled={generation_active}>{"Delete beat"}</button>
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

fn format_variable_source(chapter_order: i64, beat_order: i64) -> String {
    if chapter_order < 0 {
        "Manual".to_string()
    } else {
        format!("Ch {} · Beat {}", chapter_order + 1, beat_order + 1)
    }
}

#[derive(Properties, PartialEq)]
pub struct StoryVariablesOverlayProps {
    pub story_id: Option<i64>,
    #[prop_or_default]
    pub detail: Option<StoryDetail>,
    #[prop_or_default]
    pub selection: StorySelection,
    pub on_close: Callback<()>,
}

#[function_component(StoryVariablesOverlay)]
pub fn story_variables_overlay(props: &StoryVariablesOverlayProps) -> Html {
    let variables = use_state(Vec::<StoryVariable>::new);
    let key = use_state(String::new);
    let value = use_state(String::new);
    let editing_key = use_state(|| None::<String>);
    let when = use_state(|| {
        props
            .detail
            .as_ref()
            .map(|detail| variable_when_from_selection(detail, props.selection))
            .unwrap_or(VariableWhen::Manual)
    });

    {
        let when = when.clone();
        let detail = props.detail.clone();
        let selection = props.selection;
        use_effect_with((detail.clone(), selection), move |(detail, selection)| {
            if let Some(detail) = detail.as_ref() {
                when.set(variable_when_from_selection(detail, *selection));
            } else {
                when.set(VariableWhen::Manual);
            }
            || ()
        });
    }

    {
        let variables = variables.clone();
        use_effect_with(props.story_id, move |story_id| {
            if let Some(story_id) = *story_id {
                let variables = variables.clone();
                wasm_bindgen_futures::spawn_local(async move {
                    if let Ok(list) = api::get_story_variables(story_id).await {
                        variables.set(list);
                    }
                });
            } else {
                variables.set(vec![]);
            }
            || ()
        });
    }

    let Some(story_id) = props.story_id else {
        return html! {
            <div class="settings-popover panel-overlay">
                <div class="settings-header">
                    <h2>{"Variables"}</h2>
                    <button class="btn secondary btn-compact" onclick={props.on_close.reform(|_| ())}>{"Close"}</button>
                </div>
                <p class="muted">{"Select a story to view variables."}</p>
            </div>
        };
    };

    let detail = props.detail.as_ref();
    let when_options: Vec<VariableWhen> = {
        let mut options = vec![VariableWhen::Manual];
        if let Some(detail) = detail {
            for chapter in &detail.chapters {
                for beat in &chapter.beats {
                    options.push(VariableWhen::Beat {
                        chapter_order: chapter.sort_order,
                        beat_order: beat.sort_order,
                    });
                }
            }
        }
        options
    };

    html! {
        <div class="settings-popover panel-overlay">
            <div class="settings-header">
                <h2>{"Variables"}</h2>
                <button class="btn secondary btn-compact" onclick={props.on_close.reform(|_| ())}>{"Close"}</button>
            </div>
            <div class="panel-overlay-body">
                <p class="muted">{"Story variables are injected when generating beat prose. Only facts from prior beats (plus manual entries) are visible to each generation — not future beats."}</p>
                { for variables.iter().map(|v| {
                    let variable_key = v.key.clone();
                    let variable_value = v.value.clone();
                    let source = format_variable_source(v.source_chapter_order, v.source_beat_order);
                    html! {
                        <div class="variable-card">
                            <div class="variable-card-header">
                                <strong>{ &v.key }</strong>
                                <span class="muted">{ source }</span>
                                <div class="variable-card-actions">
                                    <button class="btn secondary btn-compact" onclick={{
                                        let key = key.clone();
                                        let value = value.clone();
                                        let editing_key = editing_key.clone();
                                        let when = when.clone();
                                        let variable_key = variable_key.clone();
                                        let variable_value = variable_value.clone();
                                        let source_chapter_order = v.source_chapter_order;
                                        let source_beat_order = v.source_beat_order;
                                        Callback::from(move |_| {
                                            key.set(variable_key.clone());
                                            value.set(variable_value.clone());
                                            editing_key.set(Some(variable_key.clone()));
                                            if source_chapter_order == MANUAL_VARIABLE_SOURCE {
                                                when.set(VariableWhen::Manual);
                                            } else {
                                                when.set(VariableWhen::Beat {
                                                    chapter_order: source_chapter_order,
                                                    beat_order: source_beat_order,
                                                });
                                            }
                                        })
                                    }}>{"edit"}</button>
                                    <button class="btn secondary btn-compact" onclick={{
                                        let variables = variables.clone();
                                        let variable_key = variable_key.clone();
                                        Callback::from(move |_| {
                                            let variables = variables.clone();
                                            let variable_key = variable_key.clone();
                                            wasm_bindgen_futures::spawn_local(async move {
                                                let _ = api::delete_story_variable(story_id, &variable_key).await;
                                                if let Ok(list) = api::get_story_variables(story_id).await {
                                                    variables.set(list);
                                                }
                                            });
                                        })
                                    }}>{"delete"}</button>
                                </div>
                            </div>
                            <div>{ &v.value }</div>
                        </div>
                    }
                }) }
                <div class="variable-form">
                    <label class="field">
                        <span class="muted">{"When"}</span>
                        <select onchange={{
                            let when = when.clone();
                            Callback::from(move |e: Event| {
                                let input: HtmlInputElement = e.target_unchecked_into();
                                if let Some(next) = variable_when_from_option(&input.value()) {
                                    when.set(next);
                                }
                            })
                        }}>
                            { for when_options.iter().map(|option| {
                                let selected = *option == *when;
                                let label = detail
                                    .map(|detail| variable_when_label(*option, detail))
                                    .unwrap_or_else(|| "Story-wide (manual)".to_string());
                                html! {
                                    <option
                                        value={variable_when_option_value(*option)}
                                        selected={selected}
                                    >
                                        { label }
                                    </option>
                                }
                            }) }
                        </select>
                    </label>
                    <input type="text" placeholder="Key" value={(*key).clone()} oninput={string_input(key.clone())} />
                    <textarea placeholder="Value" value={(*value).clone()} rows="3" oninput={string_input(value.clone())} />
                    <button class="btn" onclick={{
                        let key = key.clone();
                        let value = value.clone();
                        let editing_key = editing_key.clone();
                        let variables = variables.clone();
                        let when = *when;
                        Callback::from(move |_| {
                            if (*key).trim().is_empty() {
                                return;
                            }
                            let variables = variables.clone();
                            let (source_chapter_order, source_beat_order) = match when {
                                VariableWhen::Manual => (
                                    Some(MANUAL_VARIABLE_SOURCE),
                                    Some(MANUAL_VARIABLE_SOURCE),
                                ),
                                VariableWhen::Beat {
                                    chapter_order,
                                    beat_order,
                                } => (Some(chapter_order), Some(beat_order)),
                            };
                            let payload = StoryVariableUpdate {
                                key: (*key).trim().to_string(),
                                value: (*value).clone(),
                                source_chapter_order,
                                source_beat_order,
                            };
                            let editing = (*editing_key).clone();
                            let key = key.clone();
                            let value = value.clone();
                            wasm_bindgen_futures::spawn_local(async move {
                                if api::upsert_story_variable(story_id, &payload).await.is_ok() {
                                    if let Ok(list) = api::get_story_variables(story_id).await {
                                        variables.set(list);
                                    }
                                    if editing.is_none() {
                                        key.set(String::new());
                                        value.set(String::new());
                                    }
                                }
                            });
                        })
                    }}>{ if (*editing_key).is_some() { "Save" } else { "Add variable" } }</button>
                </div>
            </div>
        </div>
    }
}
