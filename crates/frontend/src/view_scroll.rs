use crate::queue_ui::AppMode;
use wasm_bindgen::JsCast;
use web_sys::HtmlElement;

pub fn is_mobile_viewport() -> bool {
    web_sys::window()
        .and_then(|window| window.match_media("(max-width: 768px)").ok().flatten())
        .map(|mq| mq.matches())
        .unwrap_or(false)
}

pub fn window_scroll_y() -> f64 {
    web_sys::window()
        .and_then(|window| window.scroll_y().ok())
        .unwrap_or(0.0)
}

pub fn mobile_scroll_chrome_active(
    mode: AppMode,
    chat_id: Option<i64>,
    story_id: Option<i64>,
    game_id: Option<i64>,
) -> bool {
    match mode {
        AppMode::Chats => chat_id.is_some(),
        AppMode::Stories => story_id.is_some(),
        AppMode::Game => game_id.is_some(),
        _ => false,
    }
}

pub fn update_mobile_chrome_inset(
    layout: &HtmlElement,
    mode: AppMode,
    scroll_chrome: bool,
    chrome_visible: bool,
    scroll_y: f64,
) {
    if !is_mobile_viewport() {
        let _ = layout.style().remove_property("--chrome-inset-top");
        let _ = layout.style().remove_property("--content-header-height");
        return;
    }

    let header_height = measure_element_height(".main .content-header");
    let topbar_height = measure_element_height(".mode-bar");
    let _ = layout
        .style()
        .set_property("--content-header-height", &format!("{header_height}px"));

    let inset = if scroll_chrome {
        if chrome_visible {
            topbar_height + header_height
        } else {
            0.0
        }
    } else {
        match mode {
            AppMode::Stories
            | AppMode::Game
            | AppMode::Queue
            | AppMode::Settings
            | AppMode::Characters
            | AppMode::Scenarios
                if scroll_y > 0.0 =>
            {
                header_height
            }
            AppMode::Stories
            | AppMode::Game
            | AppMode::Queue
            | AppMode::Settings
            | AppMode::Characters
            | AppMode::Scenarios
            | AppMode::Chats => topbar_height + header_height,
        }
    };

    let _ = layout
        .style()
        .set_property("--chrome-inset-top", &format!("{inset}px"));
}

pub fn scroll_content_view_to_bottom(scroll_el: Option<&HtmlElement>) {
    let Some(window) = web_sys::window() else {
        return;
    };
    if is_mobile_viewport() {
        let height = window
            .document()
            .map(|document| {
                document
                    .document_element()
                    .map(|root| root.scroll_height())
                    .or_else(|| document.body().map(|body| body.scroll_height()))
                    .unwrap_or(0)
            })
            .unwrap_or(0) as f64;
        window.scroll_to_with_x_and_y(0.0, height);
    } else if let Some(el) = scroll_el {
        el.set_scroll_top(el.scroll_height());
    }
}

fn measure_element_height(selector: &str) -> f64 {
    web_sys::window()
        .and_then(|window| window.document())
        .and_then(|document| document.query_selector(selector).ok().flatten())
        .and_then(|element| element.dyn_into::<HtmlElement>().ok())
        .map(|element| element.offset_height() as f64)
        .unwrap_or(0.0)
}
