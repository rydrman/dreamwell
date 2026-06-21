use dreamwell_types::{Job, Story, StoryDetail};

/// Default debounce delay for field autosave (milliseconds).
///
/// We use a small in-house debouncer on `gloo-timers` rather than `yew-hooks`
/// (requires Yew 0.23; this app is on 0.21) or stream crates like `fluxion`
/// (heavyweight for a single trailing-edge timer).
pub const AUTOSAVE_DEBOUNCE_MS: u32 = 500;

/// Generation-based debounce token: only the latest scheduled callback should run.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DebounceToken(u64);

impl DebounceToken {
    pub fn initial() -> Self {
        Self(0)
    }

    pub fn next(self) -> Self {
        Self(self.0.saturating_add(1))
    }

    pub fn is_current(self, latest: Self) -> bool {
        self == latest
    }
}

/// Monotonic fetch generation — stale responses are ignored when superseded.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FetchGeneration(u64);

impl FetchGeneration {
    #[allow(dead_code)]
    pub fn initial() -> Self {
        Self(0)
    }

    pub fn from_raw(value: u64) -> Self {
        Self(value)
    }

    pub fn raw(self) -> u64 {
        self.0
    }

    pub fn bump(self) -> Self {
        Self(self.0.saturating_add(1))
    }

    pub fn is_current(self, latest: Self) -> bool {
        self == latest
    }
}

/// Whether a completed fetch should still be applied.
pub fn fetch_response_is_current(
    started: FetchGeneration,
    latest: FetchGeneration,
    pending_matches: bool,
) -> bool {
    started.is_current(latest) && pending_matches
}

/// Whether an SSE payload should replace the in-memory story detail.
///
/// During generation we need live streaming updates. When idle, SSE reconnect
/// polls can echo stale payloads and must not stomp open editors.
pub fn should_replace_detail_from_sse(active_job: Option<&Job>) -> bool {
    active_job.is_some()
}

/// Open story detail still shows an active job, but the sidebar row no longer does.
pub fn detail_stale_vs_story_list(detail: &StoryDetail, story: &Story) -> bool {
    detail.story.active_job.is_some() && story.active_job.is_none()
}

/// Merge server story metadata into the sidebar list without touching detail.
pub fn story_list_with_detail(stories: &[Story], detail: &StoryDetail) -> Vec<Story> {
    stories
        .iter()
        .map(|s| {
            if s.id == detail.story.id {
                detail.story.clone()
            } else {
                s.clone()
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;
    use dreamwell_types::{JobStatus, JobType, LengthPreset};

    fn sample_job() -> Job {
        Job {
            id: 1,
            job_type: JobType::StoryBeatProse,
            status: JobStatus::Running,
            story_id: Some(1),
            chapter_id: Some(1),
            beat_id: Some(1),
            chat_id: None,
            message_id: None,
            game_id: None,
            turn_id: None,
            guidance_notes: String::new(),
            error: None,
            position: 0,
            created_at: Utc::now(),
            started_at: None,
            completed_at: None,
        }
    }

    fn sample_story(id: i64, title: &str) -> Story {
        Story {
            id,
            title: title.into(),
            premise: String::new(),
            tone: String::new(),
            genre: String::new(),
            pov: String::new(),
            length_preset: LengthPreset::Short,
            notes: String::new(),
            tracked_details: String::new(),
            created_at: Utc::now(),
            updated_at: Utc::now(),
            active_job: None,
            queued_jobs: 0,
        }
    }

    #[test]
    fn debounce_token_only_latest_fires() {
        let t0 = DebounceToken::initial();
        let t1 = t0.next();
        let t2 = t1.next();
        assert!(!t0.is_current(t2));
        assert!(!t1.is_current(t2));
        assert!(t2.is_current(t2));
    }

    #[test]
    fn superseded_fetch_is_not_current() {
        let g1 = FetchGeneration::initial().bump();
        let g2 = g1.bump();
        assert!(!fetch_response_is_current(g1, g2, true));
        assert!(fetch_response_is_current(g2, g2, true));
    }

    #[test]
    fn pending_mismatch_blocks_apply() {
        let g = FetchGeneration::initial().bump();
        assert!(!fetch_response_is_current(g, g, false));
    }

    #[test]
    fn sse_detail_replace_only_during_active_job() {
        let job = sample_job();
        assert!(should_replace_detail_from_sse(Some(&job)));
        assert!(!should_replace_detail_from_sse(None));
    }

    #[test]
    fn detail_stale_when_sidebar_job_cleared() {
        let mut detail_story = sample_story(1, "Detail");
        detail_story.active_job = Some(sample_job());
        let detail = StoryDetail {
            story: detail_story,
            chapters: vec![],
            actors: vec![],
            state: vec![],
        };
        let list_story = sample_story(1, "List");
        assert!(detail_stale_vs_story_list(&detail, &list_story));
    }

    #[test]
    fn detail_fresh_when_sidebar_job_matches() {
        let job = sample_job();
        let mut detail_story = sample_story(1, "Detail");
        detail_story.active_job = Some(job.clone());
        let detail = StoryDetail {
            story: detail_story,
            chapters: vec![],
            actors: vec![],
            state: vec![],
        };
        let mut list_story = sample_story(1, "List");
        list_story.active_job = Some(job);
        assert!(!detail_stale_vs_story_list(&detail, &list_story));
    }

    #[test]
    fn story_list_with_detail_updates_matching_story() {
        let stories = vec![sample_story(1, "Old"), sample_story(2, "Other")];
        let detail = StoryDetail {
            story: sample_story(1, "Fresh"),
            chapters: vec![],
            actors: vec![],
            state: vec![],
        };
        let merged = story_list_with_detail(&stories, &detail);
        assert_eq!(merged[0].title, "Fresh");
        assert_eq!(merged[1].title, "Other");
    }
}
