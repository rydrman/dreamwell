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
    IosManual,
    AndroidManual,
    DesktopManual,
    Unavailable,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Platform {
    Ios,
    Android,
    Desktop,
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

fn user_agent() -> Option<String> {
    web_sys::window().map(|window| window.navigator().user_agent().unwrap_or_default())
}

fn platform_from_user_agent(ua: &str) -> Platform {
    let ua = ua.to_lowercase();
    if ua.contains("iphone")
        || ua.contains("ipad")
        || ua.contains("ipod")
        || (ua.contains("macintosh") && ua.contains("mobile"))
    {
        Platform::Ios
    } else if ua.contains("android") {
        Platform::Android
    } else {
        Platform::Desktop
    }
}

fn current_platform() -> Platform {
    user_agent()
        .map(|ua| platform_from_user_agent(&ua))
        .unwrap_or(Platform::Desktop)
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

pub fn restore_hint() {
    if let Some(storage) = local_storage() {
        let _ = storage.remove_item(DISMISS_KEY);
    }
}

fn local_storage() -> Option<Storage> {
    web_sys::window().and_then(|window| window.local_storage().ok().flatten())
}

pub fn has_deferred_prompt() -> bool {
    DEFERRED_PROMPT.with(|prompt| prompt.borrow().is_some())
}

pub fn install_kind() -> InstallKind {
    if is_installed() {
        return InstallKind::Unavailable;
    }
    if has_deferred_prompt() {
        return InstallKind::NativePrompt;
    }
    match current_platform() {
        Platform::Ios => InstallKind::IosManual,
        Platform::Android => InstallKind::AndroidManual,
        Platform::Desktop => InstallKind::DesktopManual,
    }
}

pub fn banner_kind() -> Option<InstallKind> {
    if is_installed() || is_dismissed() {
        return None;
    }
    match install_kind() {
        // Chrome shows its own install promotion when beforeinstallprompt fires.
        InstallKind::NativePrompt | InstallKind::Unavailable => None,
        kind => Some(kind),
    }
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
        // Do not call preventDefault here. Chrome uses that to suppress its own
        // install banner; we want the browser-native promotion when available.
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

fn install_copy(kind: InstallKind) -> (&'static str, &'static str, bool) {
    match kind {
        InstallKind::NativePrompt => (
            "Install Dreamwell",
            "Add Dreamwell to your home screen for quick access.",
            true,
        ),
        InstallKind::IosManual => (
            "Add Dreamwell to Home Screen",
            "Open Safari's Share menu, then tap \"Add to Home Screen\".",
            false,
        ),
        InstallKind::AndroidManual => (
            "Install Dreamwell",
            "Open Chrome's menu and choose \"Install app\" or \"Add to Home screen\".",
            false,
        ),
        InstallKind::DesktopManual => (
            "Install Dreamwell",
            "Use the install icon in the address bar, or open the browser menu and choose \"Install Dreamwell\".",
            false,
        ),
        InstallKind::Unavailable => ("", "", false),
    }
}

fn manual_steps(kind: InstallKind) -> Html {
    match kind {
        InstallKind::IosManual => html! {
            <ol class="install-steps">
                <li>{"Open this page in Safari if you are using another browser."}</li>
                <li>{"Tap the Share button at the bottom of the screen."}</li>
                <li>{"Scroll down and tap \"Add to Home Screen\"."}</li>
                <li>{"Tap \"Add\" to confirm."}</li>
            </ol>
        },
        InstallKind::AndroidManual => html! {
            <ol class="install-steps">
                <li>{"Open this page in Chrome if you are using another browser."}</li>
                <li>{"Tap the menu button (three dots) in the top-right corner."}</li>
                <li>{"Choose \"Install app\" or \"Add to Home screen\"."}</li>
                <li>{"Confirm the install when prompted."}</li>
            </ol>
        },
        InstallKind::DesktopManual => html! {
            <ol class="install-steps">
                <li>{"Look for the install icon in Chrome's address bar."}</li>
                <li>{"If it is not there, open the browser menu (three dots)."}</li>
                <li>{"Choose \"Install Dreamwell\" or \"Save and share\" → \"Install page as app\"."}</li>
            </ol>
        },
        InstallKind::NativePrompt | InstallKind::Unavailable => html! {},
    }
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

    let (title, body, show_install) = install_copy(props.kind);
    if matches!(props.kind, InstallKind::Unavailable) {
        return html! {};
    }

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

#[derive(Properties, PartialEq)]
pub struct InstallSettingsProps {
    pub kind: InstallKind,
    pub dismissed: bool,
    pub on_change: Callback<()>,
}

#[function_component(InstallSettings)]
pub fn install_settings(props: &InstallSettingsProps) -> Html {
    let kind = props.kind;

    let restore = {
        let on_change = props.on_change.clone();
        Callback::from(move |_| {
            restore_hint();
            on_change.emit(());
        })
    };

    html! {
        <div class="settings-group">
            <strong>{"Install app"}</strong>
            if is_installed() {
                <p class="muted" style="margin:0.35rem 0 0;">
                    {"Dreamwell is running as an installed app."}
                </p>
            } else if matches!(kind, InstallKind::NativePrompt) {
                <p class="muted" style="margin:0.35rem 0 0.5rem;">
                    {"Chrome should offer to install Dreamwell automatically. If you dismissed that prompt, use the button below or Chrome's menu → \"Install Dreamwell\"."}
                </p>
                <InstallActions kind={InstallKind::NativePrompt} on_change={props.on_change.clone()} />
            } else {
                <p class="muted" style="margin:0.35rem 0 0.5rem;">
                    { manual_intro(kind) }
                </p>
                { manual_steps(kind) }
            }
            if props.dismissed && !is_installed() {
                <p class="muted" style="margin:0.75rem 0 0;">
                    {"The install banner is hidden on this device."}
                </p>
                <button class="btn secondary btn-compact" style="margin-top:0.5rem;" onclick={restore}>
                    {"Show install banner again"}
                </button>
            }
        </div>
    }
}

fn manual_intro(kind: InstallKind) -> &'static str {
    match kind {
        InstallKind::IosManual => {
            "iPhone and iPad do not show an automatic install button. Add Dreamwell manually:"
        }
        InstallKind::AndroidManual => {
            "If Chrome does not show an install offer at the top of the page, add Dreamwell manually:"
        }
        InstallKind::DesktopManual => "Install Dreamwell as a desktop app from Chrome or Edge:",
        InstallKind::NativePrompt | InstallKind::Unavailable => "",
    }
}

#[derive(Properties, PartialEq)]
struct InstallActionsProps {
    kind: InstallKind,
    on_change: Callback<()>,
}

#[function_component(InstallActions)]
fn install_actions(props: &InstallActionsProps) -> Html {
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

    if !matches!(props.kind, InstallKind::NativePrompt) {
        return html! {};
    }

    html! {
        <button class="btn primary btn-compact" disabled={*busy} onclick={on_install}>
            { if *busy { "Installing…" } else { "Install Dreamwell" } }
        </button>
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_ios_devices_including_chrome() {
        assert!(matches!(
            platform_from_user_agent(
                "Mozilla/5.0 (iPhone; CPU iPhone OS 17_0 like Mac OS X) AppleWebKit/605.1.15 (KHTML, like Gecko) CriOS/120.0.0.0 Mobile/15E148 Safari/604.1"
            ),
            Platform::Ios
        ));
    }

    #[test]
    fn detects_ipad_desktop_ua() {
        assert!(matches!(
            platform_from_user_agent(
                "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/605.1.15 (KHTML, like Gecko) Version/17.0 Mobile/15E148 Safari/604.1"
            ),
            Platform::Ios
        ));
    }

    #[test]
    fn detects_android_devices() {
        assert!(matches!(
            platform_from_user_agent(
                "Mozilla/5.0 (Linux; Android 14; Pixel 8) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/120.0.0.0 Mobile Safari/537.36"
            ),
            Platform::Android
        ));
    }
}
