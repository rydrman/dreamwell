use std::collections::HashMap;

use dreamwell_state::{
    build_state_block as state_build_block,
    build_state_block_annotated as state_build_block_annotated, plan_revert_changes,
    plan_state_changes, state_kind_str, EntryMutation, RevertMutation,
};
use dreamwell_types::{AppliedStateChange, GameActor, GameStateEntry, StateChangeRequest};
use sqlx::SqlitePool;

use crate::error::AppResult;

pub use dreamwell_state::{skill_modifier, validate_skill};

pub fn build_state_block(state: &[GameStateEntry], actors: &[GameActor]) -> String {
    state_build_block(state, actors)
}

/// State block with per-entry authoring notes keyed by `(actor_id, key)`.
pub fn build_state_block_annotated(
    state: &[GameStateEntry],
    actors: &[GameActor],
    annotations: &HashMap<(Option<i64>, String), String>,
) -> String {
    state_build_block_annotated(state, actors, annotations)
}

pub async fn apply_state_changes(
    pool: &SqlitePool,
    game_id: i64,
    turn_id: i64,
    changes: &[StateChangeRequest],
    actors: &[GameActor],
    current: &[GameStateEntry],
) -> AppResult<Vec<AppliedStateChange>> {
    let plan = plan_state_changes(changes, actors, current);
    let mut id_map: HashMap<i64, i64> = HashMap::new();
    for (idx, vivify) in plan.vivify.iter().enumerate() {
        let temp_id = -(idx as i64 + 1);
        let actor_id = insert_npc_actor(pool, game_id, &vivify.name).await?;
        id_map.insert(temp_id, actor_id);
    }

    let now = chrono::Utc::now().to_rfc3339();
    for mutation in &plan.mutations {
        persist_mutation(pool, game_id, turn_id, &now, mutation, &id_map).await?;
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

fn map_actor_id(actor_id: Option<i64>, id_map: &HashMap<i64, i64>) -> Option<i64> {
    actor_id.map(|id| {
        if id < 0 {
            id_map.get(&id).copied().unwrap_or(id)
        } else {
            id
        }
    })
}

async fn persist_mutation(
    pool: &SqlitePool,
    game_id: i64,
    source_id: i64,
    now: &str,
    mutation: &EntryMutation,
    id_map: &HashMap<i64, i64>,
) -> AppResult<()> {
    match mutation {
        EntryMutation::Insert {
            actor_id,
            kind,
            key,
            value,
            num_value,
            max_value,
        } => {
            let actor_id = map_actor_id(*actor_id, id_map);
            let kind_str = state_kind_str(*kind);
            if num_value.is_some() {
                sqlx::query(
                    "INSERT INTO game_state_entries (game_id, actor_id, kind, key, value, num_value, max_value, source_turn, updated_at) VALUES (?1,?2,?3,?4,'',?5,?6,?7,?8)",
                )
                .bind(game_id)
                .bind(actor_id)
                .bind(kind_str)
                .bind(key)
                .bind(num_value)
                .bind(max_value)
                .bind(source_id)
                .bind(now)
                .execute(pool)
                .await?;
            } else {
                sqlx::query(
                    "INSERT INTO game_state_entries (game_id, actor_id, kind, key, value, source_turn, updated_at) VALUES (?1,?2,?3,?4,?5,?6,?7)",
                )
                .bind(game_id)
                .bind(actor_id)
                .bind(kind_str)
                .bind(key)
                .bind(value)
                .bind(source_id)
                .bind(now)
                .execute(pool)
                .await?;
            }
        }
        EntryMutation::UpdateNumeric {
            entry_id,
            num_value,
            max_value,
        } => {
            sqlx::query(
                "UPDATE game_state_entries SET num_value=?1, max_value=?2, source_turn=?3, updated_at=?4 WHERE id=?5",
            )
            .bind(num_value)
            .bind(max_value)
            .bind(source_id)
            .bind(now)
            .bind(entry_id)
            .execute(pool)
            .await?;
        }
        EntryMutation::UpdateText { entry_id, value } => {
            sqlx::query(
                "UPDATE game_state_entries SET value=?1, source_turn=?2, updated_at=?3 WHERE id=?4",
            )
            .bind(value)
            .bind(source_id)
            .bind(now)
            .bind(entry_id)
            .execute(pool)
            .await?;
        }
        EntryMutation::UpdateKind { entry_id, kind } => {
            sqlx::query("UPDATE game_state_entries SET kind=?1, updated_at=?2 WHERE id=?3")
                .bind(state_kind_str(*kind))
                .bind(now)
                .bind(entry_id)
                .execute(pool)
                .await?;
        }
        EntryMutation::Delete { entry_id } => {
            sqlx::query("DELETE FROM game_state_entries WHERE id = ?1")
                .bind(entry_id)
                .execute(pool)
                .await?;
        }
    }
    Ok(())
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
        persist_revert(pool, game_id, &now, &mutation).await?;
    }
    let _ = turn_id;
    Ok(())
}

async fn persist_revert(
    pool: &SqlitePool,
    game_id: i64,
    now: &str,
    mutation: &RevertMutation,
) -> AppResult<()> {
    match mutation {
        RevertMutation::RestoreNumeric {
            actor_id,
            kind,
            key,
            num_value,
        } => {
            sqlx::query(
                "UPDATE game_state_entries SET num_value=?1, source_turn=-1, updated_at=?2 WHERE game_id=?3 AND actor_id IS ?4 AND kind=?5 AND key=?6",
            )
            .bind(num_value)
            .bind(now)
            .bind(game_id)
            .bind(actor_id)
            .bind(state_kind_str(*kind))
            .bind(key)
            .execute(pool)
            .await?;
        }
        RevertMutation::RestoreText {
            actor_id,
            kind,
            key,
            value,
        } => {
            sqlx::query(
                "UPDATE game_state_entries SET value=?1, source_turn=-1, updated_at=?2 WHERE game_id=?3 AND actor_id IS ?4 AND kind=?5 AND key=?6",
            )
            .bind(value)
            .bind(now)
            .bind(game_id)
            .bind(actor_id)
            .bind(state_kind_str(*kind))
            .bind(key)
            .execute(pool)
            .await?;
        }
        RevertMutation::DeleteByKey {
            actor_id,
            kind,
            key,
        } => {
            sqlx::query(
                "DELETE FROM game_state_entries WHERE game_id=?1 AND actor_id IS ?2 AND kind=?3 AND key=?4",
            )
            .bind(game_id)
            .bind(actor_id)
            .bind(state_kind_str(*kind))
            .bind(key)
            .execute(pool)
            .await?;
        }
    }
    Ok(())
}
