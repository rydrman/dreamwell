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

/// Apply SSE message updates for live generation or benign idle snapshots.
/// Completion transitions are handled by REST refetch, not this guard.
pub fn should_apply_messages_from_sse(payload_messages: &[Message], payload_chat: &Chat) -> bool {
    if payload_chat.active_job.is_some() {
        return true;
    }
    !messages_stale_vs_chat(payload_messages, payload_chat)
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
}
