use std::collections::HashMap;

use dreamwell_state::{state_kind_str, EntryMutation, RevertMutation};
use sqlx::SqlitePool;

use crate::error::AppResult;

#[allow(clippy::too_many_arguments)]
pub async fn persist_state_mutation(
    pool: &SqlitePool,
    table: &str,
    scope_col: &str,
    scope_id: i64,
    source_col: &str,
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
            float_value,
            float_min,
            float_max,
            unit,
        } => {
            let actor_id = map_actor_id(*actor_id, id_map);
            let kind_str = state_kind_str(*kind);
            let sql = format!(
                "INSERT INTO {table} ({scope_col}, actor_id, kind, key, value, float_value, float_min, float_max, unit, {source_col}, updated_at) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11)"
            );
            sqlx::query(&sql)
                .bind(scope_id)
                .bind(actor_id)
                .bind(kind_str)
                .bind(key)
                .bind(value)
                .bind(float_value)
                .bind(float_min)
                .bind(float_max)
                .bind(unit)
                .bind(source_id)
                .bind(now)
                .execute(pool)
                .await?;
        }
        EntryMutation::UpdateMeasurement {
            entry_id,
            float_value,
            float_min,
            float_max,
            unit,
            clear,
        } => {
            if *clear {
                let sql = format!(
                    "UPDATE {table} SET float_value=NULL, float_min=NULL, float_max=NULL, unit=NULL, value='', {source_col}=?1, updated_at=?2 WHERE id=?3"
                );
                sqlx::query(&sql)
                    .bind(source_id)
                    .bind(now)
                    .bind(entry_id)
                    .execute(pool)
                    .await?;
            } else {
                let sql = format!(
                    "UPDATE {table} SET float_value=COALESCE(?1, float_value), float_min=?2, float_max=?3, unit=COALESCE(?4, unit), {source_col}=?5, updated_at=?6 WHERE id=?7"
                );
                sqlx::query(&sql)
                    .bind(float_value)
                    .bind(float_min)
                    .bind(float_max)
                    .bind(unit)
                    .bind(source_id)
                    .bind(now)
                    .bind(entry_id)
                    .execute(pool)
                    .await?;
            }
        }
        EntryMutation::UpdateSequence { entry_id, value } => {
            let sql =
                format!("UPDATE {table} SET value=?1, {source_col}=?2, updated_at=?3 WHERE id=?4");
            sqlx::query(&sql)
                .bind(value)
                .bind(source_id)
                .bind(now)
                .bind(entry_id)
                .execute(pool)
                .await?;
        }
        EntryMutation::UpdateText { entry_id, value } => {
            let sql =
                format!("UPDATE {table} SET value=?1, {source_col}=?2, updated_at=?3 WHERE id=?4");
            sqlx::query(&sql)
                .bind(value)
                .bind(source_id)
                .bind(now)
                .bind(entry_id)
                .execute(pool)
                .await?;
        }
        EntryMutation::UpdateKind { entry_id, kind } => {
            let sql = format!("UPDATE {table} SET kind=?1, updated_at=?2 WHERE id=?3");
            sqlx::query(&sql)
                .bind(state_kind_str(*kind))
                .bind(now)
                .bind(entry_id)
                .execute(pool)
                .await?;
        }
        EntryMutation::Delete { entry_id } => {
            sqlx::query(&format!("DELETE FROM {table} WHERE id = ?1"))
                .bind(entry_id)
                .execute(pool)
                .await?;
        }
    }
    Ok(())
}

pub async fn persist_state_revert(
    pool: &SqlitePool,
    table: &str,
    scope_col: &str,
    scope_id: i64,
    now: &str,
    mutation: &RevertMutation,
) -> AppResult<()> {
    match mutation {
        RevertMutation::RestoreStructured {
            actor_id,
            kind,
            key,
            value,
        } => {
            let sql = format!(
                "UPDATE {table} SET value=?1, {scope_col}=?2, updated_at=?3 WHERE {scope_col}=?2 AND actor_id IS ?4 AND kind=?5 AND key=?6"
            );
            sqlx::query(&sql)
                .bind(value)
                .bind(scope_id)
                .bind(now)
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
            let sql = format!(
                "UPDATE {table} SET value=?1, updated_at=?2 WHERE {scope_col}=?3 AND actor_id IS ?4 AND kind=?5 AND key=?6"
            );
            sqlx::query(&sql)
                .bind(value)
                .bind(now)
                .bind(scope_id)
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
            let sql = format!(
                "DELETE FROM {table} WHERE {scope_col}=?1 AND actor_id IS ?2 AND kind=?3 AND key=?4"
            );
            sqlx::query(&sql)
                .bind(scope_id)
                .bind(actor_id)
                .bind(state_kind_str(*kind))
                .bind(key)
                .execute(pool)
                .await?;
        }
        RevertMutation::RestoreMeasurement {
            actor_id,
            key,
            float_value,
            float_min,
            float_max,
            unit,
        } => {
            let update_sql = format!(
                "UPDATE {table} SET float_value=?1, float_min=?2, float_max=?3, unit=?4, value='', updated_at=?5 WHERE {scope_col}=?6 AND actor_id IS ?7 AND kind='measurement' AND key=?8"
            );
            let result = sqlx::query(&update_sql)
                .bind(float_value)
                .bind(float_min)
                .bind(float_max)
                .bind(unit)
                .bind(now)
                .bind(scope_id)
                .bind(actor_id)
                .bind(key)
                .execute(pool)
                .await?;
            if result.rows_affected() == 0 && float_value.is_some() {
                let insert_sql = format!(
                    "INSERT INTO {table} ({scope_col}, actor_id, kind, key, value, float_value, float_min, float_max, unit, source_turn, updated_at) VALUES (?1,?2,'measurement',?3,'',?4,?5,?6,?7,-1,?8)"
                );
                sqlx::query(&insert_sql)
                    .bind(scope_id)
                    .bind(actor_id)
                    .bind(key)
                    .bind(float_value)
                    .bind(float_min)
                    .bind(float_max)
                    .bind(unit)
                    .bind(now)
                    .execute(pool)
                    .await?;
            }
        }
    }
    Ok(())
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
