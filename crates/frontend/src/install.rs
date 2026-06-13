use std::cell::RefCell;
use std::rc::Rc;

use js_sys::{Function, Promise, Reflect};
use wasm_bindgen::closure::Closure;
use wasm_bindgen::JsCast;
use wasm_bindgen::JsValue;
use wasm_bindgen_futures::JsFuture;
use yew::prelude::*;

type InstallPromptListener = Closure<dyn FnMut(web_sys::Event)>;
type AppInstalledListener = Closure<dyn FnMut(web_sys::Event)>;

thread_local! {
    static DEFERRED_PROMPT: RefCell<Option<JsValue>> = const { RefCell::new(None) };
    static INSTALL_LISTENER: RefCell<Option<InstallPromptListener>> = const { RefCell::new(None) };
    static APP_INSTALLED_LISTENER: RefCell<Option<AppInstalledListener>> = const { RefCell::new(None) };
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum InstallKind {
    NativePrompt,
    Unavailable,
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
    Reflect::get(&window.navigator(), &JsValue::from_str("standalone"))
        .ok()
        .and_then(|value| value.as_bool())
        .unwrap_or(false)
}

pub fn has_deferred_prompt() -> bool {
    DEFERRED_PROMPT.with(|prompt| prompt.borrow().is_some())
}

pub fn install_kind() -> InstallKind {
    if is_installed() {
        InstallKind::Unavailable
    } else if has_deferred_prompt() {
        InstallKind::NativePrompt
    } else {
        InstallKind::Unavailable
    }
}

pub fn init(on_change: Callback<()>) {
    if is_installed() {
        return;
    }

    let Some(window) = web_sys::window() else {
        return;
    };

    let on_change = Rc::new(on_change);
    let install_closure = Closure::wrap(Box::new({
        let on_change = on_change.clone();
        move |event: web_sys::Event| {
            event.prevent_default();
            DEFERRED_PROMPT.with(|prompt| *prompt.borrow_mut() = Some(event.into()));
            on_change.emit(());
        }
    }) as Box<dyn FnMut(_)>);

    let _ = window.add_event_listener_with_callback(
        "beforeinstallprompt",
        install_closure.as_ref().unchecked_ref(),
    );
    INSTALL_LISTENER.with(|listener| *listener.borrow_mut() = Some(install_closure));

    let installed_closure = Closure::wrap(Box::new(move |_event: web_sys::Event| {
        DEFERRED_PROMPT.with(|prompt| *prompt.borrow_mut() = None);
        on_change.emit(());
    }) as Box<dyn FnMut(_)>);

    let _ = window.add_event_listener_with_callback(
        "appinstalled",
        installed_closure.as_ref().unchecked_ref(),
    );
    APP_INSTALLED_LISTENER.with(|listener| *listener.borrow_mut() = Some(installed_closure));
}

async fn call_method(target: &JsValue, method: &str) -> Result<JsValue, JsValue> {
    let function = Reflect::get(target, &JsValue::from_str(method))?
        .dyn_into::<Function>()
        .map_err(|_| JsValue::from_str("expected function"))?;
    function.call0(target)
}

pub async fn trigger_install() -> bool {
    let prompt = DEFERRED_PROMPT.with(|deferred| deferred.borrow_mut().take());
    let Some(prompt) = prompt else {
        return false;
    };

    let Ok(prompt_result) = call_method(&prompt, "prompt").await else {
        return false;
    };
    if JsFuture::from(
        prompt_result
            .dyn_into::<Promise>()
            .unwrap_or_else(|_| Promise::resolve(&JsValue::NULL)),
    )
    .await
    .is_err()
    {
        return false;
    }

    let Ok(choice_result) = Reflect::get(&prompt, &JsValue::from_str("userChoice")) else {
        return true;
    };
    let Ok(choice_promise) = choice_result.dyn_into::<Promise>() else {
        return true;
    };
    let _ = JsFuture::from(choice_promise).await;
    true
}

#[derive(Properties, PartialEq)]
pub struct InstallButtonProps {
    pub on_change: Callback<()>,
}

#[function_component(InstallButton)]
pub fn install_button(props: &InstallButtonProps) -> Html {
    let busy = use_state(|| false);

    let on_install = {
        let busy = busy.clone();
        let on_change = props.on_change.clone();
        Callback::from(move |_| {
            if *busy {
                return;
            }
            busy.set(true);
            let busy = busy.clone();
            let on_change = on_change.clone();
            wasm_bindgen_futures::spawn_local(async move {
                let _ = trigger_install().await;
                on_change.emit(());
                busy.set(false);
            });
        })
    };

    html! {
        <button
            type="button"
            class="mode-btn mode-btn-icon"
            title="Install Dreamwell"
            aria-label="Install Dreamwell"
            disabled={*busy}
            onclick={on_install}
        >
            <span class="mode-btn-icon-glyph">{ if *busy { "…" } else { "⬇" } }</span>
        </button>
    }
}

#[derive(Properties, PartialEq)]
pub struct InstallSettingsProps {
    pub kind: InstallKind,
    pub on_change: Callback<()>,
}

#[function_component(InstallSettings)]
pub fn install_settings(props: &InstallSettingsProps) -> Html {
    if is_installed() {
        return html! {
            <div class="settings-group">
                <strong>{"Install app"}</strong>
                <p class="muted" style="margin:0.35rem 0 0;">
                    {"Dreamwell is running as an installed app."}
                </p>
            </div>
        };
    }

    if !matches!(props.kind, InstallKind::NativePrompt) {
        return html! {};
    }

    html! {
        <div class="settings-group">
            <strong>{"Install app"}</strong>
            <p class="muted" style="margin:0.35rem 0 0.5rem;">
                {"Install Dreamwell on this device for quick access from your home screen."}
            </p>
            <InstallSettingsButton on_change={props.on_change.clone()} />
        </div>
    }
}

#[derive(Properties, PartialEq)]
struct InstallSettingsButtonProps {
    on_change: Callback<()>,
}

#[function_component(InstallSettingsButton)]
fn install_settings_button(props: &InstallSettingsButtonProps) -> Html {
    let busy = use_state(|| false);

    let on_install = {
        let busy = busy.clone();
        let on_change = props.on_change.clone();
        Callback::from(move |_| {
            if *busy {
                return;
            }
            busy.set(true);
            let busy = busy.clone();
            let on_change = on_change.clone();
            wasm_bindgen_futures::spawn_local(async move {
                let _ = trigger_install().await;
                on_change.emit(());
                busy.set(false);
            });
        })
    };

    html! {
        <button class="btn primary btn-compact" disabled={*busy} onclick={on_install}>
            { if *busy { "Installing…" } else { "Install Dreamwell" } }
        </button>
    }
}
