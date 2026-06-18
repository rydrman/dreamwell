use dreamwell_types::{Chat, JobStatus, Message};

use crate::summary_ui::messages_have_stale_summarize_placeholder;

/// Whether any message still shows a queued or running generation job.
pub fn messages_show_active_job(messages: &[Message]) -> bool {
    messages.iter().any(|message| {
        matches!(
            message.job_status,
            Some(JobStatus::Queued) | Some(JobStatus::Running)
        )
    })
}

/// Open-chat messages are behind the sidebar chat row (generation or summarize).
pub fn messages_stale_vs_chat(messages: &[Message], chat: &Chat) -> bool {
    (messages_show_active_job(messages) && chat.active_job.is_none())
        || messages_have_stale_summarize_placeholder(messages, chat)
}

/// Apply SSE message updates for live generation or benign idle snapshots.
/// Completed jobs may also be applied directly when the payload is fresh.
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
            is_summary: false,
            in_summary: false,
            created_at: Utc::now(),
            job_status,
            generation_error: None,
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
            guidance_notes: String::new(),
            error: None,
            position: 0,
            created_at: Utc::now(),
            started_at: None,
            completed_at: None,
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
            guidance_notes: String::new(),
            error: None,
            position: 0,
            created_at: Utc::now(),
            started_at: None,
            completed_at: None,
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

    fn summarize_placeholder_message() -> Message {
        Message {
            id: 2,
            chat_id: 1,
            role: MessageRole::System,
            content: "Summarizing earlier messages…".into(),
            thought_content: String::new(),
            thought_duration_ms: None,
            thought_in_progress: false,
            variable_updates: vec![],
            is_summary: true,
            in_summary: false,
            created_at: Utc::now(),
            job_status: None,
            generation_error: None,
        }
    }

    #[test]
    fn detects_stale_summarize_placeholder_when_job_cleared() {
        let messages = vec![summarize_placeholder_message()];
        let chat = sample_chat(None);
        assert!(messages_stale_vs_chat(&messages, &chat));
    }

    #[test]
    fn summarize_placeholder_matches_active_job() {
        let job = Job {
            id: 1,
            job_type: JobType::ChatSummarize,
            status: JobStatus::Running,
            chat_id: Some(1),
            message_id: Some(2),
            story_id: None,
            chapter_id: None,
            beat_id: None,
            guidance_notes: String::new(),
            error: None,
            position: 0,
            created_at: Utc::now(),
            started_at: None,
            completed_at: None,
        };
        let messages = vec![summarize_placeholder_message()];
        let chat = sample_chat(Some(job));
        assert!(!messages_stale_vs_chat(&messages, &chat));
    }
}
