use std::cell::RefCell;
use std::rc::Rc;

use gloo_timers::callback::Timeout;
use yew::prelude::*;

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum AutoSavePhase {
    Synced,
    Debouncing,
    Saving,
    Failed,
}

pub fn auto_save_status_html(phase: AutoSavePhase, error: Option<&str>) -> Html {
    match phase {
        AutoSavePhase::Synced => {
            html! { <span class="muted" style="font-size:0.85rem;">{"Saved"}</span> }
        }
        AutoSavePhase::Debouncing => {
            html! { <span class="muted" style="font-size:0.85rem;">{"Saving…"}</span> }
        }
        AutoSavePhase::Saving => {
            html! { <span class="muted" style="font-size:0.85rem;">{"Saving…"}</span> }
        }
        AutoSavePhase::Failed => html! {
            <span class="message-error" style="font-size:0.85rem;">{ error.unwrap_or("Save failed") }</span>
        },
    }
}

pub struct AutoSaveController {
    timeout: Rc<RefCell<Option<Timeout>>>,
    phase: UseStateHandle<AutoSavePhase>,
    error: UseStateHandle<Option<String>>,
}

impl Clone for AutoSaveController {
    fn clone(&self) -> Self {
        Self {
            timeout: self.timeout.clone(),
            phase: self.phase.clone(),
            error: self.error.clone(),
        }
    }
}

impl AutoSaveController {
    pub fn new(
        phase: UseStateHandle<AutoSavePhase>,
        error: UseStateHandle<Option<String>>,
    ) -> Self {
        Self {
            timeout: Rc::new(RefCell::new(None)),
            phase,
            error,
        }
    }

    pub fn mark_saved(&self) {
        self.error.set(None);
        self.phase.set(AutoSavePhase::Synced);
    }

    pub fn mark_failed(&self, message: String) {
        self.error.set(Some(message));
        self.phase.set(AutoSavePhase::Failed);
    }

    pub fn schedule<F>(&self, save: F)
    where
        F: FnOnce() + Clone + 'static,
    {
        if let Some(handle) = self.timeout.borrow_mut().take() {
            drop(handle);
        }
        self.phase.set(AutoSavePhase::Debouncing);
        let phase = self.phase.clone();
        let timeout = self.timeout.clone();
        *timeout.borrow_mut() = Some(Timeout::new(400, move || {
            phase.set(AutoSavePhase::Saving);
            save();
        }));
    }

    /// Mark the save complete. When `apply` is true, the caller should also push the
    /// server response into parent state. When false, the user kept editing after the
    /// request was sent — skip the parent refresh to avoid clobbering local draft.
    pub fn complete(&self, apply: bool) -> bool {
        self.mark_saved();
        apply
    }
}
