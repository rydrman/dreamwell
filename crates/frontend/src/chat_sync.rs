use chrono::{DateTime, Utc};
use dreamwell_types::{Chat, JobStatus, Message};

/// Whether any message still shows a queued or running generation job.
pub fn messages_show_active_job(messages: &[Message]) -> bool {
    messages.iter().any(message_has_active_job)
}

/// Whether a single message should be treated as queued or streaming.
pub fn message_has_active_job(message: &Message) -> bool {
    matches!(
        message.job_status,
        Some(JobStatus::Queued) | Some(JobStatus::Running)
    )
}

/// Whether message-level job status should drive UI (composer, bubbles).
///
/// When the sidebar chat row has cleared its active job but open messages still
/// show `queued`/`running`, the message list is stale and must not block input.
pub fn message_generation_live(chat: &Chat, messages: &[Message]) -> bool {
    !messages_stale_vs_chat(messages, chat)
}

/// Open-chat messages still show generation, but the sidebar row no longer has an active job.
pub fn messages_stale_vs_chat(messages: &[Message], chat: &Chat) -> bool {
    messages_show_active_job(messages) && chat.active_job.is_none()
}

/// Whether the open chat likely has server-side messages we have not loaded yet.
///
/// Used on poll ticks and tab resume when generation is idle locally but the chat row
/// was touched after our last successful sync (e.g. user sent while the tab was hidden).
pub fn messages_need_refetch(
    messages: &[Message],
    chat: &Chat,
    last_synced_chat_updated_at: Option<DateTime<Utc>>,
) -> bool {
    if messages_stale_vs_chat(messages, chat) {
        return true;
    }
    if let Some(job) = &chat.active_job {
        if let Some(msg_id) = job.message_id {
            if !messages.iter().any(|message| message.id == msg_id) {
                return true;
            }
        }
        if message_generation_live(chat, messages) {
            return false;
        }
    }
    if messages_show_active_job(messages) {
        return false;
    }
    match last_synced_chat_updated_at {
        Some(synced) => chat.updated_at > synced,
        None => chat.active_job.is_some() || chat.queued_jobs > 0,
    }
}

/// Apply SSE message updates for live generation or benign idle snapshots.
/// Completion transitions are handled by REST refetch, not this guard.
pub fn should_apply_messages_from_sse(payload_messages: &[Message], payload_chat: &Chat) -> bool {
    if payload_chat.active_job.is_some() {
        return true;
    }
    !messages_stale_vs_chat(payload_messages, payload_chat)
}

/// Whether two message lists would render identically in the chat timeline.
///
/// Ignores bookkeeping fields such as `in_summary` that change during multi-pass
/// summarization without affecting visible bubbles.
pub fn messages_display_equivalent(current: &[Message], incoming: &[Message]) -> bool {
    if current.len() != incoming.len() {
        return false;
    }
    current
        .iter()
        .zip(incoming)
        .all(|(left, right)| message_display_equivalent(left, right))
}

fn message_display_equivalent(left: &Message, right: &Message) -> bool {
    left.id == right.id
        && left.role == right.role
        && left.content == right.content
        && left.thought_content == right.thought_content
        && left.thought_duration_ms == right.thought_duration_ms
        && left.thought_in_progress == right.thought_in_progress
        && left.variable_updates == right.variable_updates
        && left.reply_beats == right.reply_beats
        && left.state_changes == right.state_changes
        && left.generation_phase == right.generation_phase
        && left.is_summary == right.is_summary
        && left.job_status == right.job_status
        && left.generation_error == right.generation_error
        && left.generation_notice == right.generation_notice
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;
    use dreamwell_types::{Job, JobType, MessageRole};

    fn sample_chat(active_job: Option<Job>) -> Chat {
        Chat {
            id: 1,
            title: "Test".into(),
            character_id: 1,
            character_name: "Char".into(),
            summary: String::new(),
            created_at: Utc::now(),
            updated_at: Utc::now(),
            archived_at: None,
            active_job,
            queued_jobs: 0,
        }
    }

    fn assistant_message(job_status: Option<JobStatus>) -> Message {
        Message {
            id: 1,
            chat_id: 1,
            role: MessageRole::Assistant,
            content: String::new(),
            thought_content: String::new(),
            thought_duration_ms: None,
            thought_in_progress: false,
            variable_updates: vec![],
            reply_beats: vec![],
            state_changes: vec![],
            generation_phase: String::new(),
            is_summary: false,
            in_summary: false,
            created_at: Utc::now(),
            job_status,
            generation_error: None,
            generation_notice: String::new(),
        }
    }

    #[test]
    fn detects_stale_messages_when_sidebar_job_cleared() {
        let messages = vec![assistant_message(Some(JobStatus::Running))];
        let chat = sample_chat(None);
        assert!(messages_stale_vs_chat(&messages, &chat));
    }

    #[test]
    fn fresh_messages_match_active_sidebar_job() {
        let job = Job {
            id: 1,
            job_type: JobType::ChatMessage,
            status: JobStatus::Running,
            chat_id: Some(1),
            message_id: Some(1),
            story_id: None,
            chapter_id: None,
            beat_id: None,
            game_id: None,
            turn_id: None,
            guidance_notes: String::new(),
            error: None,
            position: 0,
            created_at: Utc::now(),
            started_at: None,
            completed_at: None,
            generation_provider: String::new(),
            generation_model: String::new(),
            generation_notice: String::new(),
        };
        let messages = vec![assistant_message(Some(JobStatus::Running))];
        let chat = sample_chat(Some(job));
        assert!(!messages_stale_vs_chat(&messages, &chat));
    }

    #[test]
    fn completed_messages_are_not_stale() {
        let messages = vec![assistant_message(None)];
        let chat = sample_chat(None);
        assert!(!messages_stale_vs_chat(&messages, &chat));
    }

    #[test]
    fn sse_apply_while_active_job() {
        let job = Job {
            id: 1,
            job_type: JobType::ChatMessage,
            status: JobStatus::Running,
            chat_id: Some(1),
            message_id: Some(1),
            story_id: None,
            chapter_id: None,
            beat_id: None,
            game_id: None,
            turn_id: None,
            guidance_notes: String::new(),
            error: None,
            position: 0,
            created_at: Utc::now(),
            started_at: None,
            completed_at: None,
            generation_provider: String::new(),
            generation_model: String::new(),
            generation_notice: String::new(),
        };
        let messages = vec![assistant_message(Some(JobStatus::Running))];
        let chat = sample_chat(Some(job));
        assert!(should_apply_messages_from_sse(&messages, &chat));
    }

    #[test]
    fn sse_apply_idle_completed_snapshot() {
        let messages = vec![assistant_message(None)];
        let chat = sample_chat(None);
        assert!(should_apply_messages_from_sse(&messages, &chat));
    }

    #[test]
    fn sse_reject_stale_idle_echo() {
        let messages = vec![assistant_message(Some(JobStatus::Running))];
        let chat = sample_chat(None);
        assert!(!should_apply_messages_from_sse(&messages, &chat));
    }

    #[test]
    fn display_equivalent_ignores_in_summary_flag() {
        let mut summarized = assistant_message(None);
        summarized.in_summary = true;
        let original = assistant_message(None);
        assert!(message_display_equivalent(&original, &summarized));
    }

    #[test]
    fn display_equivalent_detects_content_change() {
        let mut changed = assistant_message(None);
        changed.content = "updated".into();
        let original = assistant_message(None);
        assert!(!message_display_equivalent(&original, &changed));
    }

    #[test]
    fn display_equivalent_lists_match() {
        let left = vec![assistant_message(None), assistant_message(None)];
        let right = left.clone();
        assert!(messages_display_equivalent(&left, &right));
    }

    #[test]
    fn message_generation_live_false_when_messages_stale() {
        let messages = vec![assistant_message(Some(JobStatus::Running))];
        let chat = sample_chat(None);
        assert!(!message_generation_live(&chat, &messages));
    }

    #[test]
    fn message_generation_live_true_when_chat_has_active_job() {
        let job = Job {
            id: 1,
            job_type: JobType::ChatMessage,
            status: JobStatus::Running,
            chat_id: Some(1),
            message_id: Some(1),
            story_id: None,
            chapter_id: None,
            beat_id: None,
            game_id: None,
            turn_id: None,
            guidance_notes: String::new(),
            error: None,
            position: 0,
            created_at: Utc::now(),
            started_at: None,
            completed_at: None,
            generation_provider: String::new(),
            generation_model: String::new(),
            generation_notice: String::new(),
        };
        let messages = vec![assistant_message(Some(JobStatus::Running))];
        let chat = sample_chat(Some(job));
        assert!(message_generation_live(&chat, &messages));
    }

    #[test]
    fn need_refetch_when_chat_touched_after_last_sync() {
        let messages = vec![assistant_message(None)];
        let mut chat = sample_chat(None);
        let synced = chat.updated_at;
        chat.updated_at = synced + chrono::Duration::seconds(5);
        assert!(messages_need_refetch(&messages, &chat, Some(synced)));
    }

    #[test]
    fn need_refetch_when_active_job_message_missing_locally() {
        let messages = vec![assistant_message(None)];
        let job = Job {
            id: 1,
            job_type: JobType::ChatMessage,
            status: JobStatus::Running,
            chat_id: Some(1),
            message_id: Some(99),
            story_id: None,
            chapter_id: None,
            beat_id: None,
            game_id: None,
            turn_id: None,
            guidance_notes: String::new(),
            error: None,
            position: 0,
            created_at: Utc::now(),
            started_at: None,
            completed_at: None,
            generation_provider: String::new(),
            generation_model: String::new(),
            generation_notice: String::new(),
        };
        let chat = sample_chat(Some(job));
        assert!(messages_need_refetch(
            &messages,
            &chat,
            Some(chat.updated_at)
        ));
    }

    #[test]
    fn skip_refetch_while_generation_live() {
        let job = Job {
            id: 1,
            job_type: JobType::ChatMessage,
            status: JobStatus::Running,
            chat_id: Some(1),
            message_id: Some(1),
            story_id: None,
            chapter_id: None,
            beat_id: None,
            game_id: None,
            turn_id: None,
            guidance_notes: String::new(),
            error: None,
            position: 0,
            created_at: Utc::now(),
            started_at: None,
            completed_at: None,
            generation_provider: String::new(),
            generation_model: String::new(),
            generation_notice: String::new(),
        };
        let messages = vec![assistant_message(Some(JobStatus::Running))];
        let mut chat = sample_chat(Some(job));
        let synced = chat.updated_at;
        chat.updated_at = synced + chrono::Duration::seconds(5);
        assert!(!messages_need_refetch(&messages, &chat, Some(synced)));
    }
}
