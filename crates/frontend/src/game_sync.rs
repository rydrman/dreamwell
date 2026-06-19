use dreamwell_types::{Game, GameDetail, Job};

/// Whether an SSE payload should replace the in-memory game detail.
pub fn should_replace_detail_from_sse(active_job: Option<&Job>) -> bool {
    active_job.is_some()
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
