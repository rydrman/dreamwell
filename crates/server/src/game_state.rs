use std::collections::HashMap;

use dreamwell_state::{
    build_state_block as state_build_block,
    build_state_block_annotated as state_build_block_annotated, plan_revert_changes,
    plan_state_changes,
};
use dreamwell_types::{
    AppliedStateChange, GameActor, GameStateEntry, StateChangeRequest, StateEntry,
};
use sqlx::SqlitePool;

use crate::error::AppResult;
use crate::state_persist::{persist_state_mutation, persist_state_revert};

pub use dreamwell_state::{skill_modifier, validate_skill};

pub fn to_state_entry(entry: &GameStateEntry) -> StateEntry {
    StateEntry {
        id: entry.id,
        game_id: entry.game_id,
        actor_id: entry.actor_id,
        kind: entry.kind,
        key: entry.key.clone(),
        value: entry.value.clone(),
        num_value: entry.num_value,
        max_value: entry.max_value,
        float_value: entry.float_value,
        float_min: entry.float_min,
        float_max: entry.float_max,
        unit: entry.unit.clone(),
        source_turn: entry.source_turn,
        updated_at: entry.updated_at,
    }
}

pub fn build_state_block(state: &[GameStateEntry], actors: &[GameActor]) -> String {
    let session_state: Vec<_> = state.iter().map(to_state_entry).collect();
    state_build_block(&session_state, actors)
}

pub fn build_state_block_annotated(
    state: &[GameStateEntry],
    actors: &[GameActor],
    annotations: &HashMap<(Option<i64>, String), String>,
) -> String {
    let session_state: Vec<_> = state.iter().map(to_state_entry).collect();
    state_build_block_annotated(&session_state, actors, annotations)
}

pub async fn apply_state_changes(
    pool: &SqlitePool,
    game_id: i64,
    turn_id: i64,
    changes: &[StateChangeRequest],
    actors: &[GameActor],
    current: &[GameStateEntry],
) -> AppResult<Vec<AppliedStateChange>> {
    let session_state: Vec<_> = current.iter().map(to_state_entry).collect();
    let plan = plan_state_changes(changes, actors, &session_state);
    let mut id_map: HashMap<i64, i64> = HashMap::new();
    for (idx, vivify) in plan.vivify.iter().enumerate() {
        let temp_id = -(idx as i64 + 1);
        let actor_id = insert_npc_actor(pool, game_id, &vivify.name).await?;
        id_map.insert(temp_id, actor_id);
    }

    let now = chrono::Utc::now().to_rfc3339();
    for mutation in &plan.mutations {
        persist_state_mutation(
            pool,
            "game_state_entries",
            "game_id",
            game_id,
            "source_turn",
            turn_id,
            &now,
            mutation,
            &id_map,
        )
        .await?;
    }
    Ok(plan.audit)
}

async fn insert_npc_actor(pool: &SqlitePool, game_id: i64, name: &str) -> AppResult<i64> {
    let now = chrono::Utc::now().to_rfc3339();
    sqlx::query(
        "INSERT INTO game_actors (game_id, role, name, description, skills, created_at, updated_at) VALUES (?1,'npc',?2,'','{}',?3,?3)",
    )
    .bind(game_id)
    .bind(name)
    .bind(&now)
    .execute(pool)
    .await?;
    let id: i64 = sqlx::query_scalar(
        "SELECT id FROM game_actors WHERE game_id = ?1 AND name = ?2 ORDER BY id DESC LIMIT 1",
    )
    .bind(game_id)
    .bind(name)
    .fetch_one(pool)
    .await?;
    Ok(id)
}

pub async fn revert_turn_state_changes(
    pool: &SqlitePool,
    game_id: i64,
    turn_id: i64,
    changes: &[AppliedStateChange],
    actors: &[GameActor],
) -> AppResult<()> {
    let mutations = plan_revert_changes(changes, actors);
    let now = chrono::Utc::now().to_rfc3339();
    for mutation in mutations {
        persist_state_revert(
            pool,
            "game_state_entries",
            "game_id",
            game_id,
            &now,
            &mutation,
        )
        .await?;
    }
    let _ = turn_id;
    Ok(())
}
