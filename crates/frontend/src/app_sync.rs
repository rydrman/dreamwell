//! Central coordinator for tab lifecycle, background pausing, and resume reconciliation.

use std::cell::RefCell;
use std::rc::Rc;

use gloo_timers::callback::Interval;
use wasm_bindgen::closure::Closure;
use wasm_bindgen::JsCast;
use web_sys::{Event, PageTransitionEvent, VisibilityState};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ResumeReason {
    Visibility,
    Online,
    PageShow,
}

type ReconcileFn = Rc<dyn Fn(ResumeReason)>;
type PauseFn = Rc<dyn Fn()>;
type PollTickFn = Rc<dyn Fn()>;

thread_local! {
    static REGISTRY: RefCell<Registry> = RefCell::new(Registry::default());
    static POLL_TICK: RefCell<Option<PollTickFn>> = const { RefCell::new(None) };
}

#[derive(Default)]
struct Registry {
    next_id: usize,
    scopes: Vec<Scope>,
}

struct Scope {
    id: usize,
    reconcile: ReconcileFn,
    pause: PauseFn,
}

impl Registry {
    fn register(&mut self, reconcile: ReconcileFn, pause: PauseFn) -> usize {
        let id = self.next_id;
        self.next_id += 1;
        self.scopes.push(Scope {
            id,
            reconcile,
            pause,
        });
        id
    }

    fn unregister(&mut self, id: usize) {
        self.scopes.retain(|scope| scope.id != id);
    }
}

/// Guard returned from [`register_scope`]; unregisters on drop.
pub struct ScopeGuard {
    id: usize,
}

impl Drop for ScopeGuard {
    fn drop(&mut self) {
        REGISTRY.with(|registry| registry.borrow_mut().unregister(self.id));
    }
}

/// Register reconcile and pause handlers for a part of the app (e.g. chats shell, stories shell).
pub fn register_scope(
    on_reconcile: impl Fn(ResumeReason) + 'static,
    on_pause: impl Fn() + 'static,
) -> ScopeGuard {
    let id = REGISTRY.with(|registry| {
        registry
            .borrow_mut()
            .register(Rc::new(on_reconcile), Rc::new(on_pause))
    });
    ScopeGuard { id }
}

/// Whether the tab is visible and the browser reports an online network state.
pub fn tab_active() -> bool {
    let visible = web_sys::window()
        .and_then(|window| window.document())
        .is_some_and(|document| document.visibility_state() == VisibilityState::Visible);
    let online = web_sys::window()
        .map(|window| window.navigator().on_line())
        .unwrap_or(true);
    visible && online
}

fn document_was_discarded() -> bool {
    let Some(document) = web_sys::window().and_then(|window| window.document()) else {
        return false;
    };
    js_sys::Reflect::get(&document, &"wasDiscarded".into())
        .ok()
        .and_then(|value| value.dyn_into::<js_sys::Function>().ok())
        .and_then(|func| func.call0(&document).ok())
        .and_then(|value| value.as_bool())
        .unwrap_or(false)
}

/// Run all registered reconcile handlers and trigger an immediate poll tick.
pub fn reconcile(reason: ResumeReason) {
    let _force = reason == ResumeReason::PageShow || document_was_discarded();

    REGISTRY.with(|registry| {
        for scope in &registry.borrow().scopes {
            (scope.reconcile)(reason);
        }
    });

    POLL_TICK.with(|tick| {
        if let Some(tick) = tick.borrow().as_ref() {
            tick();
        }
    });
}

/// Pause background sync for all registered scopes (e.g. when the tab is hidden).
pub fn pause_all() {
    REGISTRY.with(|registry| {
        for scope in &registry.borrow().scopes {
            (scope.pause)();
        }
    });
}

/// Install document/window lifecycle listeners. Returns a cleanup closure for the effect.
pub fn install_lifecycle() -> impl FnOnce() {
    let visibility_callback = Closure::wrap(Box::new(move |_event: Event| {
        if tab_active() {
            reconcile(ResumeReason::Visibility);
        } else {
            pause_all();
        }
    }) as Box<dyn FnMut(_)>);

    let online_callback = Closure::wrap(Box::new(move |_event: Event| {
        if tab_active() {
            reconcile(ResumeReason::Online);
        }
    }) as Box<dyn FnMut(_)>);

    let offline_callback = Closure::wrap(Box::new(move |_event: Event| {
        pause_all();
    }) as Box<dyn FnMut(_)>);

    let pageshow_callback = Closure::wrap(Box::new(move |event: PageTransitionEvent| {
        if event.persisted() && tab_active() {
            reconcile(ResumeReason::PageShow);
        }
    }) as Box<dyn FnMut(_)>);

    let document = web_sys::window().and_then(|window| window.document());
    if let Some(document) = document.as_ref() {
        let _ = document.add_event_listener_with_callback(
            "visibilitychange",
            visibility_callback.as_ref().unchecked_ref(),
        );
    }

    let window = web_sys::window();
    if let Some(window) = window.as_ref() {
        let _ = window
            .add_event_listener_with_callback("online", online_callback.as_ref().unchecked_ref());
        let _ = window
            .add_event_listener_with_callback("offline", offline_callback.as_ref().unchecked_ref());
        let _ = window.add_event_listener_with_callback(
            "pageshow",
            pageshow_callback.as_ref().unchecked_ref(),
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
            let _ = window.remove_event_listener_with_callback(
                "offline",
                offline_callback.as_ref().unchecked_ref(),
            );
            let _ = window.remove_event_listener_with_callback(
                "pageshow",
                pageshow_callback.as_ref().unchecked_ref(),
            );
        }
    }
}

/// Start a poll interval that only fires while [`tab_active`] and run one tick immediately.
pub fn start_poll(interval_ms: u32, tick: PollTickFn) -> impl FnOnce() {
    POLL_TICK.with(|slot| {
        *slot.borrow_mut() = Some(tick.clone());
    });
    tick();

    let handle = Interval::new(interval_ms, move || {
        if tab_active() {
            tick();
        }
    });

    move || {
        POLL_TICK.with(|slot| {
            *slot.borrow_mut() = None;
        });
        drop(handle);
    }
}
