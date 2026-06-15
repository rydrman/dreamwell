use wasm_bindgen::JsCast;
use web_sys::{Element, HtmlTextAreaElement};
use yew::prelude::*;

/// Floor height so empty single-line fields are still comfortably clickable.
const MIN_TEXTAREA_HEIGHT_PX: i32 = 40;

/// Retry until Yew attaches the ref (first paint often runs effects too early).
const MAX_ATTACH_ATTEMPTS: u32 = 12;

/// Resize a textarea to fit its content without internal scrolling.
pub fn fit_textarea(textarea: &HtmlTextAreaElement) {
    let style = textarea.style();
    // Reset height so scrollHeight reflects the full value (including autosave padding).
    let _ = style.set_property("height", "auto");
    let height = textarea.scroll_height().max(MIN_TEXTAREA_HEIGHT_PX);
    let _ = style.set_property("height", &format!("{height}px"));
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

fn schedule_fit_passes(textarea: HtmlTextAreaElement) {
    gloo_timers::callback::Timeout::new(0, {
        let textarea = textarea.clone();
        move || {
            fit_textarea(&textarea);
            let textarea = textarea.clone();
            gloo_timers::callback::Timeout::new(0, move || {
                fit_textarea(&textarea);
                let textarea = textarea.clone();
                gloo_timers::callback::Timeout::new(16, move || fit_textarea(&textarea)).forget();
            })
            .forget();
        }
    })
    .forget();
}

/// After layout, resize `textarea` (deferred for Yew value commits).
pub fn fit_textarea_when_ready(textarea: HtmlTextAreaElement) {
    schedule_fit_passes(textarea);
}

pub fn fit_textareas_in_when_ready(root: Element) {
    gloo_timers::callback::Timeout::new(0, move || {
        fit_textareas_in(&root);
        let root = root.clone();
        gloo_timers::callback::Timeout::new(0, move || {
            fit_textareas_in(&root);
            let root = root.clone();
            gloo_timers::callback::Timeout::new(16, move || fit_textareas_in(&root)).forget();
        })
        .forget();
    })
    .forget();
}

/// Fit once `root_ref` is attached, retrying across early frames.
pub fn fit_when_root_attached(root_ref: NodeRef) {
    schedule_fit_root(root_ref, 0);
}

fn schedule_fit_root(root_ref: NodeRef, attempt: u32) {
    let delay_ms = if attempt == 0 { 0 } else { 16 };
    gloo_timers::callback::Timeout::new(delay_ms, move || {
        if let Some(root) = root_ref.cast::<Element>() {
            fit_textareas_in(&root);
            fit_textareas_in_when_ready(root);
        } else if attempt < MAX_ATTACH_ATTEMPTS {
            schedule_fit_root(root_ref, attempt + 1);
        }
    })
    .forget();
}

/// Fit once `textarea_ref` is attached, retrying across early frames.
pub fn fit_when_textarea_attached(textarea_ref: NodeRef) {
    schedule_fit_textarea(textarea_ref, 0);
}

fn schedule_fit_textarea(textarea_ref: NodeRef, attempt: u32) {
    let delay_ms = if attempt == 0 { 0 } else { 16 };
    gloo_timers::callback::Timeout::new(delay_ms, move || {
        if let Some(textarea) = textarea_ref.cast::<HtmlTextAreaElement>() {
            fit_textarea(&textarea);
            fit_textarea_when_ready(textarea);
        } else if attempt < MAX_ATTACH_ATTEMPTS {
            schedule_fit_textarea(textarea_ref, attempt + 1);
        }
    })
    .forget();
}

/// Bubble `oninput` handler for a container that holds auto-growing textareas.
pub fn container_input_callback() -> Callback<InputEvent> {
    Callback::from(|event: InputEvent| {
        if let Ok(textarea) = event.target().unwrap().dyn_into::<HtmlTextAreaElement>() {
            fit_textarea(&textarea);
        }
    })
}

/// After layout, resize every textarea under `root`.
#[macro_export]
macro_rules! use_fit_textareas_in {
    ($root_ref:expr, $trigger:expr) => {{
        let root_ref = ($root_ref).clone();
        yew::functional::use_effect_with($trigger, move |_| {
            $crate::auto_grow::fit_when_root_attached(root_ref);
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
            $crate::auto_grow::fit_when_textarea_attached(textarea_ref);
            || ()
        });
    }};
}
