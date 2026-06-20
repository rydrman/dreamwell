use chrono::Utc;
use dreamwell_types::{normalize_game_traits, Scenario, ScenarioCreate, ScenarioUpdate};
use sqlx::SqlitePool;

use crate::db::parse_dt;
use crate::error::{AppError, AppResult};

pub async fn list_scenarios(pool: &SqlitePool) -> AppResult<Vec<Scenario>> {
    let rows = sqlx::query_as::<_, ScenarioRow>(
        "SELECT id, title, premise, setting, gm_style, opening_message, pc_name, pc_description, traits, character_id, created_at, updated_at FROM scenarios ORDER BY updated_at DESC",
    )
    .fetch_all(pool)
    .await?;
    rows.into_iter().map(scenario_from_row).collect()
}

pub async fn get_scenario(pool: &SqlitePool, id: i64) -> AppResult<Scenario> {
    let row = sqlx::query_as::<_, ScenarioRow>(
        "SELECT id, title, premise, setting, gm_style, opening_message, pc_name, pc_description, traits, character_id, created_at, updated_at FROM scenarios WHERE id = ?1",
    )
    .bind(id)
    .fetch_optional(pool)
    .await?
    .ok_or_else(|| AppError::not_found("Scenario not found"))?;
    scenario_from_row(row)
}

pub async fn create_scenario(pool: &SqlitePool, payload: ScenarioCreate) -> AppResult<Scenario> {
    let now = Utc::now().to_rfc3339();
    let traits_json = serde_json::to_string(&normalize_game_traits(payload.traits.clone()))
        .unwrap_or_else(|_| "{}".to_string());
    let id = sqlx::query_scalar::<_, i64>(
        "INSERT INTO scenarios (title, premise, setting, gm_style, opening_message, pc_name, pc_description, traits, character_id, created_at, updated_at) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?10) RETURNING id",
    )
    .bind(&payload.title)
    .bind(&payload.premise)
    .bind(&payload.setting)
    .bind(&payload.gm_style)
    .bind(&payload.opening_message)
    .bind(&payload.pc_name)
    .bind(&payload.pc_description)
    .bind(&traits_json)
    .bind(payload.character_id)
    .bind(&now)
    .fetch_one(pool)
    .await?;
    get_scenario(pool, id).await
}

pub async fn update_scenario(
    pool: &SqlitePool,
    id: i64,
    payload: ScenarioUpdate,
) -> AppResult<Scenario> {
    let existing = get_scenario(pool, id).await?;
    let updated = Scenario {
        title: payload.title.unwrap_or(existing.title),
        premise: payload.premise.unwrap_or(existing.premise),
        setting: payload.setting.unwrap_or(existing.setting),
        gm_style: payload.gm_style.unwrap_or(existing.gm_style),
        opening_message: payload.opening_message.unwrap_or(existing.opening_message),
        pc_name: payload.pc_name.unwrap_or(existing.pc_name),
        pc_description: payload.pc_description.unwrap_or(existing.pc_description),
        traits: payload
            .traits
            .map(normalize_game_traits)
            .unwrap_or(existing.traits),
        character_id: payload.character_id.unwrap_or(existing.character_id),
        updated_at: Utc::now(),
        ..existing
    };
    let traits_json = serde_json::to_string(&updated.traits).unwrap_or_else(|_| "{}".to_string());
    sqlx::query(
        "UPDATE scenarios SET title=?1, premise=?2, setting=?3, gm_style=?4, opening_message=?5, pc_name=?6, pc_description=?7, traits=?8, character_id=?9, updated_at=?10 WHERE id=?11",
    )
    .bind(&updated.title)
    .bind(&updated.premise)
    .bind(&updated.setting)
    .bind(&updated.gm_style)
    .bind(&updated.opening_message)
    .bind(&updated.pc_name)
    .bind(&updated.pc_description)
    .bind(&traits_json)
    .bind(updated.character_id)
    .bind(updated.updated_at.to_rfc3339())
    .bind(id)
    .execute(pool)
    .await?;
    get_scenario(pool, id).await
}

pub async fn delete_scenario(pool: &SqlitePool, id: i64) -> AppResult<()> {
    let result = sqlx::query("DELETE FROM scenarios WHERE id = ?1")
        .bind(id)
        .execute(pool)
        .await?;
    if result.rows_affected() == 0 {
        return Err(AppError::not_found("Scenario not found"));
    }
    Ok(())
}

fn scenario_from_row(row: ScenarioRow) -> AppResult<Scenario> {
    let traits = serde_json::from_str(&row.traits).unwrap_or_default();
    Ok(Scenario {
        id: row.id,
        title: row.title,
        premise: row.premise,
        setting: row.setting,
        gm_style: row.gm_style,
        opening_message: row.opening_message,
        pc_name: row.pc_name,
        pc_description: row.pc_description,
        traits: normalize_game_traits(traits),
        character_id: row.character_id,
        created_at: parse_dt(&row.created_at)?,
        updated_at: parse_dt(&row.updated_at)?,
    })
}

#[derive(sqlx::FromRow)]
struct ScenarioRow {
    id: i64,
    title: String,
    premise: String,
    setting: String,
    gm_style: String,
    opening_message: String,
    pc_name: String,
    pc_description: String,
    traits: String,
    character_id: Option<i64>,
    created_at: String,
    updated_at: String,
}
