use dreamwell_types::*;
use web_sys::HtmlInputElement;
use yew::prelude::*;

use crate::api;
use crate::queue_ui::QueueBar;
use crate::MobilePane;

#[derive(Clone, Copy, PartialEq)]
pub enum StorySelection {
    Basics,
    Chapter(i64),
    Beat { chapter_id: i64, beat_id: i64 },
}

#[derive(Properties, PartialEq)]
pub struct StoriesShellProps {
    pub queue: Option<QueueStatus>,
    pub on_open_queue: Callback<()>,
    #[prop_or_default]
    pub initial_story_id: Option<i64>,
}

#[function_component(StoriesShell)]
pub fn stories_shell(props: &StoriesShellProps) -> Html {
    let stories = use_state(Vec::<Story>::new);
    let selected_story_id = use_state(|| None::<i64>);
    let detail = use_state(|| None::<StoryDetail>);
    let selection = use_state(|| StorySelection::Basics);
    let guidance = use_state(String::new);
    let loading = use_state(|| true);
    let mobile_pane = use_state(|| MobilePane::Main);

    {
        let stories = stories.clone();
        let selected_story_id = selected_story_id.clone();
        let loading = loading.clone();
        let initial_story_id = props.initial_story_id;
        use_effect_with((), move |_| {
            wasm_bindgen_futures::spawn_local(async move {
                if let Ok(list) = api::list_stories().await {
                    if let Some(id) = initial_story_id {
                        selected_story_id.set(Some(id));
                    } else if let Some(first) = list.first() {
                        selected_story_id.set(Some(first.id));
                    }
                    stories.set(list);
                }
                loading.set(false);
            });
            || ()
        });
    }

    {
        let detail = detail.clone();
        let stories = stories.clone();
        let selected_story_id = *selected_story_id;
        let selection = selection.clone();
        use_effect_with(selected_story_id, move |story_id| {
            let mut stream_holder = None::<api::StoryStream>;
            if let Some(story_id) = *story_id {
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
                selection.set(StorySelection::Basics);
            }
            move || drop(stream_holder)
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

    html! {
        <>
            if *mobile_pane == MobilePane::Sidebar {
                <div class="drawer-backdrop" onclick={Callback::from({
                    let mobile_pane = mobile_pane.clone();
                    move |_| mobile_pane.set(MobilePane::Main)
                })} />
            }
            <div class={classes!(
                "app-shell",
                (*mobile_pane == MobilePane::Sidebar).then_some("pane-sidebar"),
            )}>
            <StorySidebar
                stories={(*stories).clone()}
                detail={(*detail).clone()}
                selected_id={*selected_story_id}
                selection={*selection}
                on_select_story={Callback::from({
                    let selected_story_id = selected_story_id.clone();
                    let selection = selection.clone();
                    let mobile_pane = mobile_pane.clone();
                    move |id| {
                        selected_story_id.set(Some(id));
                        selection.set(StorySelection::Basics);
                        mobile_pane.set(MobilePane::Main);
                    }
                })}
                on_select_chapter={Callback::from({
                    let selection = selection.clone();
                    let mobile_pane = mobile_pane.clone();
                    move |id| {
                        selection.set(StorySelection::Chapter(id));
                        mobile_pane.set(MobilePane::Main);
                    }
                })}
                on_select_beat={Callback::from({
                    let selection = selection.clone();
                    let mobile_pane = mobile_pane.clone();
                    move |(chapter_id, beat_id)| {
                        selection.set(StorySelection::Beat { chapter_id, beat_id });
                        mobile_pane.set(MobilePane::Main);
                    }
                })}
                on_new={Callback::from({
                    let stories = stories.clone();
                    let selected_story_id = selected_story_id.clone();
                    let selection = selection.clone();
                    move |_| {
                        let stories = stories.clone();
                        let selected_story_id = selected_story_id.clone();
                        let selection = selection.clone();
                        wasm_bindgen_futures::spawn_local(async move {
                            let payload = StoryCreate {
                                title: format!("Story {}", stories.len() + 1),
                                ..Default::default()
                            };
                            if let Ok(d) = api::create_story(&payload).await {
                                if let Ok(list) = api::list_stories().await {
                                    stories.set(list);
                                }
                                selected_story_id.set(Some(d.story.id));
                                selection.set(StorySelection::Basics);
                            }
                        });
                    }
                })}
                on_delete={Callback::from({
                    let stories = stories.clone();
                    let selected_story_id = selected_story_id.clone();
                    move |id| {
                        let stories = stories.clone();
                        let selected_story_id = selected_story_id.clone();
                        wasm_bindgen_futures::spawn_local(async move {
                            let _ = api::delete_story(id).await;
                            if let Ok(list) = api::list_stories().await {
                                if *selected_story_id == Some(id) {
                                    selected_story_id.set(list.first().map(|s| s.id));
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
                    selection={*selection}
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
                        let selection = selection.clone();
                        move |s| selection.set(s)
                    })}
                    on_open_sidebar={Callback::from({
                        let mobile_pane = mobile_pane.clone();
                        move |_| mobile_pane.set(MobilePane::Sidebar)
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
    detail: Option<StoryDetail>,
    selected_id: Option<i64>,
    selection: StorySelection,
    on_select_story: Callback<i64>,
    on_select_chapter: Callback<i64>,
    on_select_beat: Callback<(i64, i64)>,
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
                            if selected {
                                if let Some(detail) = &props.detail {
                                    <div class="story-tree">
                                        <div class={tree_item_class(props.selection == StorySelection::Basics)}
                                            onclick={props.on_select_story.reform(move |_| id)}>
                                            {"Story basics"}
                                        </div>
                                        { for detail.chapters.iter().map(|ch| {
                                            let ch_id = ch.id;
                                            let ch_selected = props.selection == StorySelection::Chapter(ch_id);
                                            html! {
                                                <>
                                                    <div class={tree_item_class(ch_selected)}
                                                        onclick={props.on_select_chapter.reform(move |_| ch_id)}>
                                                        { format!("Ch. {} — {}", ch.sort_order + 1, if ch.title.is_empty() { "…" } else { &ch.title }) }
                                                    </div>
                                                    { for ch.beats.iter().map(|beat| {
                                                        let beat_id = beat.id;
                                                        let beat_selected = props.selection == StorySelection::Beat { chapter_id: ch_id, beat_id };
                                                        let streaming = matches!(beat.job_status, Some(JobStatus::Running) | Some(JobStatus::Queued));
                                                        html! {
                                                            <div class={classes!(tree_item_class(beat_selected), "tree-beat")}
                                                                onclick={props.on_select_beat.reform(move |_| (ch_id, beat_id))}>
                                                                { format!("  Beat {} — {}", beat.sort_order + 1, if beat.title.is_empty() { "…" } else { &beat.title }) }
                                                                if streaming { <span class="badge">{"…"}</span> }
                                                            </div>
                                                        }
                                                    }) }
                                                </>
                                            }
                                        }) }
                                    </div>
                                }
                            }
                        </div>
                    }
                }) }
            </div>
        </aside>
    }
}

fn tree_item_class(selected: bool) -> Classes {
    classes!("tree-item", selected.then_some("selected"))
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

    let story_id = detail.story.id;
    let generating = detail.story.active_job.is_some();

    html! {
        <>
            <header class="header">
                <div class="mobile-toolbar">
                    <button class="btn secondary" onclick={props.on_open_sidebar.reform(|_| ())}>{"Stories"}</button>
                </div>
                <h1 class="header-title">{ detail.story.title.clone() }</h1>
                <p class="header-subtitle muted">
                    { format!("{} · {} chapters target", detail.story.length_preset.label(), detail.story.length_preset.ref_chapters()) }
                </p>
            </header>
            <div class="story-editor">
                { match props.selection {
                    StorySelection::Basics => html! {
                        <StoryBasicsForm
                            story={detail.story.clone()}
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
                    },
                    StorySelection::Chapter(chapter_id) => {
                        let chapter = detail.chapters.iter().find(|c| c.id == chapter_id).cloned();
                        html! {
                            <ChapterEditor
                                story_id={story_id}
                                chapter={chapter}
                                guidance={props.guidance.clone()}
                                generating={generating}
                                on_guidance={props.on_guidance.clone()}
                                on_detail={props.on_detail.clone()}
                            />
                        }
                    }
                    StorySelection::Beat { chapter_id, beat_id } => {
                        let chapter = detail.chapters.iter().find(|c| c.id == chapter_id);
                        let beat = chapter.and_then(|c| c.beats.iter().find(|b| b.id == beat_id)).cloned();
                        html! {
                            <BeatEditor
                                story_id={story_id}
                                chapter_id={chapter_id}
                                beat={beat}
                                guidance={props.guidance.clone()}
                                generating={generating}
                                on_guidance={props.on_guidance.clone()}
                                on_detail={props.on_detail.clone()}
                            />
                        }
                    }
                } }
                <GuidanceBox
                    guidance={props.guidance.clone()}
                    on_guidance={props.on_guidance.clone()}
                    visible={!matches!(props.selection, StorySelection::Basics)}
                />
                if matches!(props.selection, StorySelection::Basics) {
                    <div class="story-actions">
                        <button class="btn" disabled={generating} onclick={{
                            let on_detail = props.on_detail.clone();
                            let guidance = props.guidance.clone();
                            Callback::from(move |_| {
                                let on_detail = on_detail.clone();
                                let notes = guidance.clone();
                                wasm_bindgen_futures::spawn_local(async move {
                                    if let Ok(d) = api::generate_chapter(story_id, &notes).await {
                                        on_detail.emit(d);
                                    }
                                });
                            })
                        }}>{"Generate next chapter"}</button>
                        <button class="btn secondary" onclick={{
                            let on_detail = props.on_detail.clone();
                            let on_selection = props.on_selection.clone();
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
                        }}>{"Add chapter manually"}</button>
                    </div>
                }
            </div>
        </>
    }
}

#[derive(Properties, PartialEq)]
struct GuidanceBoxProps {
    guidance: String,
    on_guidance: Callback<String>,
    visible: bool,
}

#[function_component(GuidanceBox)]
fn guidance_box(props: &GuidanceBoxProps) -> Html {
    if !props.visible {
        return html! {};
    }
    html! {
        <label class="field" style="margin-top:1rem;">
            <span class="muted">{"Guidance for next generation"}</span>
            <textarea
                placeholder="Optional notes for the AI…"
                value={props.guidance.clone()}
                rows="3"
                oninput={Callback::from({
                    let on_guidance = props.on_guidance.clone();
                    move |e: InputEvent| {
                        let input: HtmlInputElement = e.target_unchecked_into();
                        on_guidance.emit(input.value());
                    }
                })}
            />
        </label>
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
    guidance: String,
    generating: bool,
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

    html! {
        <div class="story-form">
            <h2 style="margin:0 0 1rem;font-size:1rem;">{ format!("Chapter {}", chapter.sort_order + 1) }</h2>
            <label class="field"><span class="muted">{"Title"}</span>
                <input type="text" value={(*title).clone()} oninput={string_input(title.clone())} />
            </label>
            <label class="field"><span class="muted">{"Synopsis"}</span>
                <textarea value={(*synopsis).clone()} rows="5" oninput={string_input(synopsis.clone())} />
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
                <button class="btn" disabled={props.generating} onclick={{
                    let on_detail = props.on_detail.clone();
                    let guidance = props.guidance.clone();
                    Callback::from(move |_| {
                        let on_detail = on_detail.clone();
                        let notes = guidance.clone();
                        wasm_bindgen_futures::spawn_local(async move {
                            if let Ok(d) = api::generate_beat(story_id, chapter_id, &notes).await {
                                on_detail.emit(d);
                            }
                        });
                    })
                }}>{"Generate next beat"}</button>
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
        </div>
    }
}

#[derive(Properties, PartialEq)]
struct BeatEditorProps {
    story_id: i64,
    chapter_id: i64,
    beat: Option<StoryBeat>,
    guidance: String,
    generating: bool,
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
            <h2 style="margin:0 0 1rem;font-size:1rem;">{ format!("Beat {}", beat.sort_order + 1) }</h2>
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
                <button class="btn" disabled={props.generating} onclick={{
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
