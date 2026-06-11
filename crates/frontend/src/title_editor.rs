use web_sys::HtmlInputElement;
use yew::prelude::*;

#[derive(Clone, Copy, PartialEq)]
pub enum TitleEditTrigger {
    Click,
    Button,
}

#[derive(Properties, PartialEq)]
pub struct TitleEditorProps {
    pub title: String,
    pub class: &'static str,
    pub placeholder: &'static str,
    pub on_save: Callback<String>,
    #[prop_or(TitleEditTrigger::Click)]
    pub trigger: TitleEditTrigger,
}

#[function_component(TitleEditor)]
pub fn title_editor(props: &TitleEditorProps) -> Html {
    let editing = use_state(|| false);
    let draft = use_state(|| props.title.clone());
    let input_ref = use_node_ref();

    {
        let draft = draft.clone();
        let editing = editing.clone();
        let title = props.title.clone();
        use_effect_with(title.clone(), move |_| {
            draft.set(title);
            editing.set(false);
            || ()
        });
    }

    {
        let input_ref = input_ref.clone();
        let editing = *editing;
        use_effect_with(editing, move |editing| {
            if *editing {
                if let Some(input) = input_ref.cast::<HtmlInputElement>() {
                    let _ = input.focus();
                    input.select();
                }
            }
            || ()
        });
    }

    if *editing {
        html! {
            <input
                ref={input_ref}
                class={props.class}
                type="text"
                value={(*draft).clone()}
                placeholder={props.placeholder}
                onclick={Callback::from(|e: MouseEvent| e.stop_propagation())}
                oninput={{
                    let draft = draft.clone();
                    Callback::from(move |e: InputEvent| {
                        let input: HtmlInputElement = e.target_unchecked_into();
                        draft.set(input.value());
                    })
                }}
                onkeydown={{
                    let editing = editing.clone();
                    let draft = draft.clone();
                    let on_save = props.on_save.clone();
                    let title = props.title.clone();
                    Callback::from(move |e: KeyboardEvent| {
                        if e.key() == "Enter" {
                            e.prevent_default();
                            let trimmed = draft.trim().to_string();
                            if !trimmed.is_empty() && trimmed != title {
                                on_save.emit(trimmed);
                            }
                            editing.set(false);
                        } else if e.key() == "Escape" {
                            editing.set(false);
                            draft.set(title.clone());
                        }
                    })
                }}
                onblur={{
                    let editing = editing.clone();
                    let draft = draft.clone();
                    let on_save = props.on_save.clone();
                    let title = props.title.clone();
                    Callback::from(move |_| {
                        let trimmed = draft.trim().to_string();
                        if !trimmed.is_empty() && trimmed != title {
                            on_save.emit(trimmed);
                        }
                        editing.set(false);
                    })
                }}
            />
        }
    } else if props.trigger == TitleEditTrigger::Button {
        html! {
            <div class={classes!("title-editor-row", props.class)}>
                <span class="title-editor-text">{ &props.title }</span>
                <button
                    type="button"
                    class="btn secondary btn-compact title-edit-btn"
                    title="Rename"
                    onclick={Callback::from({
                        let editing = editing.clone();
                        move |e: MouseEvent| {
                            e.stop_propagation();
                            editing.set(true);
                        }
                    })}
                >
                    {"✎"}
                </button>
            </div>
        }
    } else {
        html! {
            <div
                class={classes!(props.class, "title-editable")}
                title="Click to rename"
                onclick={Callback::from({
                    let editing = editing.clone();
                    move |e: MouseEvent| {
                        e.stop_propagation();
                        editing.set(true);
                    }
                })}
            >
                { &props.title }
            </div>
        }
    }
}
