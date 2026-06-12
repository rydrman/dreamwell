//! Post-generation follow-up jobs run after a chat reply completes successfully.
//!
//! Each follow-up module owns its own enqueue and run logic. This coordinator
//! invokes them in order so new follow-up types can be added without changing
//! the chat generation handler.

use dreamwell_types::Settings;
use sqlx::SqlitePool;
use tokio::sync::mpsc;

use crate::error::AppResult;
use crate::summarize::maybe_enqueue_summarize;
use crate::variable_recheck::maybe_enqueue_variable_recheck;

/// Context for enqueueing follow-up jobs after chat generation succeeds.
pub struct ChatGenerationComplete<'a> {
    pub pool: &'a SqlitePool,
    pub work_tx: &'a mpsc::UnboundedSender<()>,
    pub chat_id: i64,
    pub message_id: i64,
    pub settings: &'a Settings,
}

/// Enqueue all enabled post-generation follow-up jobs for a completed chat reply.
pub async fn enqueue_chat_followups(ctx: &ChatGenerationComplete<'_>) -> AppResult<()> {
    maybe_enqueue_variable_recheck(
        ctx.pool,
        ctx.work_tx,
        ctx.chat_id,
        ctx.message_id,
        ctx.settings,
    )
    .await?;
    maybe_enqueue_summarize(ctx.pool, ctx.work_tx, ctx.chat_id, ctx.settings).await?;
    Ok(())
}
