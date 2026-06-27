use std::collections::HashMap;

use chrono::Utc;
use dreamwell_state::state_kind_str;
use dreamwell_types::{
    normalize_game_traits, substitute_macros, AppliedStateChange, ElementInstances, EngineMode,
    Game, GameActor, GameActorUpdate, GameCreate, GameDetail, GameElementsConfig, GameScene,
    GameStateEntry, GameStateEntryUpdate, GameTurn, GameTurnCheck, GameTurnSystemRoll, GameUpdate,
    Job, JobType, MacroContext, MechanicalResult, ResolutionSystem, RulesBlock, ScenarioTrigger,
    StateKind, SubmitTurnRequest, TrackedVarDef, TraitDef, TurnEditField, TurnObservability,
};
use sqlx::SqlitePool;

use crate::db::{get_job, job_type_str, parse_dt, JobRow};
use crate::error::{AppError, AppResult};
use dreamwell_types::CHAT_ARCHIVE_RETENTION_DAYS;

const DEFAULT_SKILLS: &str = r#"{"Finesse":0,"Force":0,"Flair":0,"Focus":0,"Sway":0}"#;
const OPENING_TURN_SORT_ORDER: i64 = -1;
const TURN_COLUMNS: &str = "id, game_id, sort_order, player_action, guidance_notes, phase, scene_beats, prose, thought_content, thought_duration_ms, thought_in_progress, state_changes, is_opening, plan_json, mechanical_results_json, observability_json, created_at, updated_at";

fn turn_select(where_clause: &str) -> String {
    format!("SELECT {TURN_COLUMNS} FROM game_turns {where_clause}")
}
const GAME_COLUMNS: &str = "id, title, premise, setting, gm_style, opening_message, character_id, scenario_id, resolution_system, modifier_min, modifier_max, merge_resolve_scene, step_mode, engine_mode, model_checks, model_resolve, model_prose, rules_blocks_json, state_schema_json, win_condition_json, scenario_triggers_json, trait_defs_json, game_elements_json, element_instances_json, created_at, updated_at, archived_at";

pub async fn purge_expired_archived_games(pool: &SqlitePool) -> AppResult<u64> {
    let cutoff = (Utc::now() - chrono::Duration::days(CHAT_ARCHIVE_RETENTION_DAYS)).to_rfc3339();
    let result =
        sqlx::query("DELETE FROM games WHERE archived_at IS NOT NULL AND archived_at < ?1")
            .bind(&cutoff)
            .execute(pool)
            .await?;
    Ok(result.rows_affected())
}

pub async fn list_games(pool: &SqlitePool) -> AppResult<Vec<Game>> {
    purge_expired_archived_games(pool).await?;
    let rows = sqlx::query_as::<_, GameRow>(&format!(
        "SELECT {GAME_COLUMNS} FROM games WHERE archived_at IS NULL ORDER BY updated_at DESC"
    ))
    .fetch_all(pool)
    .await?;
    let mut games = Vec::with_capacity(rows.len());
    for row in rows {
        games.push(game_from_row(pool, row).await?);
    }
    Ok(games)
}

pub async fn list_archived_games(pool: &SqlitePool) -> AppResult<Vec<Game>> {
    purge_expired_archived_games(pool).await?;
    let rows = sqlx::query_as::<_, GameRow>(&format!(
        "SELECT {GAME_COLUMNS} FROM games WHERE archived_at IS NOT NULL ORDER BY archived_at DESC, title ASC"
    ))
    .fetch_all(pool)
    .await?;
    let mut games = Vec::with_capacity(rows.len());
    for row in rows {
        games.push(game_from_row(pool, row).await?);
    }
    Ok(games)
}

pub async fn get_game(pool: &SqlitePool, id: i64) -> AppResult<Game> {
    let row = fetch_game_row(pool, id, false)
        .await?
        .ok_or_else(|| AppError::not_found("Game not found"))?;
    game_from_row(pool, row).await
}

async fn fetch_game_row(
    pool: &SqlitePool,
    id: i64,
    include_archived: bool,
) -> AppResult<Option<GameRow>> {
    let sql = if include_archived {
        format!("SELECT {GAME_COLUMNS} FROM games WHERE id = ?1")
    } else {
        format!("SELECT {GAME_COLUMNS} FROM games WHERE id = ?1 AND archived_at IS NULL")
    };
    sqlx::query_as::<_, GameRow>(&sql)
        .bind(id)
        .fetch_optional(pool)
        .await
        .map_err(Into::into)
}

pub async fn get_game_detail(pool: &SqlitePool, id: i64) -> AppResult<GameDetail> {
    let game = get_game(pool, id).await?;
    let actors = list_actors(pool, id).await?;
    let state = list_state_entries(pool, id).await?;
    let turns = list_turns(pool, id).await?;
    let scenes = list_scenes(pool, id).await?;
    Ok(GameDetail {
        game,
        actors,
        state,
        turns,
        scenes,
    })
}

async fn game_from_row(pool: &SqlitePool, row: GameRow) -> AppResult<Game> {
    let active_job = if row.archived_at.is_none() {
        get_active_game_job(pool, row.id).await?
    } else {
        None
    };
    let queued_jobs: i64 = if row.archived_at.is_none() {
        sqlx::query_scalar(
            "SELECT COUNT(*) FROM generation_jobs WHERE game_id = ?1 AND status = 'queued'",
        )
        .bind(row.id)
        .fetch_one(pool)
        .await?
    } else {
        0
    };
    Ok(Game {
        id: row.id,
        title: row.title,
        premise: row.premise,
        setting: row.setting,
        gm_style: row.gm_style,
        opening_message: row.opening_message,
        character_id: row.character_id,
        scenario_id: row.scenario_id,
        resolution_system: parse_resolution_system(&row.resolution_system),
        modifier_min: row.modifier_min,
        modifier_max: row.modifier_max,
        merge_resolve_scene: row.merge_resolve_scene != 0,
        step_mode: row.step_mode != 0,
        engine_mode: EngineMode::from_db(&row.engine_mode),
        model_checks: row.model_checks,
        model_resolve: row.model_resolve,
        model_prose: row.model_prose,
        rules_blocks: parse_json_vec::<RulesBlock>(&row.rules_blocks_json),
        state_schema: parse_json_vec::<TrackedVarDef>(&row.state_schema_json),
        win_condition: row
            .win_condition_json
            .as_deref()
            .and_then(|s| serde_json::from_str(s).ok()),
        scenario_triggers: parse_json_vec::<ScenarioTrigger>(&row.scenario_triggers_json),
        trait_defs: parse_json_vec::<TraitDef>(&row.trait_defs_json),
        game_elements: parse_json_default::<GameElementsConfig>(&row.game_elements_json),
        element_instances: parse_json_default::<ElementInstances>(&row.element_instances_json),
        created_at: parse_dt(&row.created_at)?,
        updated_at: parse_dt(&row.updated_at)?,
        archived_at: row.archived_at.as_deref().map(parse_dt).transpose()?,
        active_job,
        queued_jobs,
    })
}

fn parse_resolution_system(s: &str) -> ResolutionSystem {
    match s {
        "pbta_2d6" => ResolutionSystem::Pbta2d6,
        _ => ResolutionSystem::Pbta2d6,
    }
}

fn resolution_system_str(system: ResolutionSystem) -> &'static str {
    match system {
        ResolutionSystem::Pbta2d6 => "pbta_2d6",
    }
}

pub async fn create_game(pool: &SqlitePool, payload: GameCreate) -> AppResult<GameDetail> {
    let now = Utc::now().to_rfc3339();
    let rules_blocks_json = json_string(&payload.rules_blocks);
    let state_schema_json = json_string(&payload.state_schema);
    let win_condition_json = optional_json_string(&payload.win_condition);
    let scenario_triggers_json = json_string(&payload.scenario_triggers);
    let trait_defs_json = json_string(&payload.trait_defs);
    let game_elements_json = json_string(&payload.game_elements);
    let element_instances_json = {
        let mut instances = if payload.element_instances.deck_piles.is_empty()
            && payload.element_instances.board_positions.is_empty()
        {
            crate::game_mechanics::init_element_instances(&payload.game_elements)
        } else {
            payload.element_instances.clone()
        };
        if !payload.game_elements.boards.is_empty() {
            instances
                .board_positions
                .entry("pc".to_string())
                .or_insert(0);
        }
        json_string(&instances)
    };
    let engine_mode = payload.engine_mode.as_db();
    let id = sqlx::query_scalar::<_, i64>(
        "INSERT INTO games (title, premise, setting, gm_style, opening_message, character_id, scenario_id, engine_mode, rules_blocks_json, state_schema_json, win_condition_json, scenario_triggers_json, trait_defs_json, game_elements_json, element_instances_json, created_at, updated_at) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15,?16,?16) RETURNING id",
    )
    .bind(&payload.title)
    .bind(&payload.premise)
    .bind(&payload.setting)
    .bind(&payload.gm_style)
    .bind(&payload.opening_message)
    .bind(payload.character_id)
    .bind(payload.scenario_id)
    .bind(engine_mode)
    .bind(&rules_blocks_json)
    .bind(&state_schema_json)
    .bind(&win_condition_json)
    .bind(&scenario_triggers_json)
    .bind(&trait_defs_json)
    .bind(&game_elements_json)
    .bind(&element_instances_json)
    .bind(&now)
    .fetch_one(pool)
    .await?;

    let pc_traits = if payload.trait_defs.is_empty() {
        normalize_game_traits(payload.pc_traits.clone())
    } else {
        payload.pc_traits.clone()
    };
    let pc_traits_json =
        serde_json::to_string(&pc_traits).unwrap_or_else(|_| DEFAULT_SKILLS.to_string());

    let actor_now = now.clone();
    sqlx::query(
        "INSERT INTO game_actors (game_id, role, name, description, skills, created_at, updated_at) VALUES (?1,'pc',?2,?3,?4,?5,?5)",
    )
    .bind(id)
    .bind(&payload.pc_name)
    .bind(&payload.pc_description)
    .bind(&pc_traits_json)
    .bind(&actor_now)
    .execute(pool)
    .await?;

    let actor_id: i64 =
        sqlx::query_scalar("SELECT id FROM game_actors WHERE game_id = ?1 AND role = 'pc' LIMIT 1")
            .bind(id)
            .fetch_one(pool)
            .await?;

    for (sort_order, npc) in payload.invited_cast.iter().enumerate() {
        let npc_traits_json =
            serde_json::to_string(&npc.traits).unwrap_or_else(|_| "{}".to_string());
        sqlx::query(
            "INSERT INTO game_actors (game_id, role, name, description, skills, sort_order, created_at, updated_at) VALUES (?1,'npc',?2,?3,?4,?5,?6,?6)",
        )
        .bind(id)
        .bind(&npc.name)
        .bind(&npc.content)
        .bind(&npc_traits_json)
        .bind((sort_order + 1) as i64)
        .bind(&actor_now)
        .execute(pool)
        .await?;
    }

    seed_scenario_state(pool, id, actor_id, &payload, &now).await?;

    sqlx::query(
        "INSERT INTO game_scenes (game_id, title, start_turn, sort_order, created_at, updated_at) VALUES (?1,'Opening Scene',0,0,?2,?2)",
    )
    .bind(id)
    .bind(&now)
    .execute(pool)
    .await?;

    let macro_ctx = MacroContext {
        char_name: payload.pc_name.as_str(),
        user_name: "User",
        persona: "",
        description: payload.pc_description.as_str(),
        personality: "",
        scenario: payload.premise.as_str(),
        first_message: payload.opening_message.as_str(),
        setup_vars: &payload.setup_var_values,
    };
    let opening = substitute_macros(payload.opening_message.trim(), &macro_ctx);
    let opening_guidance = substitute_macros(payload.opening_guidance.trim(), &macro_ctx);
    if payload.opening_as_player_action {
        if !opening.is_empty() || !opening_guidance.is_empty() {
            prepare_submit_turn(
                pool,
                id,
                &SubmitTurnRequest {
                    player_action: opening,
                    guidance_notes: opening_guidance,
                },
            )
            .await?;
        }
    } else {
        seed_opening_turn(pool, id, &opening, &now).await?;
        if !opening_guidance.is_empty() {
            prepare_submit_turn(
                pool,
                id,
                &SubmitTurnRequest {
                    player_action: String::new(),
                    guidance_notes: opening_guidance,
                },
            )
            .await?;
        }
    }

    get_game_detail(pool, id).await
}

async fn seed_scenario_state(
    pool: &SqlitePool,
    game_id: i64,
    pc_actor_id: i64,
    payload: &GameCreate,
    now: &str,
) -> AppResult<()> {
    for def in &payload.state_schema {
        let value = payload
            .setup_var_values
            .get(&def.key)
            .cloned()
            .unwrap_or_else(|| def.initial_value.clone());
        let num_value = def.initial_num.or_else(|| value.parse::<i64>().ok());
        let kind = state_kind_str(def.kind);
        let target = def.target.trim();
        let (actor_id, target_is_named_actor) = if target.is_empty()
            || target.eq_ignore_ascii_case("world")
        {
            (None, false)
        } else if target.eq_ignore_ascii_case("pc") || target.eq_ignore_ascii_case("user") {
            (Some(pc_actor_id), true)
        } else {
            let resolved: Option<i64> = sqlx::query_scalar(
                "SELECT id FROM game_actors WHERE game_id = ?1 AND role = 'npc' AND name = ?2 LIMIT 1",
            )
            .bind(game_id)
            .bind(target)
            .fetch_optional(pool)
            .await?;
            (resolved, true)
        };
        // A target that names a specific actor we couldn't resolve is skipped rather
        // than silently leaking onto world scope.
        if target_is_named_actor && actor_id.is_none() {
            continue;
        }
        if def.kind == StateKind::Measurement {
            let float_value = def
                .initial_float
                .or_else(|| def.initial_num.map(|n| n as f64))
                .or_else(|| value.parse::<f64>().ok());
            let float_max = def.initial_max.map(|n| n as f64);
            sqlx::query(
                "INSERT INTO game_state_entries (game_id, actor_id, kind, key, value, float_value, float_min, float_max, unit, source_turn, updated_at) VALUES (?1,?2,?3,?4,'',?5,NULL,?6,?7,-1,?8)",
            )
            .bind(game_id)
            .bind(actor_id)
            .bind(kind)
            .bind(&def.key)
            .bind(float_value)
            .bind(float_max)
            .bind(def.unit.as_deref())
            .bind(now)
            .execute(pool)
            .await?;
        } else if def.kind == StateKind::Sequence {
            let items = def.sequence_items.clone().unwrap_or_default();
            let Some(payload) = dreamwell_types::SequencePayload::new(
                items,
                def.initial_num,
                def.sequence_loop.unwrap_or(false),
            ) else {
                continue;
            };
            sqlx::query(
                "INSERT INTO game_state_entries (game_id, actor_id, kind, key, value, source_turn, updated_at) VALUES (?1,?2,?3,?4,?5,-1,?6)",
            )
            .bind(game_id)
            .bind(actor_id)
            .bind(kind)
            .bind(&def.key)
            .bind(payload.encode())
            .bind(now)
            .execute(pool)
            .await?;
        } else {
            sqlx::query(
                "INSERT INTO game_state_entries (game_id, actor_id, kind, key, value, num_value, source_turn, updated_at) VALUES (?1,?2,?3,?4,?5,?6,-1,?7)",
            )
            .bind(game_id)
            .bind(actor_id)
            .bind(kind)
            .bind(&def.key)
            .bind(&value)
            .bind(num_value)
            .bind(now)
            .execute(pool)
            .await?;
        }
    }
    for (key, value) in &payload.setup_var_values {
        if payload.state_schema.iter().any(|d| d.key == *key) {
            continue;
        }
        sqlx::query(
            "INSERT INTO game_state_entries (game_id, actor_id, kind, key, value, source_turn, updated_at) VALUES (?1,NULL,'variable',?2,?3,-1,?4)",
        )
        .bind(game_id)
        .bind(key)
        .bind(value)
        .bind(now)
        .execute(pool)
        .await?;
    }
    Ok(())
}

async fn seed_opening_turn(
    pool: &SqlitePool,
    game_id: i64,
    opening_message: &str,
    now: &str,
) -> AppResult<()> {
    if opening_message.trim().is_empty() {
        return Ok(());
    }
    sqlx::query(
        "INSERT INTO game_turns (game_id, sort_order, player_action, phase, scene_beats, prose, state_changes, is_opening, created_at, updated_at) VALUES (?1,?2,'','done','[]',?3,'[]',1,?4,?4)",
    )
    .bind(game_id)
    .bind(OPENING_TURN_SORT_ORDER)
    .bind(opening_message.trim())
    .bind(now)
    .execute(pool)
    .await?;
    Ok(())
}

async fn opening_turn_id(pool: &SqlitePool, game_id: i64) -> AppResult<Option<i64>> {
    Ok(sqlx::query_scalar(
        "SELECT id FROM game_turns WHERE game_id = ?1 AND is_opening = 1 LIMIT 1",
    )
    .bind(game_id)
    .fetch_optional(pool)
    .await?)
}

async fn sync_opening_turn(
    pool: &SqlitePool,
    game_id: i64,
    opening_message: &str,
) -> AppResult<()> {
    let trimmed = opening_message.trim();
    if let Some(turn_id) = opening_turn_id(pool, game_id).await? {
        if trimmed.is_empty() {
            sqlx::query("DELETE FROM game_turns WHERE id = ?1")
                .bind(turn_id)
                .execute(pool)
                .await?;
        } else {
            let now = Utc::now().to_rfc3339();
            sqlx::query(
                "UPDATE game_turns SET prose = ?1, phase = 'done', updated_at = ?2 WHERE id = ?3",
            )
            .bind(trimmed)
            .bind(&now)
            .bind(turn_id)
            .execute(pool)
            .await?;
        }
    } else if !trimmed.is_empty() {
        seed_opening_turn(pool, game_id, trimmed, &Utc::now().to_rfc3339()).await?;
    }
    Ok(())
}

pub async fn update_game(pool: &SqlitePool, id: i64, payload: GameUpdate) -> AppResult<Game> {
    let existing = get_game(pool, id).await?;
    let opening_message_updated = payload.opening_message.is_some();
    let updated = Game {
        title: payload.title.unwrap_or(existing.title),
        premise: payload.premise.unwrap_or(existing.premise),
        setting: payload.setting.unwrap_or(existing.setting),
        gm_style: payload.gm_style.unwrap_or(existing.gm_style),
        opening_message: payload.opening_message.unwrap_or(existing.opening_message),
        resolution_system: payload
            .resolution_system
            .unwrap_or(existing.resolution_system),
        modifier_min: payload.modifier_min.unwrap_or(existing.modifier_min),
        modifier_max: payload.modifier_max.unwrap_or(existing.modifier_max),
        merge_resolve_scene: payload
            .merge_resolve_scene
            .unwrap_or(existing.merge_resolve_scene),
        step_mode: payload.step_mode.unwrap_or(existing.step_mode),
        engine_mode: payload.engine_mode.unwrap_or(existing.engine_mode),
        model_checks: payload.model_checks.unwrap_or(existing.model_checks),
        model_resolve: payload.model_resolve.unwrap_or(existing.model_resolve),
        model_prose: payload.model_prose.unwrap_or(existing.model_prose),
        updated_at: Utc::now(),
        ..existing
    };
    sqlx::query(
        "UPDATE games SET title=?1, premise=?2, setting=?3, gm_style=?4, opening_message=?5, resolution_system=?6, modifier_min=?7, modifier_max=?8, merge_resolve_scene=?9, step_mode=?10, engine_mode=?11, model_checks=?12, model_resolve=?13, model_prose=?14, updated_at=?15 WHERE id=?16",
    )
    .bind(&updated.title)
    .bind(&updated.premise)
    .bind(&updated.setting)
    .bind(&updated.gm_style)
    .bind(&updated.opening_message)
    .bind(resolution_system_str(updated.resolution_system))
    .bind(updated.modifier_min)
    .bind(updated.modifier_max)
    .bind(updated.merge_resolve_scene as i64)
    .bind(updated.step_mode as i64)
    .bind(updated.engine_mode.as_db())
    .bind(&updated.model_checks)
    .bind(&updated.model_resolve)
    .bind(&updated.model_prose)
    .bind(updated.updated_at.to_rfc3339())
    .bind(id)
    .execute(pool)
    .await?;
    if opening_message_updated {
        sync_opening_turn(pool, id, &updated.opening_message).await?;
    }
    get_game(pool, id).await
}

pub async fn archive_game(pool: &SqlitePool, id: i64) -> AppResult<()> {
    let exists = fetch_game_row(pool, id, false).await?;
    if exists.is_none() {
        return Err(AppError::not_found("Game not found"));
    }
    let now = Utc::now().to_rfc3339();
    sqlx::query("UPDATE games SET archived_at = ?1 WHERE id = ?2 AND archived_at IS NULL")
        .bind(&now)
        .bind(id)
        .execute(pool)
        .await?;
    Ok(())
}

pub async fn restore_game(pool: &SqlitePool, id: i64) -> AppResult<Game> {
    let exists = fetch_game_row(pool, id, true).await?;
    if exists
        .as_ref()
        .and_then(|row| row.archived_at.as_deref())
        .is_none()
    {
        return Err(AppError::not_found("Archived game not found"));
    }
    let now = Utc::now().to_rfc3339();
    sqlx::query("UPDATE games SET archived_at = NULL, updated_at = ?1 WHERE id = ?2")
        .bind(&now)
        .bind(id)
        .execute(pool)
        .await?;
    get_game(pool, id).await
}

pub async fn permanently_delete_game(pool: &SqlitePool, id: i64) -> AppResult<()> {
    let result = sqlx::query("DELETE FROM games WHERE id = ?1 AND archived_at IS NOT NULL")
        .bind(id)
        .execute(pool)
        .await?;
    if result.rows_affected() == 0 {
        return Err(AppError::not_found("Archived game not found"));
    }
    Ok(())
}

pub async fn list_active_jobs_for_game(pool: &SqlitePool, game_id: i64) -> AppResult<Vec<Job>> {
    let rows = sqlx::query_as::<_, JobRow>(
        &format!("SELECT {} FROM generation_jobs WHERE game_id = ?1 AND status IN ('queued','running') ORDER BY created_at ASC", crate::db::JOB_COLUMNS),
    )
    .bind(game_id)
    .fetch_all(pool)
    .await?;
    Ok(rows.into_iter().map(Into::into).collect())
}

pub async fn touch_game(pool: &SqlitePool, game_id: i64) -> AppResult<()> {
    let now = Utc::now().to_rfc3339();
    sqlx::query("UPDATE games SET updated_at = ?1 WHERE id = ?2")
        .bind(&now)
        .bind(game_id)
        .execute(pool)
        .await?;
    Ok(())
}

async fn list_actors(pool: &SqlitePool, game_id: i64) -> AppResult<Vec<GameActor>> {
    let rows = sqlx::query_as::<_, ActorRow>(
        "SELECT id, game_id, role, name, description, skills, sort_order, created_at, updated_at FROM game_actors WHERE game_id = ?1 ORDER BY sort_order ASC, id ASC",
    )
    .bind(game_id)
    .fetch_all(pool)
    .await?;
    rows.into_iter().map(actor_from_row).collect()
}

fn actor_from_row(row: ActorRow) -> AppResult<GameActor> {
    let skills: HashMap<String, i64> = serde_json::from_str(&row.skills).unwrap_or_default();
    Ok(GameActor {
        id: row.id,
        game_id: row.game_id,
        role: row.role,
        name: row.name,
        description: row.description,
        skills,
        sort_order: row.sort_order,
        created_at: parse_dt(&row.created_at)?,
        updated_at: parse_dt(&row.updated_at)?,
    })
}

pub async fn update_actor(
    pool: &SqlitePool,
    game_id: i64,
    actor_id: i64,
    payload: GameActorUpdate,
) -> AppResult<GameActor> {
    let existing = sqlx::query_as::<_, ActorRow>(
        "SELECT id, game_id, role, name, description, skills, sort_order, created_at, updated_at FROM game_actors WHERE id = ?1 AND game_id = ?2",
    )
    .bind(actor_id)
    .bind(game_id)
    .fetch_optional(pool)
    .await?
    .ok_or_else(|| AppError::not_found("Actor not found"))?;
    let mut actor = actor_from_row(existing)?;
    if let Some(name) = payload.name {
        actor.name = name;
    }
    if let Some(description) = payload.description {
        actor.description = description;
    }
    if let Some(skills) = payload.skills {
        actor.skills = skills;
    }
    actor.updated_at = Utc::now();
    let skills_json = serde_json::to_string(&actor.skills).unwrap_or_else(|_| "{}".to_string());
    sqlx::query(
        "UPDATE game_actors SET name=?1, description=?2, skills=?3, updated_at=?4 WHERE id=?5",
    )
    .bind(&actor.name)
    .bind(&actor.description)
    .bind(&skills_json)
    .bind(actor.updated_at.to_rfc3339())
    .bind(actor_id)
    .execute(pool)
    .await?;
    Ok(actor)
}

async fn list_state_entries(pool: &SqlitePool, game_id: i64) -> AppResult<Vec<GameStateEntry>> {
    let rows = sqlx::query_as::<_, StateRow>(
        "SELECT id, game_id, actor_id, kind, key, value, num_value, max_value, float_value, float_min, float_max, unit, source_turn, updated_at FROM game_state_entries WHERE game_id = ?1 ORDER BY kind ASC, key ASC",
    )
    .bind(game_id)
    .fetch_all(pool)
    .await?;
    rows.into_iter().map(state_from_row).collect()
}

fn state_from_row(row: StateRow) -> AppResult<GameStateEntry> {
    Ok(GameStateEntry {
        id: row.id,
        game_id: row.game_id,
        actor_id: row.actor_id,
        kind: parse_state_kind(&row.kind),
        key: row.key,
        value: row.value,
        num_value: row.num_value,
        max_value: row.max_value,
        float_value: row.float_value,
        float_min: row.float_min,
        float_max: row.float_max,
        unit: row.unit,
        source_turn: row.source_turn,
        updated_at: parse_dt(&row.updated_at)?,
    })
}

fn parse_state_kind(s: &str) -> StateKind {
    match s {
        "condition" => StateKind::Condition,
        "variable" | "fact" => StateKind::Variable,
        "measurement" | "resource" | "gauge" => StateKind::Measurement,
        "sequence" | "clock" => StateKind::Sequence,
        _ => StateKind::Variable,
    }
}

pub async fn update_state_entry(
    pool: &SqlitePool,
    game_id: i64,
    entry_id: i64,
    payload: GameStateEntryUpdate,
) -> AppResult<GameStateEntry> {
    let row = sqlx::query_as::<_, StateRow>(
        "SELECT id, game_id, actor_id, kind, key, value, num_value, max_value, float_value, float_min, float_max, unit, source_turn, updated_at FROM game_state_entries WHERE id = ?1 AND game_id = ?2",
    )
    .bind(entry_id)
    .bind(game_id)
    .fetch_optional(pool)
    .await?
    .ok_or_else(|| AppError::not_found("State entry not found"))?;
    let mut entry = state_from_row(row)?;
    if let Some(value) = payload.value {
        entry.value = value;
    }
    if let Some(num) = payload.num_value {
        entry.num_value = Some(num);
    }
    if let Some(max) = payload.max_value {
        entry.max_value = Some(max);
    }
    entry.updated_at = Utc::now();
    sqlx::query(
        "UPDATE game_state_entries SET value=?1, num_value=?2, max_value=?3, source_turn=-1, updated_at=?4 WHERE id=?5",
    )
    .bind(&entry.value)
    .bind(entry.num_value)
    .bind(entry.max_value)
    .bind(entry.updated_at.to_rfc3339())
    .bind(entry_id)
    .execute(pool)
    .await?;
    Ok(entry)
}

async fn list_turns(pool: &SqlitePool, game_id: i64) -> AppResult<Vec<GameTurn>> {
    let rows = sqlx::query_as::<_, TurnRow>(&turn_select(
        "WHERE game_id = ?1 ORDER BY sort_order ASC, id ASC",
    ))
    .bind(game_id)
    .fetch_all(pool)
    .await?;
    let mut turns = Vec::with_capacity(rows.len());
    for row in rows {
        turns.push(turn_from_row(pool, row).await?);
    }
    Ok(turns)
}

async fn turn_generation_error(pool: &SqlitePool, turn_id: i64) -> AppResult<Option<String>> {
    Ok(sqlx::query_scalar::<_, String>(
        "SELECT error FROM generation_jobs WHERE turn_id = ?1 AND status = 'failed' AND error IS NOT NULL ORDER BY completed_at DESC LIMIT 1",
    )
    .bind(turn_id)
    .fetch_optional(pool)
    .await?)
}

async fn turn_from_row(pool: &SqlitePool, row: TurnRow) -> AppResult<GameTurn> {
    let checks = list_checks_for_turn(pool, row.id).await?;
    let system_rolls = list_system_rolls_for_turn(pool, row.id).await?;
    let generation_error = turn_generation_error(pool, row.id).await?;
    let plan = row
        .plan_json
        .as_deref()
        .and_then(|s| serde_json::from_str(s).ok());
    Ok(GameTurn {
        id: row.id,
        game_id: row.game_id,
        sort_order: row.sort_order,
        player_action: row.player_action,
        guidance_notes: row.guidance_notes,
        phase: row.phase,
        scene_beats: parse_string_array(&row.scene_beats),
        prose: row.prose,
        thought_content: row.thought_content,
        thought_duration_ms: row.thought_duration_ms,
        thought_in_progress: row.thought_in_progress != 0,
        state_changes: parse_state_changes(&row.state_changes),
        checks,
        system_rolls,
        plan,
        mechanical_results: parse_json_vec::<MechanicalResult>(&row.mechanical_results_json),
        observability: parse_json_default::<TurnObservability>(&row.observability_json),
        is_opening: row.is_opening != 0,
        generation_error,
        created_at: parse_dt(&row.created_at)?,
        updated_at: parse_dt(&row.updated_at)?,
    })
}

async fn list_checks_for_turn(pool: &SqlitePool, turn_id: i64) -> AppResult<Vec<GameTurnCheck>> {
    let rows = sqlx::query_as::<_, CheckRow>(
        "SELECT id, turn_id, label, skill, modifier, stakes, justification, dice_expr, seed, rolls, total, tier, margin, sort_order, created_at FROM game_turn_checks WHERE turn_id = ?1 ORDER BY sort_order ASC, id ASC",
    )
    .bind(turn_id)
    .fetch_all(pool)
    .await?;
    rows.into_iter().map(check_from_row).collect()
}

fn check_from_row(row: CheckRow) -> AppResult<GameTurnCheck> {
    let rolls: Vec<i64> = serde_json::from_str(&row.rolls).unwrap_or_default();
    let tier = if row.tier.is_empty() {
        None
    } else {
        crate::game_resolution::parse_tier(&row.tier)
    };
    Ok(GameTurnCheck {
        id: row.id,
        turn_id: row.turn_id,
        label: row.label,
        skill: row.skill,
        modifier: row.modifier,
        stakes: row.stakes,
        justification: row.justification,
        dice_expr: row.dice_expr,
        seed: row.seed,
        rolls,
        total: row.total,
        tier,
        margin: row.margin,
        sort_order: row.sort_order,
        created_at: parse_dt(&row.created_at)?,
    })
}

fn parse_string_array(json: &str) -> Vec<String> {
    serde_json::from_str::<Vec<String>>(json).unwrap_or_default()
}

fn parse_state_changes(json: &str) -> Vec<AppliedStateChange> {
    serde_json::from_str(json).unwrap_or_default()
}

async fn list_scenes(pool: &SqlitePool, game_id: i64) -> AppResult<Vec<GameScene>> {
    let rows = sqlx::query_as::<_, SceneRow>(
        "SELECT id, game_id, title, summary, summary_valid, summary_at, start_turn, sort_order, created_at, updated_at FROM game_scenes WHERE game_id = ?1 ORDER BY sort_order ASC, id ASC",
    )
    .bind(game_id)
    .fetch_all(pool)
    .await?;
    rows.into_iter().map(scene_from_row).collect()
}

fn scene_from_row(row: SceneRow) -> AppResult<GameScene> {
    Ok(GameScene {
        id: row.id,
        game_id: row.game_id,
        title: row.title,
        summary: row.summary,
        summary_valid: row.summary_valid != 0,
        summary_at: row.summary_at.as_deref().map(parse_dt).transpose()?,
        start_turn: row.start_turn,
        sort_order: row.sort_order,
        created_at: parse_dt(&row.created_at)?,
        updated_at: parse_dt(&row.updated_at)?,
    })
}

pub async fn get_active_game_job(pool: &SqlitePool, game_id: i64) -> AppResult<Option<Job>> {
    let row = sqlx::query_as::<_, JobRow>(
        &format!("SELECT {} FROM generation_jobs WHERE game_id = ?1 AND status IN ('queued','running') ORDER BY created_at ASC LIMIT 1", crate::db::JOB_COLUMNS),
    )
    .bind(game_id)
    .fetch_optional(pool)
    .await?;
    Ok(row.map(Into::into))
}

pub async fn has_active_turn_job(pool: &SqlitePool, turn_id: i64) -> AppResult<bool> {
    let count = sqlx::query_scalar::<_, i64>(
        "SELECT COUNT(*) FROM generation_jobs WHERE turn_id = ?1 AND status IN ('queued','running')",
    )
    .bind(turn_id)
    .fetch_one(pool)
    .await?;
    Ok(count > 0)
}

pub async fn enqueue_game_job(
    pool: &SqlitePool,
    job_type: JobType,
    game_id: i64,
    turn_id: Option<i64>,
    guidance_notes: String,
) -> AppResult<Job> {
    let now = Utc::now().to_rfc3339();
    let position: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM generation_jobs WHERE status = 'queued'")
            .fetch_one(pool)
            .await?;
    let id = sqlx::query_scalar::<_, i64>(
        "INSERT INTO generation_jobs (job_type, game_id, turn_id, guidance_notes, status, position, created_at) VALUES (?1,?2,?3,?4,'queued',?5,?6) RETURNING id",
    )
    .bind(job_type_str(job_type))
    .bind(game_id)
    .bind(turn_id)
    .bind(&guidance_notes)
    .bind(position + 1)
    .bind(&now)
    .fetch_one(pool)
    .await?;
    get_job(pool, id).await
}

pub async fn prepare_submit_turn(
    pool: &SqlitePool,
    game_id: i64,
    payload: &SubmitTurnRequest,
) -> AppResult<(GameTurn, Job)> {
    if payload.player_action.trim().is_empty() && payload.guidance_notes.trim().is_empty() {
        return Err(AppError::bad_request(
            "Turn requires a player action or GM guidance",
        ));
    }
    let _game = get_game(pool, game_id).await?;
    if let Some(active) = get_active_game_job(pool, game_id).await? {
        return Err(AppError::bad_request(format!(
            "Game already has an active job (id {})",
            active.id
        )));
    }
    let sort_order: i64 = sqlx::query_scalar(
        "SELECT COALESCE(MAX(sort_order), -1) + 1 FROM game_turns WHERE game_id = ?1",
    )
    .bind(game_id)
    .fetch_one(pool)
    .await?;
    let now = Utc::now().to_rfc3339();
    let turn_id = sqlx::query_scalar::<_, i64>(
        "INSERT INTO game_turns (game_id, sort_order, player_action, guidance_notes, phase, is_opening, created_at, updated_at) VALUES (?1,?2,?3,?4,'pending',0,?5,?5) RETURNING id",
    )
    .bind(game_id)
    .bind(sort_order)
    .bind(&payload.player_action)
    .bind(&payload.guidance_notes)
    .bind(&now)
    .fetch_one(pool)
    .await?;
    let turn = get_turn(pool, game_id, turn_id).await?;
    let job = enqueue_game_job(
        pool,
        JobType::GameTurnStructuredAgent,
        game_id,
        Some(turn_id),
        payload.guidance_notes.clone(),
    )
    .await?;
    touch_game(pool, game_id).await?;
    Ok((turn, job))
}

pub async fn prepare_continue_turn(
    _pool: &SqlitePool,
    _game_id: i64,
    _turn_id: i64,
) -> AppResult<Job> {
    Err(AppError::bad_request(
        "Step mode is not supported for game turns",
    ))
}

/// Whether the most recent turn can be regenerated or retried (mirrors game UI affordances).
fn turn_can_be_regenerated(turn: &GameTurn) -> bool {
    if turn.is_opening {
        return false;
    }
    if turn.phase == "done" {
        return !turn.prose.trim().is_empty();
    }
    if turn.phase == "failed" {
        return true;
    }
    // Stuck mid-pipeline (e.g. checks/prose) after a cancelled or crashed job.
    turn.phase != "pending" && !turn.phase.ends_with("_pause")
}

pub async fn prepare_regenerate_turn(
    pool: &SqlitePool,
    game_id: i64,
    turn_id: i64,
) -> AppResult<Job> {
    let _game = get_game(pool, game_id).await?;
    let detail = get_game_detail(pool, game_id).await?;
    let turn = detail
        .turns
        .iter()
        .find(|t| t.id == turn_id)
        .cloned()
        .ok_or_else(|| AppError::not_found("Turn not found"))?;
    let Some(last) = detail.turns.last() else {
        return Err(AppError::bad_request("Game has no turns"));
    };
    if turn.id != last.id {
        return Err(AppError::bad_request(
            "Only the most recent turn can be regenerated",
        ));
    }
    if turn.is_opening {
        return Err(AppError::bad_request("Opening scene cannot be regenerated"));
    }
    if !turn_can_be_regenerated(&turn) {
        if turn.phase == "done" {
            return Err(AppError::bad_request(
                "Turn has no generated response to regenerate",
            ));
        }
        if turn.phase == "pending" {
            return Err(AppError::bad_request("Turn is still generating"));
        }
        return Err(AppError::bad_request(
            "Only completed, failed, or stuck turns can be retried",
        ));
    }
    if has_active_turn_job(pool, turn_id).await? {
        return Err(AppError::bad_request("Turn already has an active job"));
    }
    prepare_structured_agent_rerun(pool, game_id, turn_id, &turn).await
}

pub async fn rewind_game_at_turn(
    pool: &SqlitePool,
    game_id: i64,
    turn_id: i64,
    include_turn: bool,
) -> AppResult<GameDetail> {
    let detail = get_game_detail(pool, game_id).await?;
    let turn = detail
        .turns
        .iter()
        .find(|t| t.id == turn_id)
        .ok_or_else(|| AppError::not_found("Turn not found"))?;
    if include_turn && turn.is_opening {
        return Err(AppError::bad_request(
            "Cannot rewind before the opening scene",
        ));
    }

    let mut to_delete: Vec<&GameTurn> = detail
        .turns
        .iter()
        .filter(|t| {
            if t.is_opening {
                return false;
            }
            if include_turn {
                t.sort_order >= turn.sort_order
            } else {
                t.sort_order > turn.sort_order
            }
        })
        .collect();
    if to_delete.is_empty() {
        return Err(AppError::bad_request("Nothing to rewind"));
    }
    to_delete.sort_by_key(|t| std::cmp::Reverse(t.sort_order));

    for deleted in &to_delete {
        crate::game_state::revert_turn_state_changes(
            pool,
            game_id,
            deleted.id,
            &deleted.state_changes,
            &detail.actors,
        )
        .await?;
    }

    let delete_ids: Vec<i64> = to_delete.iter().map(|t| t.id).collect();
    for id in &delete_ids {
        sqlx::query("DELETE FROM game_turns WHERE id = ?1 AND game_id = ?2")
            .bind(id)
            .bind(game_id)
            .execute(pool)
            .await?;
    }

    let kept_turns: Vec<GameTurn> = detail
        .turns
        .iter()
        .filter(|t| !delete_ids.contains(&t.id))
        .cloned()
        .collect();
    let game = get_game(pool, game_id).await?;
    let instances =
        crate::game_mechanics::replay_element_instances(&game.game_elements, &kept_turns);
    update_game_element_instances(pool, game_id, &instances).await?;

    let scene_cutoff = if include_turn {
        turn.sort_order
    } else {
        turn.sort_order + 1
    };
    sqlx::query(
        "DELETE FROM game_scenes WHERE game_id = ?1 AND start_turn >= ?2 AND start_turn > 0",
    )
    .bind(game_id)
    .bind(scene_cutoff)
    .execute(pool)
    .await?;
    invalidate_scene_summaries_from(pool, game_id, scene_cutoff).await?;
    touch_game(pool, game_id).await?;
    get_game_detail(pool, game_id).await
}

pub async fn fork_game_at_turn(
    pool: &SqlitePool,
    game_id: i64,
    fork_turn_id: i64,
) -> AppResult<GameDetail> {
    let detail = get_game_detail(pool, game_id).await?;
    if get_active_game_job(pool, game_id).await?.is_some() {
        return Err(AppError::bad_request("Game has an active job"));
    }
    let fork_turn = detail
        .turns
        .iter()
        .find(|t| t.id == fork_turn_id)
        .ok_or_else(|| AppError::not_found("Turn not found"))?;
    if fork_turn.is_opening {
        return Err(AppError::bad_request("Cannot fork from the opening turn"));
    }
    if fork_turn.phase != "done" {
        return Err(AppError::bad_request("Can only fork from a completed turn"));
    }

    let kept_turns: Vec<&GameTurn> = detail
        .turns
        .iter()
        .filter(|t| t.sort_order <= fork_turn.sort_order)
        .collect();
    let mut dropped_turns: Vec<&GameTurn> = detail
        .turns
        .iter()
        .filter(|t| t.sort_order > fork_turn.sort_order)
        .collect();
    dropped_turns.sort_by_key(|t| std::cmp::Reverse(t.sort_order));

    let source = &detail.game;
    let now = Utc::now().to_rfc3339();
    let title = format!("{} (fork)", source.title.trim());
    let rules_blocks_json = json_string(&source.rules_blocks);
    let state_schema_json = json_string(&source.state_schema);
    let win_condition_json = optional_json_string(&source.win_condition);
    let scenario_triggers_json = json_string(&source.scenario_triggers);
    let trait_defs_json = json_string(&source.trait_defs);
    let game_elements_json = json_string(&source.game_elements);
    let instances = crate::game_mechanics::replay_element_instances(
        &source.game_elements,
        &kept_turns.iter().copied().cloned().collect::<Vec<_>>(),
    );
    let element_instances_json = json_string(&instances);

    let new_id = sqlx::query_scalar::<_, i64>(
        "INSERT INTO games (title, premise, setting, gm_style, opening_message, character_id, scenario_id, resolution_system, modifier_min, modifier_max, merge_resolve_scene, step_mode, engine_mode, model_checks, model_resolve, model_prose, rules_blocks_json, state_schema_json, win_condition_json, scenario_triggers_json, trait_defs_json, game_elements_json, element_instances_json, created_at, updated_at) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15,?16,?17,?18,?19,?20,?21,?22,?23,?24,?24) RETURNING id",
    )
    .bind(&title)
    .bind(&source.premise)
    .bind(&source.setting)
    .bind(&source.gm_style)
    .bind(&source.opening_message)
    .bind(source.character_id)
    .bind(source.scenario_id)
    .bind(resolution_system_str(source.resolution_system))
    .bind(source.modifier_min)
    .bind(source.modifier_max)
    .bind(source.merge_resolve_scene as i64)
    .bind(source.step_mode as i64)
    .bind(source.engine_mode.as_db())
    .bind(&source.model_checks)
    .bind(&source.model_resolve)
    .bind(&source.model_prose)
    .bind(&rules_blocks_json)
    .bind(&state_schema_json)
    .bind(&win_condition_json)
    .bind(&scenario_triggers_json)
    .bind(&trait_defs_json)
    .bind(&game_elements_json)
    .bind(&element_instances_json)
    .bind(&now)
    .fetch_one(pool)
    .await?;

    let mut actor_map: std::collections::HashMap<i64, i64> = std::collections::HashMap::new();
    for actor in &detail.actors {
        let skills = json_string(&actor.skills);
        let new_actor_id = sqlx::query_scalar::<_, i64>(
            "INSERT INTO game_actors (game_id, role, name, description, skills, sort_order, created_at, updated_at) VALUES (?1,?2,?3,?4,?5,?6,?7,?7) RETURNING id",
        )
        .bind(new_id)
        .bind(&actor.role)
        .bind(&actor.name)
        .bind(&actor.description)
        .bind(&skills)
        .bind(actor.sort_order)
        .bind(&now)
        .fetch_one(pool)
        .await?;
        actor_map.insert(actor.id, new_actor_id);
    }

    for entry in &detail.state {
        let kind = state_kind_str(entry.kind);
        sqlx::query(
            "INSERT INTO game_state_entries (game_id, actor_id, kind, key, value, num_value, max_value, float_value, float_min, float_max, unit, source_turn, updated_at) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13)",
        )
        .bind(new_id)
        .bind(entry.actor_id.and_then(|id| actor_map.get(&id).copied()))
        .bind(kind)
        .bind(&entry.key)
        .bind(&entry.value)
        .bind(entry.num_value)
        .bind(entry.max_value)
        .bind(entry.float_value)
        .bind(entry.float_min)
        .bind(entry.float_max)
        .bind(&entry.unit)
        .bind(entry.source_turn)
        .bind(&now)
        .execute(pool)
        .await?;
    }

    let new_actors = list_actors(pool, new_id).await?;
    for turn in dropped_turns {
        crate::game_state::revert_turn_state_changes(
            pool,
            new_id,
            turn.id,
            &turn.state_changes,
            &new_actors,
        )
        .await?;
    }

    for turn in kept_turns {
        clone_turn_to_game(pool, new_id, turn, &now).await?;
    }

    for scene in &detail.scenes {
        if scene.start_turn > fork_turn.sort_order {
            continue;
        }
        sqlx::query(
            "INSERT INTO game_scenes (game_id, title, summary, summary_valid, summary_at, start_turn, sort_order, created_at, updated_at) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?8)",
        )
        .bind(new_id)
        .bind(&scene.title)
        .bind(&scene.summary)
        .bind(scene.summary_valid as i64)
        .bind(scene.summary_at.map(|t| t.to_rfc3339()))
        .bind(scene.start_turn)
        .bind(scene.sort_order)
        .bind(&now)
        .execute(pool)
        .await?;
    }

    get_game_detail(pool, new_id).await
}

async fn clone_turn_to_game(
    pool: &SqlitePool,
    new_game_id: i64,
    turn: &GameTurn,
    now: &str,
) -> AppResult<()> {
    let scene_beats = json_string(&turn.scene_beats);
    let state_changes = json_string(&turn.state_changes);
    let plan_json = turn.plan.as_ref().map(json_string);
    let mechanical_results = json_string(&turn.mechanical_results);
    let observability = json_string(&turn.observability);
    let new_turn_id = sqlx::query_scalar::<_, i64>(
        "INSERT INTO game_turns (game_id, sort_order, player_action, guidance_notes, phase, scene_beats, prose, thought_content, thought_duration_ms, thought_in_progress, state_changes, is_opening, plan_json, mechanical_results_json, observability_json, created_at, updated_at) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15,?16,?16) RETURNING id",
    )
    .bind(new_game_id)
    .bind(turn.sort_order)
    .bind(&turn.player_action)
    .bind(&turn.guidance_notes)
    .bind(&turn.phase)
    .bind(&scene_beats)
    .bind(&turn.prose)
    .bind(&turn.thought_content)
    .bind(turn.thought_duration_ms)
    .bind(turn.thought_in_progress as i64)
    .bind(&state_changes)
    .bind(turn.is_opening as i64)
    .bind(plan_json)
    .bind(&mechanical_results)
    .bind(&observability)
    .bind(now)
    .fetch_one(pool)
    .await?;

    for check in &turn.checks {
        let mut copied = check.clone();
        copied.id = 0;
        copied.turn_id = new_turn_id;
        insert_turn_check(pool, new_turn_id, &copied).await?;
    }
    for roll in &turn.system_rolls {
        let mut copied = roll.clone();
        copied.id = 0;
        copied.turn_id = new_turn_id;
        insert_system_roll(pool, new_turn_id, &copied).await?;
    }
    Ok(())
}

async fn prepare_structured_agent_rerun(
    pool: &SqlitePool,
    game_id: i64,
    turn_id: i64,
    turn: &GameTurn,
) -> AppResult<Job> {
    if has_active_turn_job(pool, turn_id).await? {
        return Err(AppError::bad_request("Turn already has an active job"));
    }
    let detail = get_game_detail(pool, game_id).await?;
    let prior_turns: Vec<GameTurn> = detail
        .turns
        .iter()
        .filter(|t| t.sort_order < turn.sort_order)
        .cloned()
        .collect();
    let instances =
        crate::game_mechanics::replay_element_instances(&detail.game.game_elements, &prior_turns);
    update_game_element_instances(pool, game_id, &instances).await?;
    crate::game_state::revert_turn_state_changes(
        pool,
        game_id,
        turn_id,
        &turn.state_changes,
        &detail.actors,
    )
    .await?;
    clear_turn_checks(pool, turn_id).await?;
    clear_system_rolls(pool, turn_id).await?;
    let now = Utc::now().to_rfc3339();
    sqlx::query(
        "UPDATE game_turns SET prose='', thought_content='', thought_duration_ms=NULL, thought_in_progress=0, scene_beats='[]', state_changes='[]', plan_json=NULL, mechanical_results_json='[]', observability_json='{}', phase='pending', updated_at=?1 WHERE id=?2",
    )
    .bind(&now)
    .bind(turn_id)
    .execute(pool)
    .await?;
    enqueue_game_job(
        pool,
        JobType::GameTurnStructuredAgent,
        game_id,
        Some(turn_id),
        turn.guidance_notes.clone(),
    )
    .await
}

pub async fn get_turn(pool: &SqlitePool, game_id: i64, turn_id: i64) -> AppResult<GameTurn> {
    let row = sqlx::query_as::<_, TurnRow>(&turn_select("WHERE id = ?1 AND game_id = ?2"))
        .bind(turn_id)
        .bind(game_id)
        .fetch_optional(pool)
        .await?
        .ok_or_else(|| AppError::not_found("Turn not found"))?;
    turn_from_row(pool, row).await
}

pub async fn update_turn_phase(pool: &SqlitePool, turn_id: i64, phase: &str) -> AppResult<()> {
    let now = Utc::now().to_rfc3339();
    sqlx::query("UPDATE game_turns SET phase=?1, updated_at=?2 WHERE id=?3")
        .bind(phase)
        .bind(&now)
        .bind(turn_id)
        .execute(pool)
        .await?;
    Ok(())
}

pub async fn update_turn_prose(pool: &SqlitePool, turn_id: i64, prose: &str) -> AppResult<()> {
    update_turn_field(pool, turn_id, TurnEditField::Prose, prose).await
}

pub async fn update_turn_field(
    pool: &SqlitePool,
    turn_id: i64,
    field: TurnEditField,
    content: &str,
) -> AppResult<()> {
    let column = match field {
        TurnEditField::Prose => "prose",
        TurnEditField::PlayerAction => "player_action",
    };
    let now = Utc::now().to_rfc3339();
    let sql = format!("UPDATE game_turns SET {column}=?1, updated_at=?2 WHERE id=?3");
    sqlx::query(&sql)
        .bind(content)
        .bind(&now)
        .bind(turn_id)
        .execute(pool)
        .await?;
    Ok(())
}

pub async fn update_turn_generation(
    pool: &SqlitePool,
    turn_id: i64,
    prose: &str,
    thought_content: &str,
    thought_duration_ms: Option<i64>,
    thought_in_progress: bool,
) -> AppResult<()> {
    let now = Utc::now().to_rfc3339();
    sqlx::query(
        "UPDATE game_turns SET prose=?1, thought_content=?2, thought_duration_ms=?3, thought_in_progress=?4, updated_at=?5 WHERE id=?6",
    )
    .bind(prose)
    .bind(thought_content)
    .bind(thought_duration_ms)
    .bind(thought_in_progress as i64)
    .bind(&now)
    .bind(turn_id)
    .execute(pool)
    .await?;
    Ok(())
}

pub async fn clear_turn_thoughts(pool: &SqlitePool, turn_id: i64) -> AppResult<()> {
    let now = Utc::now().to_rfc3339();
    sqlx::query(
        "UPDATE game_turns SET thought_content='', thought_duration_ms=NULL, thought_in_progress=0, updated_at=?1 WHERE id=?2",
    )
    .bind(&now)
    .bind(turn_id)
    .execute(pool)
    .await?;
    Ok(())
}

pub async fn update_turn_state_changes(
    pool: &SqlitePool,
    turn_id: i64,
    changes: &[AppliedStateChange],
) -> AppResult<()> {
    let json = serde_json::to_string(changes).unwrap_or_else(|_| "[]".to_string());
    let now = Utc::now().to_rfc3339();
    sqlx::query("UPDATE game_turns SET state_changes=?1, updated_at=?2 WHERE id=?3")
        .bind(&json)
        .bind(&now)
        .bind(turn_id)
        .execute(pool)
        .await?;
    Ok(())
}

pub async fn insert_system_roll(
    pool: &SqlitePool,
    turn_id: i64,
    roll: &GameTurnSystemRoll,
) -> AppResult<i64> {
    let rolls = serde_json::to_string(&roll.rolls).unwrap_or_else(|_| "[]".to_string());
    let now = Utc::now().to_rfc3339();
    let id = sqlx::query_scalar::<_, i64>(
        "INSERT INTO game_turn_system_rolls (turn_id, label, dice_expr, rolls, outcome_key, outcome_summary, sort_order, created_at) VALUES (?1,?2,?3,?4,?5,?6,?7,?8) RETURNING id",
    )
    .bind(turn_id)
    .bind(&roll.label)
    .bind(&roll.dice_expr)
    .bind(&rolls)
    .bind(&roll.outcome_key)
    .bind(&roll.outcome_summary)
    .bind(roll.sort_order)
    .bind(&now)
    .fetch_one(pool)
    .await?;
    Ok(id)
}

async fn list_system_rolls_for_turn(
    pool: &SqlitePool,
    turn_id: i64,
) -> AppResult<Vec<GameTurnSystemRoll>> {
    let rows = sqlx::query_as::<_, SystemRollRow>(
        "SELECT id, turn_id, label, dice_expr, rolls, outcome_key, outcome_summary, sort_order, created_at FROM game_turn_system_rolls WHERE turn_id = ?1 ORDER BY sort_order ASC, id ASC",
    )
    .bind(turn_id)
    .fetch_all(pool)
    .await?;
    rows.into_iter().map(system_roll_from_row).collect()
}

fn system_roll_from_row(row: SystemRollRow) -> AppResult<GameTurnSystemRoll> {
    let rolls: Vec<i64> = serde_json::from_str(&row.rolls).unwrap_or_default();
    Ok(GameTurnSystemRoll {
        id: row.id,
        turn_id: row.turn_id,
        label: row.label,
        dice_expr: row.dice_expr,
        rolls,
        outcome_key: row.outcome_key,
        outcome_summary: row.outcome_summary,
        sort_order: row.sort_order,
        created_at: parse_dt(&row.created_at)?,
    })
}

fn json_string<T: serde::Serialize + ?Sized>(value: &T) -> String {
    serde_json::to_string(value).unwrap_or_else(|_| "[]".to_string())
}

fn optional_json_string<T: serde::Serialize>(value: &Option<T>) -> Option<String> {
    value.as_ref().map(|v| json_string(v))
}

fn parse_json_vec<T: serde::de::DeserializeOwned>(json: &str) -> Vec<T> {
    serde_json::from_str(json).unwrap_or_default()
}

fn parse_json_default<T: serde::de::DeserializeOwned + Default>(json: &str) -> T {
    serde_json::from_str(json).unwrap_or_default()
}

pub async fn update_game_element_instances(
    pool: &SqlitePool,
    game_id: i64,
    instances: &ElementInstances,
) -> AppResult<()> {
    let json = json_string(instances);
    let now = Utc::now().to_rfc3339();
    sqlx::query("UPDATE games SET element_instances_json=?1, updated_at=?2 WHERE id=?3")
        .bind(&json)
        .bind(&now)
        .bind(game_id)
        .execute(pool)
        .await?;
    Ok(())
}

pub async fn update_turn_mechanical_results(
    pool: &SqlitePool,
    turn_id: i64,
    results: &[MechanicalResult],
) -> AppResult<()> {
    let json = json_string(results);
    let now = Utc::now().to_rfc3339();
    sqlx::query("UPDATE game_turns SET mechanical_results_json=?1, updated_at=?2 WHERE id=?3")
        .bind(&json)
        .bind(&now)
        .bind(turn_id)
        .execute(pool)
        .await?;
    Ok(())
}

pub async fn update_turn_observability(
    pool: &SqlitePool,
    turn_id: i64,
    observability: &TurnObservability,
) -> AppResult<()> {
    let json = json_string(observability);
    let now = Utc::now().to_rfc3339();
    sqlx::query("UPDATE game_turns SET observability_json=?1, updated_at=?2 WHERE id=?3")
        .bind(&json)
        .bind(&now)
        .bind(turn_id)
        .execute(pool)
        .await?;
    Ok(())
}

pub async fn merge_turn_observability(
    pool: &SqlitePool,
    turn_id: i64,
    patch: TurnObservability,
) -> AppResult<()> {
    let turn = sqlx::query_as::<_, TurnRow>(&turn_select("WHERE id = ?1"))
        .bind(turn_id)
        .fetch_one(pool)
        .await?;
    let mut obs = parse_json_default::<TurnObservability>(&turn.observability_json);
    obs.llm_call_count += patch.llm_call_count;
    obs.tool_call_count += patch.tool_call_count;
    obs.tool_iterations += patch.tool_iterations;
    for (k, v) in patch.phase_timings_ms {
        obs.phase_timings_ms.insert(k, v);
    }
    obs.engine_mode = patch.engine_mode;
    update_turn_observability(pool, turn_id, &obs).await
}

pub async fn insert_turn_check(
    pool: &SqlitePool,
    turn_id: i64,
    check: &GameTurnCheck,
) -> AppResult<i64> {
    let rolls = serde_json::to_string(&check.rolls).unwrap_or_else(|_| "[]".to_string());
    let tier = check
        .tier
        .map(crate::game_resolution::tier_str)
        .unwrap_or("")
        .to_string();
    let now = Utc::now().to_rfc3339();
    let id = sqlx::query_scalar::<_, i64>(
        "INSERT INTO game_turn_checks (turn_id, label, skill, modifier, stakes, justification, dice_expr, seed, rolls, total, tier, margin, sort_order, created_at) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14) RETURNING id",
    )
    .bind(turn_id)
    .bind(&check.label)
    .bind(&check.skill)
    .bind(check.modifier)
    .bind(&check.stakes)
    .bind(&check.justification)
    .bind(&check.dice_expr)
    .bind(check.seed)
    .bind(&rolls)
    .bind(check.total)
    .bind(&tier)
    .bind(check.margin)
    .bind(check.sort_order)
    .bind(&now)
    .fetch_one(pool)
    .await?;
    Ok(id)
}

pub async fn clear_system_rolls(pool: &SqlitePool, turn_id: i64) -> AppResult<()> {
    sqlx::query("DELETE FROM game_turn_system_rolls WHERE turn_id = ?1")
        .bind(turn_id)
        .execute(pool)
        .await?;
    Ok(())
}

pub async fn clear_turn_checks(pool: &SqlitePool, turn_id: i64) -> AppResult<()> {
    sqlx::query("DELETE FROM game_turn_checks WHERE turn_id = ?1")
        .bind(turn_id)
        .execute(pool)
        .await?;
    Ok(())
}

pub async fn invalidate_scene_summaries_from(
    pool: &SqlitePool,
    game_id: i64,
    from_turn: i64,
) -> AppResult<()> {
    sqlx::query("UPDATE game_scenes SET summary_valid = 0 WHERE game_id = ?1 AND start_turn >= ?2")
        .bind(game_id)
        .bind(from_turn)
        .execute(pool)
        .await?;
    Ok(())
}

pub async fn update_scene_summary(
    pool: &SqlitePool,
    scene_id: i64,
    summary: &str,
) -> AppResult<()> {
    let now = Utc::now().to_rfc3339();
    sqlx::query(
        "UPDATE game_scenes SET summary=?1, summary_valid=1, summary_at=?2, updated_at=?3 WHERE id=?4",
    )
    .bind(summary)
    .bind(&now)
    .bind(&now)
    .bind(scene_id)
    .execute(pool)
    .await?;
    Ok(())
}

#[derive(sqlx::FromRow)]
struct GameRow {
    id: i64,
    title: String,
    premise: String,
    setting: String,
    gm_style: String,
    opening_message: String,
    character_id: Option<i64>,
    scenario_id: Option<i64>,
    resolution_system: String,
    modifier_min: i64,
    modifier_max: i64,
    merge_resolve_scene: i64,
    step_mode: i64,
    engine_mode: String,
    model_checks: String,
    model_resolve: String,
    model_prose: String,
    rules_blocks_json: String,
    state_schema_json: String,
    win_condition_json: Option<String>,
    scenario_triggers_json: String,
    trait_defs_json: String,
    game_elements_json: String,
    element_instances_json: String,
    created_at: String,
    updated_at: String,
    archived_at: Option<String>,
}

#[derive(sqlx::FromRow)]
struct ActorRow {
    id: i64,
    game_id: i64,
    role: String,
    name: String,
    description: String,
    skills: String,
    sort_order: i64,
    created_at: String,
    updated_at: String,
}

#[derive(sqlx::FromRow)]
struct StateRow {
    id: i64,
    game_id: i64,
    actor_id: Option<i64>,
    kind: String,
    key: String,
    value: String,
    num_value: Option<i64>,
    max_value: Option<i64>,
    float_value: Option<f64>,
    float_min: Option<f64>,
    float_max: Option<f64>,
    unit: Option<String>,
    source_turn: i64,
    updated_at: String,
}

#[derive(sqlx::FromRow)]
struct TurnRow {
    id: i64,
    game_id: i64,
    sort_order: i64,
    player_action: String,
    guidance_notes: String,
    phase: String,
    scene_beats: String,
    prose: String,
    thought_content: String,
    thought_duration_ms: Option<i64>,
    thought_in_progress: i64,
    state_changes: String,
    is_opening: i64,
    plan_json: Option<String>,
    mechanical_results_json: String,
    observability_json: String,
    created_at: String,
    updated_at: String,
}

#[derive(sqlx::FromRow)]
struct SystemRollRow {
    id: i64,
    turn_id: i64,
    label: String,
    dice_expr: String,
    rolls: String,
    outcome_key: String,
    outcome_summary: String,
    sort_order: i64,
    created_at: String,
}

#[derive(sqlx::FromRow)]
struct CheckRow {
    id: i64,
    turn_id: i64,
    label: String,
    skill: String,
    modifier: i64,
    stakes: String,
    justification: String,
    dice_expr: String,
    seed: i64,
    rolls: String,
    total: i64,
    tier: String,
    margin: i64,
    sort_order: i64,
    created_at: String,
}

#[derive(sqlx::FromRow)]
struct SceneRow {
    id: i64,
    game_id: i64,
    title: String,
    summary: String,
    summary_valid: i64,
    summary_at: Option<String>,
    start_turn: i64,
    sort_order: i64,
    created_at: String,
    updated_at: String,
}

#[cfg(test)]
mod tests {
    use dreamwell_types::{GameCreate, ScenarioNpc, StateKind, TrackedVarDef};

    use super::*;

    async fn test_pool() -> SqlitePool {
        let pool = SqlitePool::connect("sqlite::memory:").await.expect("pool");
        sqlx::migrate!("./migrations")
            .run(&pool)
            .await
            .expect("migrate");
        pool
    }

    #[tokio::test]
    async fn seed_scenario_state_applies_world_and_pc_scope() {
        let pool = test_pool().await;
        let now = chrono::Utc::now().to_rfc3339();
        sqlx::query(
            "INSERT INTO games (title, premise, setting, gm_style, opening_message, created_at, updated_at) VALUES ('g','','','','',?1,?1)",
        )
        .bind(&now)
        .execute(&pool)
        .await
        .expect("game");
        let game_id: i64 = sqlx::query_scalar("SELECT id FROM games WHERE title = 'g'")
            .fetch_one(&pool)
            .await
            .expect("id");
        sqlx::query(
            "INSERT INTO game_actors (game_id, role, name, description, skills, created_at, updated_at) VALUES (?1,'pc','Hero','','{}',?2,?2)",
        )
        .bind(game_id)
        .bind(&now)
        .execute(&pool)
        .await
        .expect("actor");
        let pc_id: i64 =
            sqlx::query_scalar("SELECT id FROM game_actors WHERE game_id = ?1 AND role = 'pc'")
                .bind(game_id)
                .fetch_one(&pool)
                .await
                .expect("pc");

        let payload = GameCreate {
            state_schema: vec![
                TrackedVarDef {
                    key: "Leader_Position".into(),
                    kind: StateKind::Measurement,
                    target: "world".into(),
                    initial_num: Some(3),
                    ..Default::default()
                },
                TrackedVarDef {
                    key: "health".into(),
                    kind: StateKind::Measurement,
                    target: "pc".into(),
                    initial_num: Some(8),
                    initial_max: Some(10),
                    ..Default::default()
                },
            ],
            ..Default::default()
        };
        seed_scenario_state(&pool, game_id, pc_id, &payload, &now)
            .await
            .expect("seed");

        let world_actor: Option<i64> = sqlx::query_scalar(
            "SELECT actor_id FROM game_state_entries WHERE game_id = ?1 AND key = 'Leader_Position'",
        )
        .bind(game_id)
        .fetch_one(&pool)
        .await
        .expect("world row");
        assert!(world_actor.is_none());

        let (pc_actor, float_value, float_max): (Option<i64>, Option<f64>, Option<f64>) =
            sqlx::query_as(
                "SELECT actor_id, float_value, float_max FROM game_state_entries WHERE game_id = ?1 AND key = 'health'",
            )
            .bind(game_id)
            .fetch_one(&pool)
            .await
            .expect("health row");
        assert_eq!(pc_actor, Some(pc_id));
        assert_eq!(float_value, Some(8.0));
        assert_eq!(float_max, Some(10.0));
    }

    #[tokio::test]
    async fn create_game_does_not_seed_default_health_or_stress() {
        let pool = test_pool().await;
        let detail = create_game(&pool, GameCreate::default())
            .await
            .expect("create");
        let rows: Vec<String> = sqlx::query_scalar(
            "SELECT key FROM game_state_entries WHERE game_id = ?1 AND key IN ('health', 'stress')",
        )
        .bind(detail.game.id)
        .fetch_all(&pool)
        .await
        .expect("rows");
        assert!(rows.is_empty());
    }

    #[tokio::test]
    async fn create_game_seeds_health_from_schema() {
        let pool = test_pool().await;
        let payload = GameCreate {
            pc_name: "Hero".into(),
            state_schema: vec![TrackedVarDef {
                key: "health".into(),
                kind: StateKind::Measurement,
                target: "pc".into(),
                initial_num: Some(3),
                initial_max: Some(3),
                ..Default::default()
            }],
            ..Default::default()
        };
        let detail = create_game(&pool, payload).await.expect("create");
        let health_rows: Vec<(Option<f64>, Option<f64>)> = sqlx::query_as(
            "SELECT float_value, float_max FROM game_state_entries WHERE game_id = ?1 AND key = 'health'",
        )
        .bind(detail.game.id)
        .fetch_all(&pool)
        .await
        .expect("health");
        assert_eq!(health_rows.len(), 1);
        assert_eq!(health_rows[0], (Some(3.0), Some(3.0)));
    }

    #[tokio::test]
    async fn create_game_applies_opening_guidance_on_first_turn() {
        let pool = test_pool().await;
        let payload = GameCreate {
            opening_message: "I look around the room.".into(),
            opening_guidance: "Keep the tone cozy and skip combat.".into(),
            opening_as_player_action: true,
            ..Default::default()
        };
        let detail = create_game(&pool, payload).await.expect("create");
        assert_eq!(detail.turns.len(), 1);
        let turn = &detail.turns[0];
        assert!(!turn.is_opening);
        assert_eq!(turn.player_action, "I look around the room.");
        assert_eq!(turn.guidance_notes, "Keep the tone cozy and skip combat.");
    }

    #[tokio::test]
    async fn create_game_queues_guidance_only_turn_after_static_opening() {
        let pool = test_pool().await;
        let payload = GameCreate {
            opening_message: "Steam curls from the kettle.".into(),
            opening_guidance: "Introduce the shopkeeper gently.".into(),
            opening_as_player_action: false,
            ..Default::default()
        };
        let detail = create_game(&pool, payload).await.expect("create");
        assert_eq!(detail.turns.len(), 2);
        assert!(detail.turns[0].is_opening);
        assert_eq!(detail.turns[0].prose, "Steam curls from the kettle.");
        assert_eq!(detail.turns[1].player_action, "");
        assert_eq!(
            detail.turns[1].guidance_notes,
            "Introduce the shopkeeper gently."
        );
    }

    #[tokio::test]
    async fn seed_scenario_state_applies_npc_scoped_entries() {
        let pool = test_pool().await;
        let now = chrono::Utc::now().to_rfc3339();
        sqlx::query(
            "INSERT INTO games (title, premise, setting, gm_style, opening_message, created_at, updated_at) VALUES ('g','','','','',?1,?1)",
        )
        .bind(&now)
        .execute(&pool)
        .await
        .expect("game");
        let game_id: i64 = sqlx::query_scalar("SELECT id FROM games WHERE title = 'g'")
            .fetch_one(&pool)
            .await
            .expect("id");
        sqlx::query(
            "INSERT INTO game_actors (game_id, role, name, description, skills, sort_order, created_at, updated_at) VALUES (?1,'pc','Hero','','{}',0,?2,?2)",
        )
        .bind(game_id)
        .bind(&now)
        .execute(&pool)
        .await
        .expect("pc");
        let pc_id: i64 =
            sqlx::query_scalar("SELECT id FROM game_actors WHERE game_id = ?1 AND role = 'pc'")
                .bind(game_id)
                .fetch_one(&pool)
                .await
                .expect("pc id");
        sqlx::query(
            "INSERT INTO game_actors (game_id, role, name, description, skills, sort_order, created_at, updated_at) VALUES (?1,'npc','Guard','','{}',1,?2,?2)",
        )
        .bind(game_id)
        .bind(&now)
        .execute(&pool)
        .await
        .expect("npc");
        let guard_id: i64 = sqlx::query_scalar(
            "SELECT id FROM game_actors WHERE game_id = ?1 AND role = 'npc' AND name = 'Guard'",
        )
        .bind(game_id)
        .fetch_one(&pool)
        .await
        .expect("guard");

        let payload = GameCreate {
            invited_cast: vec![ScenarioNpc {
                name: "Guard".into(),
                ..Default::default()
            }],
            state_schema: vec![TrackedVarDef {
                key: "alertness".into(),
                kind: StateKind::Sequence,
                target: "Guard".into(),
                initial_num: Some(2),
                sequence_items: Some(vec![
                    "calm".into(),
                    "watchful".into(),
                    "alert".into(),
                    "alarm".into(),
                ]),
                ..Default::default()
            }],
            ..Default::default()
        };
        seed_scenario_state(&pool, game_id, pc_id, &payload, &now)
            .await
            .expect("seed");

        let (actor_id, value): (Option<i64>, String) = sqlx::query_as(
            "SELECT actor_id, value FROM game_state_entries WHERE game_id = ?1 AND key = 'alertness'",
        )
        .bind(game_id)
        .fetch_one(&pool)
        .await
        .expect("alertness");
        assert_eq!(actor_id, Some(guard_id));
        let seq = dreamwell_types::SequencePayload::decode(&value).expect("sequence payload");
        assert_eq!(seq.position, 2);
        assert_eq!(seq.items.len(), 4);
    }

    #[test]
    fn turn_can_be_regenerated_matches_ui_retry_affordances() {
        let now = chrono::Utc::now();
        let base = || GameTurn {
            id: 1,
            game_id: 1,
            sort_order: 1,
            player_action: "act".into(),
            guidance_notes: String::new(),
            phase: String::new(),
            scene_beats: Vec::new(),
            prose: String::new(),
            thought_content: String::new(),
            thought_duration_ms: None,
            thought_in_progress: false,
            state_changes: Vec::new(),
            checks: Vec::new(),
            system_rolls: Vec::new(),
            plan: None,
            mechanical_results: Vec::new(),
            observability: TurnObservability::default(),
            is_opening: false,
            generation_error: None,
            created_at: now,
            updated_at: now,
        };

        let mut turn = base();
        turn.phase = "done".into();
        turn.prose = "The door creaks.".into();
        assert!(turn_can_be_regenerated(&turn));

        turn.prose.clear();
        assert!(!turn_can_be_regenerated(&turn));

        turn.phase = "failed".into();
        assert!(turn_can_be_regenerated(&turn));

        turn.phase = "prose".into();
        assert!(turn_can_be_regenerated(&turn));

        turn.phase = "pending".into();
        assert!(!turn_can_be_regenerated(&turn));

        turn.phase = "checks_pause".into();
        assert!(!turn_can_be_regenerated(&turn));
    }

    #[tokio::test]
    async fn prepare_regenerate_turn_allows_failed_last_turn() {
        let pool = test_pool().await;
        let detail = create_game(&pool, GameCreate::default())
            .await
            .expect("create");
        let game_id = detail.game.id;
        let now = chrono::Utc::now().to_rfc3339();
        let turn_id = sqlx::query_scalar::<_, i64>(
            "INSERT INTO game_turns (game_id, sort_order, player_action, guidance_notes, phase, is_opening, created_at, updated_at) VALUES (?1,1,'look around','','failed',0,?2,?2) RETURNING id",
        )
        .bind(game_id)
        .bind(&now)
        .fetch_one(&pool)
        .await
        .expect("turn");

        let job = prepare_regenerate_turn(&pool, game_id, turn_id)
            .await
            .expect("regenerate");
        assert_eq!(job.job_type, JobType::GameTurnStructuredAgent);

        let turn = get_turn(&pool, game_id, turn_id).await.expect("turn");
        assert_eq!(turn.phase, "pending");
    }

    #[tokio::test]
    async fn update_turn_field_updates_prose_and_player_action() {
        let pool = test_pool().await;
        let detail = create_game(&pool, GameCreate::default())
            .await
            .expect("create");
        let game_id = detail.game.id;
        let now = chrono::Utc::now().to_rfc3339();
        let turn_id = sqlx::query_scalar::<_, i64>(
            "INSERT INTO game_turns (game_id, sort_order, player_action, guidance_notes, phase, prose, thought_content, is_opening, created_at, updated_at) VALUES (?1,1,'look around','','done','Old prose.','hidden thoughts',0,?2,?2) RETURNING id",
        )
        .bind(game_id)
        .bind(&now)
        .fetch_one(&pool)
        .await
        .expect("turn");

        update_turn_field(&pool, turn_id, TurnEditField::Prose, "New prose.")
            .await
            .expect("prose");
        update_turn_field(&pool, turn_id, TurnEditField::PlayerAction, "peek inside")
            .await
            .expect("action");

        let turn = get_turn(&pool, game_id, turn_id).await.expect("turn");
        assert_eq!(turn.prose, "New prose.");
        assert_eq!(turn.player_action, "peek inside");
    }
}
