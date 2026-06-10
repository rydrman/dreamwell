use std::env;
use std::path::PathBuf;
use std::sync::atomic::{AtomicI64, AtomicU32, Ordering};

pub static MAX_CONCURRENT_JOBS: AtomicI64 = AtomicI64::new(1);
pub static GENERATION_MAX_RETRIES: AtomicU32 = AtomicU32::new(3);

#[derive(Clone, Debug)]
pub struct Config {
    pub database_url: String,
    pub static_dir: PathBuf,
    pub host: String,
    pub port: u16,
    pub sse_poll_interval_ms: u64,
}

impl Config {
    pub fn from_env() -> Self {
        let database_url = env::var("DREAMWELL_DATABASE_URL")
            .unwrap_or_else(|_| "sqlite:dreamwell.db".to_string());
        let static_dir = env::var("DREAMWELL_STATIC_DIR")
            .map(PathBuf::from)
            .unwrap_or_else(|_| PathBuf::from("./crates/frontend/dist"));
        let host = env::var("DREAMWELL_HOST").unwrap_or_else(|_| "0.0.0.0".to_string());
        let port = env::var("DREAMWELL_PORT")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(8080);
        let sse_poll_interval_ms = env::var("DREAMWELL_SSE_POLL_INTERVAL_MS")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(250);
        if let Ok(v) = env::var("DREAMWELL_MAX_CONCURRENT_JOBS") {
            if let Ok(n) = v.parse::<i64>() {
                MAX_CONCURRENT_JOBS.store(n.max(1), Ordering::SeqCst);
            }
        }
        if let Ok(v) = env::var("DREAMWELL_GENERATION_MAX_RETRIES") {
            if let Ok(n) = v.parse::<u32>() {
                GENERATION_MAX_RETRIES.store(n.max(1), Ordering::SeqCst);
            }
        }
        Self {
            database_url,
            static_dir,
            host,
            port,
            sse_poll_interval_ms,
        }
    }
}
