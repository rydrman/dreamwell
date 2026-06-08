pub mod characters;
pub mod chats;
pub mod settings;
pub mod stories;

use sqlx::SqlitePool;

use crate::queue::JobQueue;

#[derive(Clone)]
pub struct AppState {
    pub pool: SqlitePool,
    pub queue: JobQueue,
    pub sse_poll_interval_ms: u64,
}
