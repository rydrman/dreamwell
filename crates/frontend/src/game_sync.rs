use dreamwell_types::{Game, GameDetail, Job};

/// Whether an SSE payload should replace the in-memory game detail.
///
/// During generation we need live updates. When idle, SSE reconnect polls can
/// echo stale payloads and must not stomp the open game view.
pub fn should_replace_detail_from_sse(active_job: Option<&Job>) -> bool {
    active_job.is_some()
}

/// Open game detail still shows an active job, but the SSE payload does not.
pub fn detail_stale_vs_sse(detail: &GameDetail, payload_active: Option<&Job>) -> bool {
    detail.game.active_job.is_some() && payload_active.is_none()
}

#[allow(dead_code)]
pub fn detail_stale_vs_game_list(detail: &GameDetail, game: &Game) -> bool {
    detail.game.active_job.is_some() && game.active_job.is_none()
}

pub fn game_list_with_detail(games: &[Game], detail: &GameDetail) -> Vec<Game> {
    games
        .iter()
        .map(|g| {
            if g.id == detail.game.id {
                detail.game.clone()
            } else {
                g.clone()
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;
    use dreamwell_types::{
        ElementInstances, EngineMode, GameElementsConfig, JobStatus, JobType, ResolutionSystem,
    };

    fn sample_job() -> Job {
        Job {
            id: 1,
            job_type: JobType::GameTurnStructuredAgent,
            status: JobStatus::Running,
            chat_id: None,
            message_id: None,
            story_id: None,
            chapter_id: None,
            beat_id: None,
            game_id: Some(1),
            turn_id: Some(1),
            guidance_notes: String::new(),
            error: None,
            position: 0,
            created_at: Utc::now(),
            started_at: None,
            completed_at: None,
            generation_provider: String::new(),
            generation_model: String::new(),
            generation_notice: String::new(),
        }
    }

    fn sample_game(active_job: Option<Job>) -> Game {
        Game {
            id: 1,
            title: "Test".into(),
            premise: String::new(),
            setting: String::new(),
            gm_style: String::new(),
            opening_message: String::new(),
            character_id: None,
            scenario_id: None,
            resolution_system: ResolutionSystem::Pbta2d6,
            modifier_min: -2,
            modifier_max: 3,
            merge_resolve_scene: true,
            step_mode: false,
            engine_mode: EngineMode::ToolsStructured,
            game_elements: GameElementsConfig::default(),
            element_instances: ElementInstances::default(),
            model_checks: String::new(),
            model_resolve: String::new(),
            model_prose: String::new(),
            rules_blocks: vec![],
            state_schema: vec![],
            win_condition: None,
            scenario_triggers: vec![],
            trait_defs: vec![],
            created_at: Utc::now(),
            updated_at: Utc::now(),
            archived_at: None,
            active_job,
            queued_jobs: 0,
        }
    }

    fn sample_detail(active_job: Option<Job>) -> GameDetail {
        GameDetail {
            game: sample_game(active_job),
            turns: vec![],
            actors: vec![],
            state: vec![],
            scenes: vec![],
        }
    }

    #[test]
    fn sse_detail_replace_only_during_active_job() {
        let job = sample_job();
        assert!(should_replace_detail_from_sse(Some(&job)));
        assert!(!should_replace_detail_from_sse(None));
    }

    #[test]
    fn detail_stale_when_sse_job_cleared() {
        let detail = sample_detail(Some(sample_job()));
        assert!(detail_stale_vs_sse(&detail, None));
    }

    #[test]
    fn detail_fresh_when_sse_job_matches() {
        let job = sample_job();
        let detail = sample_detail(Some(job.clone()));
        assert!(!detail_stale_vs_sse(&detail, Some(&job)));
    }
}
