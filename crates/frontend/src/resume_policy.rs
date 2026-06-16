//! Pure resume planning — testable policy for tab-return sync.
#![allow(dead_code)]

use crate::app_sync::ResumeContext;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ResumeAction {
    ResumeSse,
    ReconnectSse,
    FetchMessages,
    FetchStory,
    PollTick,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ResumePlanInput {
    pub ctx: ResumeContext,
    pub open_chat_id: Option<i64>,
    pub open_story_id: Option<i64>,
    pub messages_stale: bool,
    pub detail_stale: bool,
    pub local_generation_active: bool,
}

pub fn plan_resume(input: ResumePlanInput) -> Vec<ResumeAction> {
    let mut actions = Vec::new();

    if input.open_chat_id.is_some() {
        actions.push(ResumeAction::FetchMessages);
    }
    if input.open_story_id.is_some() {
        actions.push(ResumeAction::FetchStory);
    }

    if input.ctx.force() {
        actions.push(ResumeAction::ReconnectSse);
    } else {
        actions.push(ResumeAction::ResumeSse);
    }

    actions.push(ResumeAction::PollTick);

    if input.messages_stale {
        actions.push(ResumeAction::FetchMessages);
    }
    if input.detail_stale {
        actions.push(ResumeAction::FetchStory);
    }

    if input.local_generation_active && !input.ctx.force() {
        // Prefer refetch before SSE reconnect when generation may still be running.
        if input.open_chat_id.is_some() {
            actions.insert(0, ResumeAction::FetchMessages);
        }
    }

    actions
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app_sync::ResumeReason;

    fn ctx(reason: ResumeReason, force: bool) -> ResumeContext {
        ResumeContext { reason, force }
    }

    fn contains(actions: &[ResumeAction], action: ResumeAction) -> bool {
        actions.contains(&action)
    }

    #[test]
    fn return_while_job_running_refetches_and_resumes_sse() {
        let actions = plan_resume(ResumePlanInput {
            ctx: ctx(ResumeReason::Visibility, false),
            open_chat_id: Some(1),
            open_story_id: None,
            messages_stale: false,
            detail_stale: false,
            local_generation_active: true,
        });
        assert!(contains(&actions, ResumeAction::FetchMessages));
        assert!(contains(&actions, ResumeAction::ResumeSse));
        assert!(contains(&actions, ResumeAction::PollTick));
        assert_eq!(actions.first(), Some(&ResumeAction::FetchMessages));
    }

    #[test]
    fn return_after_job_finished_refetches_on_stale_messages() {
        let actions = plan_resume(ResumePlanInput {
            ctx: ctx(ResumeReason::Visibility, false),
            open_chat_id: Some(1),
            open_story_id: None,
            messages_stale: true,
            detail_stale: false,
            local_generation_active: false,
        });
        assert!(contains(&actions, ResumeAction::FetchMessages));
        assert!(contains(&actions, ResumeAction::ResumeSse));
    }

    #[test]
    fn pageshow_force_reconnects_sse() {
        let actions = plan_resume(ResumePlanInput {
            ctx: ctx(ResumeReason::PageShow, true),
            open_chat_id: Some(1),
            open_story_id: Some(2),
            messages_stale: false,
            detail_stale: false,
            local_generation_active: false,
        });
        assert!(contains(&actions, ResumeAction::ReconnectSse));
        assert!(contains(&actions, ResumeAction::FetchMessages));
        assert!(contains(&actions, ResumeAction::FetchStory));
    }

    #[test]
    fn stale_detail_triggers_story_refetch() {
        let actions = plan_resume(ResumePlanInput {
            ctx: ctx(ResumeReason::Visibility, false),
            open_chat_id: None,
            open_story_id: Some(3),
            messages_stale: false,
            detail_stale: true,
            local_generation_active: false,
        });
        assert!(contains(&actions, ResumeAction::FetchStory));
    }
}
