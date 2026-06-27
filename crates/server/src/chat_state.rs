use std::collections::HashMap;

use dreamwell_state::{
    build_state_block as state_build_block, plan_revert_changes, plan_state_changes,
};
use dreamwell_types::{
    AppliedStateChange, ChatActor, ChatStateEntry, SessionActor, StateChangeRequest, StateEntry,
};
use sqlx::SqlitePool;

use crate::error::AppResult;
use crate::state_persist::{persist_state_mutation, persist_state_revert};

pub fn to_session_actor(actor: &ChatActor) -> SessionActor {
    SessionActor {
        id: actor.id,
        game_id: actor.chat_id,
        role: actor.role.clone(),
        name: actor.name.clone(),
        description: actor.description.clone(),
        skills: actor.skills.clone(),
        sort_order: actor.sort_order,
        created_at: actor.created_at,
        updated_at: actor.updated_at,
    }
}

pub fn to_state_entry(entry: &ChatStateEntry) -> StateEntry {
    StateEntry {
        id: entry.id,
        game_id: entry.chat_id,
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
        source_turn: entry.source_message_id,
        updated_at: entry.updated_at,
    }
}

pub fn build_state_block(state: &[ChatStateEntry], actors: &[ChatActor]) -> String {
    let session_actors: Vec<_> = actors.iter().map(to_session_actor).collect();
    let session_state: Vec<_> = state.iter().map(to_state_entry).collect();
    state_build_block(&session_state, &session_actors)
}

pub async fn apply_state_changes(
    pool: &SqlitePool,
    chat_id: i64,
    message_id: i64,
    changes: &[StateChangeRequest],
    actors: &[ChatActor],
    current: &[ChatStateEntry],
) -> AppResult<Vec<AppliedStateChange>> {
    let session_actors: Vec<_> = actors.iter().map(to_session_actor).collect();
    let session_state: Vec<_> = current.iter().map(to_state_entry).collect();
    let plan = plan_state_changes(changes, &session_actors, &session_state);

    let mut id_map: HashMap<i64, i64> = HashMap::new();
    for (idx, vivify) in plan.vivify.iter().enumerate() {
        let temp_id = -(idx as i64 + 1);
        let actor_id = insert_npc_actor(pool, chat_id, &vivify.name).await?;
        id_map.insert(temp_id, actor_id);
    }

    let now = chrono::Utc::now().to_rfc3339();
    for mutation in &plan.mutations {
        persist_state_mutation(
            pool,
            "chat_state_entries",
            "chat_id",
            chat_id,
            "source_message_id",
            message_id,
            &now,
            mutation,
            &id_map,
        )
        .await?;
    }
    Ok(plan.audit)
}

async fn insert_npc_actor(pool: &SqlitePool, chat_id: i64, name: &str) -> AppResult<i64> {
    let now = chrono::Utc::now().to_rfc3339();
    sqlx::query(
        "INSERT INTO chat_actors (chat_id, role, name, description, skills, created_at, updated_at) VALUES (?1,'npc',?2,'','{}',?3,?3)",
    )
    .bind(chat_id)
    .bind(name)
    .bind(&now)
    .execute(pool)
    .await?;
    let id: i64 = sqlx::query_scalar(
        "SELECT id FROM chat_actors WHERE chat_id = ?1 AND name = ?2 ORDER BY id DESC LIMIT 1",
    )
    .bind(chat_id)
    .bind(name)
    .fetch_one(pool)
    .await?;
    Ok(id)
}

pub async fn revert_message_state_changes(
    pool: &SqlitePool,
    chat_id: i64,
    changes: &[AppliedStateChange],
    actors: &[ChatActor],
) -> AppResult<()> {
    let session_actors: Vec<_> = actors.iter().map(to_session_actor).collect();
    let mutations = plan_revert_changes(changes, &session_actors);
    let now = chrono::Utc::now().to_rfc3339();
    for mutation in mutations {
        persist_state_revert(
            pool,
            "chat_state_entries",
            "chat_id",
            chat_id,
            &now,
            &mutation,
        )
        .await?;
    }
    Ok(())
}
