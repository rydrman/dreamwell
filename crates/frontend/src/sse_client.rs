//! Pure SSE connection state — testable without browser APIs.
#![allow(dead_code)]

pub const IDLE_RECONNECT_MS: u32 = 2_000;
const MAX_BACKOFF_MS: u32 = 30_000;
const BASE_BACKOFF_MS: u32 = 1_000;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ConnectionMode {
    Active,
    Paused,
    Stopped,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ScheduledReconnect {
    None,
    AfterMs(u32),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SseConnectionState {
    mode: ConnectionMode,
    attempt: u32,
    pending_reconnect_ms: Option<u32>,
}

impl Default for SseConnectionState {
    fn default() -> Self {
        Self::new()
    }
}

impl SseConnectionState {
    pub fn new() -> Self {
        Self {
            mode: ConnectionMode::Active,
            attempt: 0,
            pending_reconnect_ms: None,
        }
    }

    pub fn mode(&self) -> ConnectionMode {
        self.mode
    }

    pub fn attempt(&self) -> u32 {
        self.attempt
    }

    pub fn pending_reconnect(&self) -> ScheduledReconnect {
        match self.pending_reconnect_ms {
            None => ScheduledReconnect::None,
            Some(ms) => ScheduledReconnect::AfterMs(ms),
        }
    }

    pub fn can_connect(&self) -> bool {
        self.mode == ConnectionMode::Active
    }

    pub fn pause(&mut self) {
        if self.mode == ConnectionMode::Stopped {
            return;
        }
        self.mode = ConnectionMode::Paused;
        self.pending_reconnect_ms = None;
    }

    pub fn resume(&mut self) {
        if self.mode == ConnectionMode::Stopped {
            return;
        }
        self.mode = ConnectionMode::Active;
        self.attempt = 0;
        self.pending_reconnect_ms = None;
    }

    pub fn reconnect(&mut self) {
        if self.mode == ConnectionMode::Stopped {
            return;
        }
        self.mode = ConnectionMode::Active;
        self.attempt = 0;
        self.pending_reconnect_ms = None;
    }

    pub fn stop(&mut self) {
        self.mode = ConnectionMode::Stopped;
        self.pending_reconnect_ms = None;
    }

    pub fn on_message(&mut self) {
        self.attempt = 0;
    }

    pub fn on_connect_failed(&mut self) -> ScheduledReconnect {
        self.schedule_error_reconnect()
    }

    pub fn on_idle(&mut self) -> ScheduledReconnect {
        if self.mode != ConnectionMode::Active {
            return ScheduledReconnect::None;
        }
        self.pending_reconnect_ms = Some(IDLE_RECONNECT_MS);
        ScheduledReconnect::AfterMs(IDLE_RECONNECT_MS)
    }

    pub fn on_error(&mut self) -> ScheduledReconnect {
        if self.mode == ConnectionMode::Stopped {
            return ScheduledReconnect::None;
        }
        self.schedule_error_reconnect()
    }

    pub fn clear_pending_reconnect(&mut self) {
        self.pending_reconnect_ms = None;
    }

    pub fn error_backoff_ms(attempt: u32) -> u32 {
        BASE_BACKOFF_MS
            .saturating_mul(2u32.saturating_pow(attempt.min(5)))
            .min(MAX_BACKOFF_MS)
    }

    fn schedule_error_reconnect(&mut self) -> ScheduledReconnect {
        if self.mode != ConnectionMode::Active {
            return ScheduledReconnect::None;
        }
        let delay_ms = Self::error_backoff_ms(self.attempt);
        self.attempt = self.attempt.saturating_add(1);
        self.pending_reconnect_ms = Some(delay_ms);
        ScheduledReconnect::AfterMs(delay_ms)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pause_suppresses_reconnect_scheduling() {
        let mut state = SseConnectionState::new();
        state.pause();
        assert!(!state.can_connect());
        assert_eq!(state.on_idle(), ScheduledReconnect::None);
        assert_eq!(state.on_error(), ScheduledReconnect::None);
    }

    #[test]
    fn resume_clears_backoff_and_allows_connect() {
        let mut state = SseConnectionState::new();
        state.on_error();
        assert_eq!(state.attempt(), 1);
        state.pause();
        state.resume();
        assert!(state.can_connect());
        assert_eq!(state.attempt(), 0);
        assert_eq!(state.pending_reconnect(), ScheduledReconnect::None);
    }

    #[test]
    fn idle_schedules_two_second_reconnect() {
        let mut state = SseConnectionState::new();
        assert_eq!(
            state.on_idle(),
            ScheduledReconnect::AfterMs(IDLE_RECONNECT_MS)
        );
    }

    #[test]
    fn error_backoff_caps_at_thirty_seconds() {
        assert_eq!(SseConnectionState::error_backoff_ms(0), 1_000);
        assert_eq!(SseConnectionState::error_backoff_ms(1), 2_000);
        assert_eq!(SseConnectionState::error_backoff_ms(5), 30_000);
        assert_eq!(SseConnectionState::error_backoff_ms(10), 30_000);
    }

    #[test]
    fn reconnect_resets_attempt_while_active() {
        let mut state = SseConnectionState::new();
        state.on_error();
        state.reconnect();
        assert_eq!(state.attempt(), 0);
    }

    #[test]
    fn stop_prevents_all_reconnects() {
        let mut state = SseConnectionState::new();
        state.stop();
        assert_eq!(state.on_error(), ScheduledReconnect::None);
        state.resume();
        assert_eq!(state.mode(), ConnectionMode::Stopped);
    }
}
