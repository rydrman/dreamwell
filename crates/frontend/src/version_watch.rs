//! Reload the tab when the server reports a newer build than the loaded bundle.

use std::cell::Cell;
use std::rc::Rc;

use dreamwell_types::HealthResponse;
use gloo_net::http::Request;
use gloo_timers::callback::{Interval, Timeout};
use wasm_bindgen_futures::spawn_local;

use crate::app_sync;
use crate::build_info;
use crate::story_save;

const CHECK_INTERVAL_MS: u32 = 60_000;
const INITIAL_DELAY_MS: u32 = 5_000;

thread_local! {
    static RELOADING: Cell<bool> = const { Cell::new(false) };
}

pub fn install() -> impl FnOnce() {
    let embedded = build_info::GIT_SHA.to_string();
    let check = Rc::new(move || {
        if !app_sync::tab_active() || RELOADING.with(|reloading| reloading.get()) {
            return;
        }
        let embedded = embedded.clone();
        spawn_local(async move {
            let Some(remote) = fetch_health_sha().await else {
                return;
            };
            if shas_match(&embedded, &remote) {
                return;
            }
            RELOADING.with(|reloading| reloading.set(true));
            story_save::flush_all_pending_autosaves();
            if let Some(window) = web_sys::window() {
                let _ = window.location().reload();
            }
        });
    });

    let scope = app_sync::register_scope(
        {
            let check = check.clone();
            move |_ctx| check()
        },
        || {},
    );

    check();

    let timeout = Timeout::new(INITIAL_DELAY_MS, {
        let check = check.clone();
        move || check()
    });

    let interval = Interval::new(CHECK_INTERVAL_MS, move || check());

    move || {
        drop(scope);
        drop(timeout);
        drop(interval);
    }
}

async fn fetch_health_sha() -> Option<String> {
    let response = Request::get("/api/health").send().await.ok()?;
    if !response.ok() {
        return None;
    }
    let health: HealthResponse = response.json().await.ok()?;
    health
        .git_sha
        .map(|sha| sha.trim().to_string())
        .filter(|sha| !sha.is_empty())
}

fn shas_match(embedded: &str, remote: &str) -> bool {
    if embedded == remote {
        return true;
    }
    let (shorter, longer) = if embedded.len() <= remote.len() {
        (embedded, remote)
    } else {
        (remote, embedded)
    };
    longer.starts_with(shorter)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn matches_exact_and_prefix_shas() {
        assert!(shas_match("c149a0b", "c149a0b"));
        assert!(shas_match("c149a0b", "c149a0bb405f"));
        assert!(shas_match("c149a0bb405f", "c149a0b"));
        assert!(!shas_match("c149a0b", "6bd7980"));
    }
}
