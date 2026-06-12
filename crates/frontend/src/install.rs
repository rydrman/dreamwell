use std::cell::RefCell;
use std::rc::Rc;

use js_sys::{Function, Promise, Reflect};
use wasm_bindgen::closure::Closure;
use wasm_bindgen::JsCast;
use wasm_bindgen::JsValue;
use wasm_bindgen_futures::JsFuture;
use web_sys::Storage;
use yew::prelude::*;

const DISMISS_KEY: &str = "dreamwell.install.dismissed";

type InstallPromptListener = Closure<dyn FnMut(web_sys::Event)>;

thread_local! {
    static DEFERRED_PROMPT: RefCell<Option<JsValue>> = const { RefCell::new(None) };
    static INSTALL_LISTENER: RefCell<Option<InstallPromptListener>> = const { RefCell::new(None) };
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum InstallKind {
    NativePrompt,
    IosHint,
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

pub fn is_ios_browser() -> bool {
    user_agent()
        .map(|ua| {
            let ua = ua.to_lowercase();
            (ua.contains("iphone") || ua.contains("ipad") || ua.contains("ipod"))
                && ua.contains("safari")
                && !ua.contains("crios")
                && !ua.contains("fxios")
        })
        .unwrap_or(false)
}

fn user_agent() -> Option<String> {
    web_sys::window().map(|window| window.navigator().user_agent().unwrap_or_default())
}

fn is_mobile_viewport() -> bool {
    web_sys::window()
        .and_then(|window| window.match_media("(max-width: 768px)").ok().flatten())
        .map(|mq| mq.matches())
        .unwrap_or(false)
}

pub fn is_dismissed() -> bool {
    local_storage()
        .and_then(|storage| storage.get_item(DISMISS_KEY).ok().flatten())
        .is_some()
}

pub fn dismiss_hint() {
    if let Some(storage) = local_storage() {
        let _ = storage.set_item(DISMISS_KEY, "1");
    }
}

fn local_storage() -> Option<Storage> {
    web_sys::window().and_then(|window| window.local_storage().ok().flatten())
}

pub fn has_deferred_prompt() -> bool {
    DEFERRED_PROMPT.with(|prompt| prompt.borrow().is_some())
}

pub fn install_kind() -> InstallKind {
    if is_installed() || is_dismissed() {
        return InstallKind::Unavailable;
    }
    if has_deferred_prompt() {
        return InstallKind::NativePrompt;
    }
    if is_ios_browser() && is_mobile_viewport() {
        return InstallKind::IosHint;
    }
    InstallKind::Unavailable
}

pub fn init(on_available: Callback<()>) {
    if is_installed() {
        return;
    }

    let Some(window) = web_sys::window() else {
        return;
    };

    let on_available = Rc::new(on_available);
    let closure = Closure::wrap(Box::new(move |event: web_sys::Event| {
        event.prevent_default();
        DEFERRED_PROMPT.with(|prompt| *prompt.borrow_mut() = Some(event.into()));
        on_available.emit(());
    }) as Box<dyn FnMut(_)>);

    let _ = window
        .add_event_listener_with_callback("beforeinstallprompt", closure.as_ref().unchecked_ref());

    INSTALL_LISTENER.with(|listener| *listener.borrow_mut() = Some(closure));
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
pub struct InstallBannerProps {
    pub kind: InstallKind,
    pub on_dismiss: Callback<()>,
    pub on_installed: Callback<()>,
}

#[function_component(InstallBanner)]
pub fn install_banner(props: &InstallBannerProps) -> Html {
    let busy = use_state(|| false);

    let on_install = {
        let busy = busy.clone();
        let on_installed = props.on_installed.clone();
        Callback::from(move |_| {
            if *busy {
                return;
            }
            busy.set(true);
            let busy = busy.clone();
            let on_installed = on_installed.clone();
            wasm_bindgen_futures::spawn_local(async move {
                if trigger_install().await {
                    on_installed.emit(());
                }
                busy.set(false);
            });
        })
    };

    let (title, body, show_install) = match props.kind {
        InstallKind::NativePrompt => (
            "Install Dreamwell",
            "Add Dreamwell to your home screen for quick access.",
            true,
        ),
        InstallKind::IosHint => (
            "Add Dreamwell to Home Screen",
            "Tap Share, then \"Add to Home Screen\".",
            false,
        ),
        InstallKind::Unavailable => return html! {},
    };

    html! {
        <div class="install-banner" role="region" aria-label="Install Dreamwell">
            <div class="install-banner-copy">
                <strong>{ title }</strong>
                <p>{ body }</p>
            </div>
            <div class="install-banner-actions">
                if show_install {
                    <button
                        class="btn primary btn-compact"
                        disabled={*busy}
                        onclick={on_install}
                    >
                        { if *busy { "Installing…" } else { "Install" } }
                    </button>
                }
                <button
                    class="btn secondary btn-compact"
                    onclick={props.on_dismiss.reform(|_| ())}
                >
                    {"Not now"}
                </button>
            </div>
        </div>
    }
}

#[function_component(InstallSettings)]
pub fn install_settings() -> Html {
    let kind = install_kind();

    html! {
        <div class="settings-group">
            <strong>{"Install app"}</strong>
            if is_installed() {
                <p class="muted" style="margin:0.35rem 0 0;">
                    {"Dreamwell is running as an installed app."}
                </p>
            } else if matches!(kind, InstallKind::NativePrompt) {
                <p class="muted" style="margin:0.35rem 0 0.5rem;">
                    {"Install Dreamwell on this device for quick access from your home screen."}
                </p>
                <InstallActions kind={InstallKind::NativePrompt} />
            } else if matches!(kind, InstallKind::IosHint) {
                <p class="muted" style="margin:0.35rem 0 0.5rem;">
                    {"On iPhone or iPad, use Safari's Share menu and choose \"Add to Home Screen\"."}
                </p>
            } else {
                <p class="muted" style="margin:0.35rem 0 0;">
                    {"Install is available on supported mobile browsers after you have used the site for a little while."}
                </p>
            }
        </div>
    }
}

#[function_component(InstallActions)]
fn install_actions(props: &InstallActionsProps) -> Html {
    let busy = use_state(|| false);

    let on_install = {
        let busy = busy.clone();
        Callback::from(move |_| {
            if *busy {
                return;
            }
            busy.set(true);
            let busy = busy.clone();
            wasm_bindgen_futures::spawn_local(async move {
                let _ = trigger_install().await;
                busy.set(false);
            });
        })
    };

    if !matches!(props.kind, InstallKind::NativePrompt) {
        return html! {};
    }

    html! {
        <button class="btn primary btn-compact" disabled={*busy} onclick={on_install}>
            { if *busy { "Installing…" } else { "Install Dreamwell" } }
        </button>
    }
}

#[derive(Properties, PartialEq)]
struct InstallActionsProps {
    kind: InstallKind,
}
