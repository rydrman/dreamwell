use wasm_bindgen::JsCast;
use web_sys::{Element, HtmlTextAreaElement};
use yew::prelude::*;

/// Resize a textarea to fit its content without internal scrolling.
pub fn fit_textarea(textarea: &HtmlTextAreaElement) {
    let style = textarea.style();
    let _ = style.set_property("height", "auto");
    let _ = style.set_property("height", &format!("{}px", textarea.scroll_height()));
}

/// Resize every textarea under `root`.
pub fn fit_textareas_in(root: &Element) {
    if let Ok(node_list) = root.query_selector_all("textarea") {
        for index in 0..node_list.length() {
            if let Some(node) = node_list.item(index) {
                if let Ok(textarea) = node.dyn_into::<HtmlTextAreaElement>() {
                    fit_textarea(&textarea);
                }
            }
        }
    }
}

pub fn fit_textareas_in_deferred(root: Element) {
    gloo_timers::callback::Timeout::new(0, move || fit_textareas_in(&root)).forget();
}

/// Bubble `oninput` handler for a container that holds auto-growing textareas.
pub fn container_input_callback() -> Callback<InputEvent> {
    Callback::from(|event: InputEvent| {
        if let Ok(textarea) = event.target().unwrap().dyn_into::<HtmlTextAreaElement>() {
            fit_textarea(&textarea);
        }
    })
}

/// After render, resize every textarea under `root`.
#[macro_export]
macro_rules! use_fit_textareas_in {
    ($root_ref:expr, $trigger:expr) => {{
        let root_ref = ($root_ref).clone();
        yew::functional::use_effect_with($trigger, move |_| {
            if let Some(root) = root_ref.cast::<web_sys::Element>() {
                $crate::auto_grow::fit_textareas_in_deferred(root);
            }
            || ()
        });
    }};
}

/// Resize a single textarea when its value changes (e.g. streaming generation).
#[macro_export]
macro_rules! use_fit_textarea {
    ($textarea_ref:expr, $value:expr) => {{
        let textarea_ref = ($textarea_ref).clone();
        yew::functional::use_effect_with($value, move |_| {
            if let Some(textarea) = textarea_ref.cast::<web_sys::HtmlTextAreaElement>() {
                let textarea = textarea.clone();
                gloo_timers::callback::Timeout::new(0, move || {
                    $crate::auto_grow::fit_textarea(&textarea);
                })
                .forget();
            }
            || ()
        });
    }};
}
