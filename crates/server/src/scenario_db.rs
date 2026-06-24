use chrono::Utc;
use dreamwell_types::{normalize_game_traits, Scenario, ScenarioCreate, ScenarioUpdate};
use sqlx::SqlitePool;

use crate::db::parse_dt;
use crate::error::{AppError, AppResult};

pub async fn list_scenarios(pool: &SqlitePool) -> AppResult<Vec<Scenario>> {
    let rows = sqlx::query_as::<_, ScenarioRow>(
        "SELECT id, title, premise, setting, gm_style, opening_message, opening_guidance, pc_name, pc_description, pc_initial_state_json, traits, character_id, rules_blocks, objective, setup_text, trait_defs, cast_json, pc_options_json, state_schema_json, win_condition_json, content_flags_json, source_meta_json, scenario_triggers_json, game_elements_json, created_at, updated_at FROM scenarios ORDER BY updated_at DESC",
    )
    .fetch_all(pool)
    .await?;
    rows.into_iter().map(scenario_from_row).collect()
}

pub async fn get_scenario(pool: &SqlitePool, id: i64) -> AppResult<Scenario> {
    let row = sqlx::query_as::<_, ScenarioRow>(
        "SELECT id, title, premise, setting, gm_style, opening_message, opening_guidance, pc_name, pc_description, pc_initial_state_json, traits, character_id, rules_blocks, objective, setup_text, trait_defs, cast_json, pc_options_json, state_schema_json, win_condition_json, content_flags_json, source_meta_json, scenario_triggers_json, game_elements_json, created_at, updated_at FROM scenarios WHERE id = ?1",
    )
    .bind(id)
    .fetch_optional(pool)
    .await?
    .ok_or_else(|| AppError::not_found("Scenario not found"))?;
    scenario_from_row(row)
}

pub async fn create_scenario(pool: &SqlitePool, payload: ScenarioCreate) -> AppResult<Scenario> {
    let now = Utc::now().to_rfc3339();
    let jsons = scenario_json_fields(&payload)?;
    let id = sqlx::query_scalar::<_, i64>(
        "INSERT INTO scenarios (title, premise, setting, gm_style, opening_message, opening_guidance, pc_name, pc_description, pc_initial_state_json, traits, character_id, rules_blocks, objective, setup_text, trait_defs, cast_json, pc_options_json, state_schema_json, win_condition_json, content_flags_json, source_meta_json, scenario_triggers_json, game_elements_json, created_at, updated_at) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15,?16,?17,?18,?19,?20,?21,?22,?23,?24,?24) RETURNING id",
    )
    .bind(&payload.title)
    .bind(&payload.premise)
    .bind(&payload.setting)
    .bind(&payload.gm_style)
    .bind(&payload.opening_message)
    .bind(&payload.opening_guidance)
    .bind(&payload.pc_name)
    .bind(&payload.pc_description)
    .bind(&jsons.pc_initial_state)
    .bind(&jsons.traits)
    .bind(payload.character_id)
    .bind(&jsons.rules_blocks)
    .bind(&payload.objective)
    .bind(&payload.setup_text)
    .bind(&jsons.trait_defs)
    .bind(&jsons.cast)
    .bind(&jsons.pc_options)
    .bind(&jsons.state_schema)
    .bind(&jsons.win_condition)
    .bind(&jsons.content_flags)
    .bind(&jsons.source_meta)
    .bind(&jsons.scenario_triggers)
    .bind(&jsons.game_elements)
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
        opening_guidance: payload
            .opening_guidance
            .unwrap_or(existing.opening_guidance),
        pc_name: payload.pc_name.unwrap_or(existing.pc_name),
        pc_description: payload.pc_description.unwrap_or(existing.pc_description),
        pc_initial_state: payload
            .pc_initial_state
            .unwrap_or(existing.pc_initial_state),
        traits: payload
            .traits
            .map(normalize_game_traits)
            .unwrap_or(existing.traits),
        character_id: payload.character_id.unwrap_or(existing.character_id),
        rules_blocks: payload.rules_blocks.unwrap_or(existing.rules_blocks),
        objective: payload.objective.unwrap_or(existing.objective),
        setup_text: payload.setup_text.unwrap_or(existing.setup_text),
        trait_defs: payload.trait_defs.unwrap_or(existing.trait_defs),
        cast: payload.cast.unwrap_or(existing.cast),
        pc_options: payload.pc_options.unwrap_or(existing.pc_options),
        state_schema: payload.state_schema.unwrap_or(existing.state_schema),
        win_condition: payload.win_condition.unwrap_or(existing.win_condition),
        content_flags: payload.content_flags.unwrap_or(existing.content_flags),
        source_meta: payload.source_meta.unwrap_or(existing.source_meta),
        scenario_triggers: payload
            .scenario_triggers
            .unwrap_or(existing.scenario_triggers),
        game_elements: payload.game_elements.unwrap_or(existing.game_elements),
        updated_at: Utc::now(),
        ..existing
    };
    let traits_json = serde_json::to_string(&updated.traits).unwrap_or_else(|_| "{}".to_string());
    let rules_blocks = json_string(&updated.rules_blocks);
    let trait_defs = json_string(&updated.trait_defs);
    let cast = json_string(&updated.cast);
    let pc_options = json_string(&updated.pc_options);
    let state_schema = json_string(&updated.state_schema);
    let pc_initial_state = json_string(&updated.pc_initial_state);
    let win_condition = optional_json_string(&updated.win_condition);
    let content_flags = json_string(&updated.content_flags);
    let source_meta = optional_json_string(&updated.source_meta);
    let scenario_triggers = json_string(&updated.scenario_triggers);
    let game_elements = json_string(&updated.game_elements);
    sqlx::query(
        "UPDATE scenarios SET title=?1, premise=?2, setting=?3, gm_style=?4, opening_message=?5, opening_guidance=?6, pc_name=?7, pc_description=?8, pc_initial_state_json=?9, traits=?10, character_id=?11, rules_blocks=?12, objective=?13, setup_text=?14, trait_defs=?15, cast_json=?16, pc_options_json=?17, state_schema_json=?18, win_condition_json=?19, content_flags_json=?20, source_meta_json=?21, scenario_triggers_json=?22, game_elements_json=?23, updated_at=?24 WHERE id=?25",
    )
    .bind(&updated.title)
    .bind(&updated.premise)
    .bind(&updated.setting)
    .bind(&updated.gm_style)
    .bind(&updated.opening_message)
    .bind(&updated.opening_guidance)
    .bind(&updated.pc_name)
    .bind(&updated.pc_description)
    .bind(&pc_initial_state)
    .bind(&traits_json)
    .bind(updated.character_id)
    .bind(&rules_blocks)
    .bind(&updated.objective)
    .bind(&updated.setup_text)
    .bind(&trait_defs)
    .bind(&cast)
    .bind(&pc_options)
    .bind(&state_schema)
    .bind(&win_condition)
    .bind(&content_flags)
    .bind(&source_meta)
    .bind(&scenario_triggers)
    .bind(&game_elements)
    .bind(updated.updated_at.to_rfc3339())
    .bind(id)
    .execute(pool)
    .await?;
    get_scenario(pool, id).await
}

pub async fn delete_scenario(pool: &SqlitePool, id: i64) -> AppResult<()> {
    let exists: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM scenarios WHERE id = ?1)")
        .bind(id)
        .fetch_one(pool)
        .await?;
    if !exists {
        return Err(AppError::not_found("Scenario not found"));
    }
    // Games snapshot scenario text at creation; drop the link so deletion succeeds.
    sqlx::query("UPDATE games SET scenario_id = NULL WHERE scenario_id = ?1")
        .bind(id)
        .execute(pool)
        .await?;
    sqlx::query("DELETE FROM scenarios WHERE id = ?1")
        .bind(id)
        .execute(pool)
        .await?;
    Ok(())
}

struct ScenarioJsonFields {
    traits: String,
    pc_initial_state: String,
    rules_blocks: String,
    trait_defs: String,
    cast: String,
    pc_options: String,
    state_schema: String,
    win_condition: Option<String>,
    content_flags: String,
    source_meta: Option<String>,
    scenario_triggers: String,
    game_elements: String,
}

fn scenario_json_fields(payload: &ScenarioCreate) -> AppResult<ScenarioJsonFields> {
    Ok(ScenarioJsonFields {
        traits: serde_json::to_string(&normalize_game_traits(payload.traits.clone()))
            .unwrap_or_else(|_| "{}".to_string()),
        pc_initial_state: json_string(&payload.pc_initial_state),
        rules_blocks: json_string(&payload.rules_blocks),
        trait_defs: json_string(&payload.trait_defs),
        cast: json_string(&payload.cast),
        pc_options: json_string(&payload.pc_options),
        state_schema: json_string(&payload.state_schema),
        win_condition: optional_json_string(&payload.win_condition),
        content_flags: json_string(&payload.content_flags),
        source_meta: optional_json_string(&payload.source_meta),
        scenario_triggers: json_string(&payload.scenario_triggers),
        game_elements: json_string(&payload.game_elements),
    })
}

fn json_string<T: serde::Serialize>(value: &T) -> String {
    serde_json::to_string(value).unwrap_or_else(|_| "[]".to_string())
}

fn optional_json_string<T: serde::Serialize>(value: &Option<T>) -> Option<String> {
    value.as_ref().map(|v| json_string(v))
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
        opening_guidance: row.opening_guidance,
        pc_name: row.pc_name,
        pc_description: row.pc_description,
        pc_initial_state: parse_json(&row.pc_initial_state_json),
        traits: normalize_game_traits(traits),
        character_id: row.character_id,
        rules_blocks: parse_json(&row.rules_blocks),
        objective: row.objective,
        setup_text: row.setup_text,
        trait_defs: parse_json(&row.trait_defs),
        cast: parse_json(&row.cast_json),
        pc_options: parse_json(&row.pc_options_json),
        state_schema: parse_json(&row.state_schema_json),
        win_condition: row
            .win_condition_json
            .as_deref()
            .and_then(|s| serde_json::from_str(s).ok()),
        content_flags: parse_json_or_default(&row.content_flags_json),
        source_meta: row
            .source_meta_json
            .as_deref()
            .and_then(|s| serde_json::from_str(s).ok()),
        scenario_triggers: parse_json(&row.scenario_triggers_json),
        game_elements: parse_json(&row.game_elements_json),
        created_at: parse_dt(&row.created_at)?,
        updated_at: parse_dt(&row.updated_at)?,
    })
}

fn parse_json<T: serde::de::DeserializeOwned + Default>(json: &str) -> T {
    serde_json::from_str(json).unwrap_or_default()
}

fn parse_json_or_default<T: serde::de::DeserializeOwned + Default>(json: &str) -> T {
    serde_json::from_str(json).unwrap_or_default()
}

#[derive(sqlx::FromRow)]
struct ScenarioRow {
    id: i64,
    title: String,
    premise: String,
    setting: String,
    gm_style: String,
    opening_message: String,
    opening_guidance: String,
    pc_name: String,
    pc_description: String,
    pc_initial_state_json: String,
    traits: String,
    character_id: Option<i64>,
    rules_blocks: String,
    objective: String,
    setup_text: String,
    trait_defs: String,
    cast_json: String,
    pc_options_json: String,
    state_schema_json: String,
    win_condition_json: Option<String>,
    content_flags_json: String,
    source_meta_json: Option<String>,
    scenario_triggers_json: String,
    game_elements_json: String,
    created_at: String,
    updated_at: String,
}

#[cfg(test)]
mod tests {
    use dreamwell_types::{RulesBlock, ScenarioCreate};

    use super::*;

    async fn test_pool() -> sqlx::SqlitePool {
        let pool = sqlx::SqlitePool::connect("sqlite::memory:")
            .await
            .expect("pool");
        sqlx::migrate!("./migrations")
            .run(&pool)
            .await
            .expect("migrate");
        pool
    }

    #[tokio::test]
    async fn delete_scenario_clears_game_reference() {
        let pool = test_pool().await;
        let created = create_scenario(&pool, ScenarioCreate::default())
            .await
            .expect("create");
        let now = Utc::now().to_rfc3339();
        sqlx::query(
            "INSERT INTO games (title, premise, setting, gm_style, opening_message, scenario_id, created_at, updated_at) VALUES ('g','','','','',?1,?2,?2)",
        )
        .bind(created.id)
        .bind(&now)
        .execute(&pool)
        .await
        .expect("game");

        delete_scenario(&pool, created.id).await.expect("delete");

        let scenario_id: Option<i64> =
            sqlx::query_scalar("SELECT scenario_id FROM games WHERE title = 'g'")
                .fetch_one(&pool)
                .await
                .expect("game row");
        assert!(scenario_id.is_none());
    }

    #[tokio::test]
    async fn scenario_round_trips_iw_rules_blocks() {
        let pool = test_pool().await;
        let payload = ScenarioCreate {
            title: "IW Test".into(),
            rules_blocks: vec![RulesBlock {
                name: "Game Mechanics".into(),
                content: "Roll 1d6".into(),
            }],
            ..Default::default()
        };
        let created = create_scenario(&pool, payload).await.expect("create");
        assert_eq!(created.rules_blocks.len(), 1);
        assert_eq!(created.rules_blocks[0].name, "Game Mechanics");

        let fetched = get_scenario(&pool, created.id).await.expect("get");
        assert_eq!(fetched.rules_blocks, created.rules_blocks);
    }

    #[tokio::test]
    async fn scenario_round_trips_opening_guidance() {
        let pool = test_pool().await;
        let payload = ScenarioCreate {
            title: "Guided Start".into(),
            opening_guidance: "Start with a warm welcome.".into(),
            ..Default::default()
        };
        let created = create_scenario(&pool, payload).await.expect("create");
        assert_eq!(created.opening_guidance, "Start with a warm welcome.");

        let fetched = get_scenario(&pool, created.id).await.expect("get");
        assert_eq!(fetched.opening_guidance, created.opening_guidance);
    }
}
