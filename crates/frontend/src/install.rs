use std::cell::RefCell;
use std::rc::Rc;

use wasm_bindgen::closure::Closure;
use wasm_bindgen::JsCast;
use yew::prelude::*;

type AppInstalledListener = Closure<dyn FnMut(web_sys::Event)>;

thread_local! {
    static APP_INSTALLED_LISTENER: RefCell<Option<AppInstalledListener>> = const { RefCell::new(None) };
}

pub fn is_installed() -> bool {
    is_display_standalone() || is_ios_standalone()
}

fn is_display_standalone() -> bool {
    let Some(window) = web_sys::window() else {
        return false;
    };
    ["standalone", "fullscreen", "minimal-ui"]
        .into_iter()
        .any(|mode| {
            window
                .match_media(&format!("(display-mode: {mode})"))
                .ok()
                .flatten()
                .map(|mq| mq.matches())
                .unwrap_or(false)
        })
}

fn is_ios_standalone() -> bool {
    let Some(window) = web_sys::window() else {
        return false;
    };
    js_sys::Reflect::get(
        &window.navigator(),
        &wasm_bindgen::JsValue::from_str("standalone"),
    )
    .ok()
    .and_then(|value| value.as_bool())
    .unwrap_or(false)
}

/// Listen for the browser finishing an install so UI can refresh.
///
/// Dreamwell does not intercept `beforeinstallprompt`. Browsers that support
/// installable PWAs show their own promotion UI when the app is eligible.
pub fn init(on_installed: Callback<()>) {
    if is_installed() {
        return;
    }

    let Some(window) = web_sys::window() else {
        return;
    };

    let on_installed = Rc::new(on_installed);
    let closure = Closure::wrap(Box::new(move |_event: web_sys::Event| {
        on_installed.emit(());
    }) as Box<dyn FnMut(_)>);

    let _ =
        window.add_event_listener_with_callback("appinstalled", closure.as_ref().unchecked_ref());
    APP_INSTALLED_LISTENER.with(|listener| *listener.borrow_mut() = Some(closure));
}

#[function_component(InstallSettings)]
pub fn install_settings() -> Html {
    if !is_installed() {
        return html! {};
    }

    html! {
        <div class="settings-group">
            <strong>{"Install app"}</strong>
            <p class="muted" style="margin:0.35rem 0 0;">
                {"Dreamwell is running as an installed app."}
            </p>
        </div>
    }
}
