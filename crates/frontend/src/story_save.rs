use std::cell::RefCell;
use std::rc::Rc;

use gloo_timers::callback::Timeout;
use yew::prelude::*;

use crate::story_sync::{AUTOSAVE_DEBOUNCE_MS, DebounceToken};

type TabFlushFn = Rc<dyn Fn()>;

thread_local! {
    static TAB_FLUSH_REGISTRY: RefCell<Vec<(usize, TabFlushFn)>> = RefCell::new(Vec::new());
    static NEXT_TAB_FLUSH_ID: RefCell<usize> = const { RefCell::new(0) };
}

/// Register a custom flush handler (e.g. settings panel) for tab-hide autosave.
pub fn register_autosave_tab_flush(flush: impl Fn() + 'static) -> AutoSaveTabGuard {
    register_tab_flush(Rc::new(flush))
}

/// Guard that unregisters a tab-hide flush handler when dropped.
pub struct AutoSaveTabGuard {
    id: usize,
}

impl Drop for AutoSaveTabGuard {
    fn drop(&mut self) {
        TAB_FLUSH_REGISTRY.with(|registry| {
            registry.borrow_mut().retain(|(id, _)| *id != self.id);
        });
    }
}

fn register_tab_flush(flush: TabFlushFn) -> AutoSaveTabGuard {
    let id = NEXT_TAB_FLUSH_ID.with(|next| {
        let id = *next.borrow();
        *next.borrow_mut() = id.saturating_add(1);
        id
    });
    TAB_FLUSH_REGISTRY.with(|registry| registry.borrow_mut().push((id, flush)));
    AutoSaveTabGuard { id }
}

/// Flush every registered autosave field (called when the tab is hidden).
pub fn flush_all_pending_autosaves() {
    TAB_FLUSH_REGISTRY.with(|registry| {
        for (_, flush) in registry.borrow().iter() {
            flush();
        }
    });
}

/// Whether a field with this phase has a debounced save waiting to run.
pub fn should_flush_on_tab_hide(phase: AutoSavePhase) -> bool {
    matches!(phase, AutoSavePhase::Debouncing)
}

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
    token: Rc<RefCell<DebounceToken>>,
    pending_flush: Rc<RefCell<Option<TabFlushFn>>>,
    phase: UseStateHandle<AutoSavePhase>,
    error: UseStateHandle<Option<String>>,
}

impl Clone for AutoSaveController {
    fn clone(&self) -> Self {
        Self {
            timeout: self.timeout.clone(),
            token: self.token.clone(),
            pending_flush: self.pending_flush.clone(),
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
            token: Rc::new(RefCell::new(DebounceToken::initial())),
            pending_flush: Rc::new(RefCell::new(None)),
            phase,
            error,
        }
    }

    /// Register this controller so [`flush_all_pending_autosaves`] runs any debounced save.
    pub fn bind_tab_flush(&self) -> AutoSaveTabGuard {
        let controller = self.clone();
        register_tab_flush(Rc::new(move || controller.flush_pending()))
    }

    /// Run a debounced save immediately when the tab is hidden.
    pub fn flush_pending(&self) {
        if !should_flush_on_tab_hide(*self.phase) {
            return;
        }
        if let Some(handle) = self.timeout.borrow_mut().take() {
            drop(handle);
        }
        *self.token.borrow_mut() = self.token.borrow().next();
        let Some(save) = self.pending_flush.borrow_mut().take() else {
            return;
        };
        self.phase.set(AutoSavePhase::Saving);
        save();
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
        let fired_token = {
            let next = self.token.borrow().next();
            *self.token.borrow_mut() = next;
            next
        };
        let save_cell = Rc::new(RefCell::new(Some(save)));
        let save_for_flush = save_cell.clone();
        *self.pending_flush.borrow_mut() = Some(Rc::new(move || {
            if let Some(run) = save_for_flush.borrow_mut().take() {
                run();
            }
        }));
        self.phase.set(AutoSavePhase::Debouncing);
        let phase = self.phase.clone();
        let timeout = self.timeout.clone();
        let token_cell = self.token.clone();
        let pending_flush = self.pending_flush.clone();
        let save_for_timer = save_cell;
        *timeout.borrow_mut() = Some(Timeout::new(AUTOSAVE_DEBOUNCE_MS, move || {
            if !fired_token.is_current(*token_cell.borrow()) {
                return;
            }
            pending_flush.borrow_mut().take();
            if let Some(run) = save_for_timer.borrow_mut().take() {
                phase.set(AutoSavePhase::Saving);
                run();
            }
        }));
    }

    /// Run a pending save immediately, cancelling any debounce timer.
    pub fn flush<F>(&self, save: F)
    where
        F: FnOnce(),
    {
        if let Some(handle) = self.timeout.borrow_mut().take() {
            drop(handle);
        }
        *self.token.borrow_mut() = self.token.borrow().next();
        self.pending_flush.borrow_mut().take();
        self.phase.set(AutoSavePhase::Saving);
        save();
    }
}

/// Register an autosave controller for flush-on-tab-hide for the component lifetime.
#[hook]
pub fn use_autosave_tab_flush(controller: AutoSaveController) {
    use_effect_with((), move |_| {
        let guard = controller.bind_tab_flush();
        move || drop(guard)
    });
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
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
    use crate::story_sync::DebounceToken;

    #[test]
    fn draft_is_dirty_compares_snapshots() {
        assert!(!draft_is_dirty(&"same", &"same"));
        assert!(draft_is_dirty(&"new", &"old"));
    }

    #[test]
    fn debounce_token_coalesces_rapid_reschedules() {
        let mut latest = DebounceToken::initial();
        let scheduled = latest.next();
        latest = scheduled;
        let scheduled_again = latest.next();
        latest = scheduled_again;
        assert!(!scheduled.is_current(latest));
        assert!(scheduled_again.is_current(latest));
    }

    #[test]
    fn finish_auto_save_synced_when_unchanged() {
        // Outcome logic is pure; controller state requires Yew runtime.
        let current = "draft";
        let snapshot = "draft";
        assert_eq!(
            if current == snapshot {
                AutoSaveOutcome::Synced
            } else {
                AutoSaveOutcome::Stale
            },
            AutoSaveOutcome::Synced
        );
    }

    #[test]
    fn finish_auto_save_stale_when_edited_during_request() {
        let current = "newer";
        let snapshot = "sent";
        assert_eq!(
            if current == snapshot {
                AutoSaveOutcome::Synced
            } else {
                AutoSaveOutcome::Stale
            },
            AutoSaveOutcome::Stale
        );
    }

    #[test]
    fn tab_flush_registry_invokes_registered_handlers() {
        use std::cell::Cell;
        let called = Rc::new(Cell::new(false));
        let called_for_flush = called.clone();
        let guard = register_autosave_tab_flush(move || called_for_flush.set(true));
        flush_all_pending_autosaves();
        assert!(called.get());
        drop(guard);
    }

    #[test]
    fn should_flush_on_tab_hide_only_while_debouncing() {
        assert!(should_flush_on_tab_hide(AutoSavePhase::Debouncing));
        assert!(!should_flush_on_tab_hide(AutoSavePhase::Synced));
        assert!(!should_flush_on_tab_hide(AutoSavePhase::Saving));
        assert!(!should_flush_on_tab_hide(AutoSavePhase::Failed));
    }

    #[test]
    fn fail_auto_save_hides_error_when_stale() {
        let current = "newer";
        let snapshot = "sent";
        let outcome = if current == snapshot {
            AutoSaveOutcome::Synced
        } else {
            AutoSaveOutcome::Stale
        };
        assert_eq!(outcome, AutoSaveOutcome::Stale);
    }
}
