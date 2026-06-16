use dreamwell_types::{Chat, JobStatus, Message};

/// Whether any message still shows a queued or running generation job.
pub fn messages_show_active_job(messages: &[Message]) -> bool {
    messages.iter().any(|message| {
        matches!(
            message.job_status,
            Some(JobStatus::Queued) | Some(JobStatus::Running)
        )
    })
}

/// Open-chat messages still show generation, but the sidebar row no longer has an active job.
pub fn messages_stale_vs_chat(messages: &[Message], chat: &Chat) -> bool {
    messages_show_active_job(messages) && chat.active_job.is_none()
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
}
