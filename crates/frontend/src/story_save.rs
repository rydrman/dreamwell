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

/// Icon overlay for the bottom-right corner of an auto-saved text field.
pub fn auto_save_field_icon(phase: AutoSavePhase, error: Option<&str>) -> Html {
    match phase {
        AutoSavePhase::Synced => html! {
            <span class="field-autosave-icon field-autosave-icon--saved" title="Saved" aria-label="Saved">
                <span class="field-autosave-glyph" aria-hidden="true">{"✓"}</span>
            </span>
        },
        AutoSavePhase::Debouncing => html! {
            <span
                class="field-autosave-icon field-autosave-icon--pending"
                title="Unsaved changes"
                aria-label="Unsaved changes"
            >
                <span class="field-autosave-glyph" aria-hidden="true">{"●"}</span>
            </span>
        },
        AutoSavePhase::Saving => html! {
            <span class="field-autosave-icon field-autosave-icon--saving" title="Saving…" aria-label="Saving">
                <span class="field-autosave-spinner" aria-hidden="true"></span>
            </span>
        },
        AutoSavePhase::Failed => {
            let message = error.unwrap_or("Save failed");
            html! {
                <span
                    class="field-autosave-icon field-autosave-icon--error"
                    title={message.to_string()}
                    aria-label="Save failed"
                >
                    <span class="field-autosave-glyph" aria-hidden="true">{"✕"}</span>
                </span>
            }
        }
    }
}

#[derive(Properties, PartialEq)]
pub struct AutoSaveFieldProps {
    pub phase: AutoSavePhase,
    #[prop_or_default]
    pub error: Option<String>,
    pub children: Children,
}

/// Wraps a text input or textarea with a save-status icon pinned to the bottom-right.
#[function_component(AutoSaveField)]
pub fn auto_save_field(props: &AutoSaveFieldProps) -> Html {
    html! {
        <span class="field-autosave-wrap">
            { for props.children.iter() }
            { auto_save_field_icon(props.phase, props.error.as_deref()) }
        </span>
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
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum AutoSaveOutcome {
    /// Draft matches the sent snapshot and last_saved was updated.
    Synced,
    /// The user kept editing after the request was sent.
    Stale,
}

/// Whether the draft differs from the last successful save.
pub fn draft_is_dirty<T: PartialEq>(draft: &T, last_saved: &T) -> bool {
    draft != last_saved
}

/// Handle a completed auto-save without refreshing parent props.
///
/// Parent/detail refresh on every field save is the main cause of controlled
/// inputs reloading and interrupting edits. Callers should only update local
/// `last_saved` here; reserve parent callbacks for structural actions.
pub fn finish_auto_save<T: Clone + PartialEq>(
    controller: &AutoSaveController,
    current: &T,
    snapshot: &T,
    last_saved: &UseStateHandle<T>,
) -> AutoSaveOutcome {
    controller.mark_saved();
    if current == snapshot {
        last_saved.set(snapshot.clone());
        AutoSaveOutcome::Synced
    } else {
        AutoSaveOutcome::Stale
    }
}

/// Record a failed save. Only surfaces the error when the draft still matches
/// the snapshot that was sent.
pub fn fail_auto_save<T: PartialEq>(
    controller: &AutoSaveController,
    current: &T,
    snapshot: &T,
    message: String,
) -> AutoSaveOutcome {
    if current == snapshot {
        controller.mark_failed(message);
        AutoSaveOutcome::Synced
    } else {
        controller.mark_saved();
        AutoSaveOutcome::Stale
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn draft_is_dirty_compares_snapshots() {
        assert!(!draft_is_dirty(&"same", &"same"));
        assert!(draft_is_dirty(&"new", &"old"));
    }
}
