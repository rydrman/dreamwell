use std::cell::Cell;

thread_local! {
    static AUTH_HANDLING: Cell<bool> = const { Cell::new(false) };
}

pub const AUTH_EXPIRED: &str = "Session expired";

pub fn is_auth_expired(err: &str) -> bool {
    err == AUTH_EXPIRED
}

pub fn is_auth_status(status: u16) -> bool {
    status == 401 || status == 403
}

/// Detect Authelia / NGINX ingress auth failures on API calls.
///
/// Unauthenticated XHR requests may return 401/403, or follow a redirect to the
/// sign-in page and come back as `200` with `text/html`.
pub fn is_auth_redirect_response(status: u16, content_type: Option<&str>) -> bool {
    if is_auth_status(status) {
        return true;
    }
    status == 200 && content_type.is_some_and(|ct| ct.to_ascii_lowercase().contains("text/html"))
}

pub fn clear_session_cache() {
    let Some(storage) = web_sys::window().and_then(|w| w.session_storage().ok().flatten()) else {
        return;
    };
    let length = storage.length().unwrap_or(0);
    let mut keys = Vec::new();
    for index in 0..length {
        if let Ok(Some(key)) = storage.key(index) {
            if key.starts_with("dreamwell.") {
                keys.push(key);
            }
        }
    }
    for key in keys {
        let _ = storage.remove_item(&key);
    }
}

/// Clear cached UI state and reload so Ingress can send the user to Authelia.
pub fn handle_auth_expiry() {
    if AUTH_HANDLING.with(|handling| handling.get()) {
        return;
    }
    AUTH_HANDLING.with(|handling| handling.set(true));
    clear_session_cache();
    if let Some(window) = web_sys::window() {
        let _ = window.location().reload();
    }
}

pub fn auth_expiry_error() -> String {
    AUTH_EXPIRED.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_auth_status_codes() {
        assert!(is_auth_status(401));
        assert!(is_auth_status(403));
        assert!(!is_auth_status(404));
        assert!(!is_auth_status(500));
    }

    #[test]
    fn detects_html_sign_in_page() {
        assert!(is_auth_redirect_response(
            200,
            Some("text/html; charset=utf-8")
        ));
        assert!(!is_auth_redirect_response(
            200,
            Some("application/json; charset=utf-8")
        ));
        assert!(!is_auth_redirect_response(200, None));
    }
}
