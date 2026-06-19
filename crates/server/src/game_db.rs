use std::collections::HashMap;

use chrono::Utc;
use dreamwell_types::{
    AppliedStateChange, Game, GameActor, GameActorUpdate, GameCreate, GameDetail, GameScene,
    GameStateEntry, GameStateEntryUpdate, GameTurn, GameTurnCheck, GameUpdate, Job, JobType,
    ResolutionSystem, StateKind, SubmitTurnRequest,
};
use sqlx::SqlitePool;

use crate::db::{get_job, job_type_str, parse_dt, JobRow};
use crate::error::{AppError, AppResult};

const DEFAULT_SKILLS: &str = r#"{"Finesse":0,"Force":0,"Flair":0,"Focus":0,"Sway":0}"#;

pub async fn list_games(pool: &SqlitePool) -> AppResult<Vec<Game>> {
    let rows = sqlx::query_as::<_, GameRow>(
        "SELECT id, title, premise, setting, gm_style, resolution_system, modifier_min, modifier_max, merge_resolve_scene, step_mode, created_at, updated_at FROM games ORDER BY updated_at DESC",
    )
    .fetch_all(pool)
    .await?;
    let mut games = Vec::with_capacity(rows.len());
    for row in rows {
        games.push(game_from_row(pool, row).await?);
    }
    Ok(games)
}

pub async fn get_game(pool: &SqlitePool, id: i64) -> AppResult<Game> {
    let row = sqlx::query_as::<_, GameRow>(
        "SELECT id, title, premise, setting, gm_style, resolution_system, modifier_min, modifier_max, merge_resolve_scene, step_mode, created_at, updated_at FROM games WHERE id = ?1",
    )
    .bind(id)
    .fetch_optional(pool)
    .await?
    .ok_or_else(|| AppError::not_found("Game not found"))?;
    game_from_row(pool, row).await
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
    let active_job = get_active_game_job(pool, row.id).await?;
    let queued_jobs: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM generation_jobs WHERE game_id = ?1 AND status = 'queued'",
    )
    .bind(row.id)
    .fetch_one(pool)
    .await?;
    Ok(Game {
        id: row.id,
        title: row.title,
        premise: row.premise,
        setting: row.setting,
        gm_style: row.gm_style,
        resolution_system: parse_resolution_system(&row.resolution_system),
        modifier_min: row.modifier_min,
        modifier_max: row.modifier_max,
        merge_resolve_scene: row.merge_resolve_scene != 0,
        step_mode: row.step_mode != 0,
        created_at: parse_dt(&row.created_at)?,
        updated_at: parse_dt(&row.updated_at)?,
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

pub async fn create_game(pool: &SqlitePool, payload: GameCreate) -> AppResult<GameDetail> {
    let now = Utc::now().to_rfc3339();
    let id = sqlx::query_scalar::<_, i64>(
        "INSERT INTO games (title, premise, setting, gm_style, created_at, updated_at) VALUES (?1,?2,?3,?4,?5,?5) RETURNING id",
    )
    .bind(&payload.title)
    .bind(&payload.premise)
    .bind(&payload.setting)
    .bind(&payload.gm_style)
    .bind(&now)
    .fetch_one(pool)
    .await?;

    let actor_now = now.clone();
    sqlx::query(
        "INSERT INTO game_actors (game_id, role, name, description, skills, created_at, updated_at) VALUES (?1,'pc','','',?2,?3,?3)",
    )
    .bind(id)
    .bind(DEFAULT_SKILLS)
    .bind(&actor_now)
    .execute(pool)
    .await?;

    let actor_id: i64 =
        sqlx::query_scalar("SELECT id FROM game_actors WHERE game_id = ?1 AND role = 'pc' LIMIT 1")
            .bind(id)
            .fetch_one(pool)
            .await?;

    for (key, current, max) in [("health", 5, 5), ("stress", 0, 5)] {
        sqlx::query(
            "INSERT INTO game_state_entries (game_id, actor_id, kind, key, num_value, max_value, source_turn, updated_at) VALUES (?1,?2,'resource',?3,?4,?5,-1,?6)",
        )
        .bind(id)
        .bind(actor_id)
        .bind(key)
        .bind(current)
        .bind(max)
        .bind(&now)
        .execute(pool)
        .await?;
    }

    sqlx::query(
        "INSERT INTO game_scenes (game_id, title, start_turn, sort_order, created_at, updated_at) VALUES (?1,'Opening Scene',0,0,?2,?2)",
    )
    .bind(id)
    .bind(&now)
    .execute(pool)
    .await?;

    get_game_detail(pool, id).await
}

pub async fn update_game(pool: &SqlitePool, id: i64, payload: GameUpdate) -> AppResult<Game> {
    let existing = get_game(pool, id).await?;
    let updated = Game {
        title: payload.title.unwrap_or(existing.title),
        premise: payload.premise.unwrap_or(existing.premise),
        setting: payload.setting.unwrap_or(existing.setting),
        gm_style: payload.gm_style.unwrap_or(existing.gm_style),
        modifier_min: payload.modifier_min.unwrap_or(existing.modifier_min),
        modifier_max: payload.modifier_max.unwrap_or(existing.modifier_max),
        merge_resolve_scene: payload
            .merge_resolve_scene
            .unwrap_or(existing.merge_resolve_scene),
        step_mode: payload.step_mode.unwrap_or(existing.step_mode),
        updated_at: Utc::now(),
        ..existing
    };
    sqlx::query(
        "UPDATE games SET title=?1, premise=?2, setting=?3, gm_style=?4, modifier_min=?5, modifier_max=?6, merge_resolve_scene=?7, step_mode=?8, updated_at=?9 WHERE id=?10",
    )
    .bind(&updated.title)
    .bind(&updated.premise)
    .bind(&updated.setting)
    .bind(&updated.gm_style)
    .bind(updated.modifier_min)
    .bind(updated.modifier_max)
    .bind(updated.merge_resolve_scene as i64)
    .bind(updated.step_mode as i64)
    .bind(updated.updated_at.to_rfc3339())
    .bind(id)
    .execute(pool)
    .await?;
    get_game(pool, id).await
}

pub async fn delete_game(pool: &SqlitePool, id: i64) -> AppResult<()> {
    let result = sqlx::query("DELETE FROM games WHERE id = ?1")
        .bind(id)
        .execute(pool)
        .await?;
    if result.rows_affected() == 0 {
        return Err(AppError::not_found("Game not found"));
    }
    Ok(())
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
        "SELECT id, game_id, actor_id, kind, key, value, num_value, max_value, source_turn, updated_at FROM game_state_entries WHERE game_id = ?1 ORDER BY kind ASC, key ASC",
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
        source_turn: row.source_turn,
        updated_at: parse_dt(&row.updated_at)?,
    })
}

fn parse_state_kind(s: &str) -> StateKind {
    match s {
        "resource" => StateKind::Resource,
        "condition" => StateKind::Condition,
        "fact" => StateKind::Fact,
        "clock" => StateKind::Clock,
        _ => StateKind::Fact,
    }
}

pub async fn update_state_entry(
    pool: &SqlitePool,
    game_id: i64,
    entry_id: i64,
    payload: GameStateEntryUpdate,
) -> AppResult<GameStateEntry> {
    let row = sqlx::query_as::<_, StateRow>(
        "SELECT id, game_id, actor_id, kind, key, value, num_value, max_value, source_turn, updated_at FROM game_state_entries WHERE id = ?1 AND game_id = ?2",
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
    let rows = sqlx::query_as::<_, TurnRow>(
        "SELECT id, game_id, sort_order, player_action, phase, scene_beats, prose, state_changes, created_at, updated_at FROM game_turns WHERE game_id = ?1 ORDER BY sort_order ASC, id ASC",
    )
    .bind(game_id)
    .fetch_all(pool)
    .await?;
    let mut turns = Vec::with_capacity(rows.len());
    for row in rows {
        turns.push(turn_from_row(pool, row).await?);
    }
    Ok(turns)
}

async fn turn_from_row(pool: &SqlitePool, row: TurnRow) -> AppResult<GameTurn> {
    let checks = list_checks_for_turn(pool, row.id).await?;
    Ok(GameTurn {
        id: row.id,
        game_id: row.game_id,
        sort_order: row.sort_order,
        player_action: row.player_action,
        phase: row.phase,
        scene_beats: parse_string_array(&row.scene_beats),
        prose: row.prose,
        state_changes: parse_state_changes(&row.state_changes),
        checks,
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
        "SELECT id, job_type, chat_id, message_id, story_id, chapter_id, beat_id, game_id, turn_id, guidance_notes, status, error, position, created_at, started_at, completed_at FROM generation_jobs WHERE game_id = ?1 AND status IN ('queued','running') ORDER BY created_at ASC LIMIT 1",
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
    let _ = get_game(pool, game_id).await?;
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
        "INSERT INTO game_turns (game_id, sort_order, player_action, phase, created_at, updated_at) VALUES (?1,?2,?3,'pending',?4,?4) RETURNING id",
    )
    .bind(game_id)
    .bind(sort_order)
    .bind(&payload.player_action)
    .bind(&now)
    .fetch_one(pool)
    .await?;
    let turn = turn_from_row(
        pool,
        TurnRow {
            id: turn_id,
            game_id,
            sort_order,
            player_action: payload.player_action.clone(),
            phase: "pending".to_string(),
            scene_beats: "[]".to_string(),
            prose: String::new(),
            state_changes: "[]".to_string(),
            created_at: now.clone(),
            updated_at: now,
        },
    )
    .await?;
    let job = enqueue_game_job(
        pool,
        JobType::GameTurnCheck,
        game_id,
        Some(turn_id),
        payload.guidance_notes.clone(),
    )
    .await?;
    touch_game(pool, game_id).await?;
    Ok((turn, job))
}

pub async fn prepare_continue_turn(
    pool: &SqlitePool,
    game_id: i64,
    turn_id: i64,
) -> AppResult<Job> {
    let turn = get_turn(pool, game_id, turn_id).await?;
    if !turn.phase.ends_with("_pause") && turn.phase != "rolled" && turn.phase != "checks" {
        return Err(AppError::bad_request("Turn is not paused for step mode"));
    }
    if has_active_turn_job(pool, turn_id).await? {
        return Err(AppError::bad_request("Turn already has an active job"));
    }
    let job_type = match turn.phase.as_str() {
        "checks_pause" | "checks" => JobType::GameTurnResolve,
        "rolled_pause" | "rolled" => JobType::GameTurnResolve,
        "resolved_pause" | "resolved" => JobType::GameTurnProse,
        "scene_pause" | "scene" => JobType::GameTurnProse,
        other if other.ends_with("_pause") => JobType::GameTurnProse,
        _ => JobType::GameTurnResolve,
    };
    enqueue_game_job(pool, job_type, game_id, Some(turn_id), String::new()).await
}

pub async fn prepare_regenerate_turn(
    pool: &SqlitePool,
    game_id: i64,
    turn_id: i64,
) -> AppResult<Job> {
    let turn = get_turn(pool, game_id, turn_id).await?;
    if has_active_turn_job(pool, turn_id).await? {
        return Err(AppError::bad_request("Turn already has an active job"));
    }
    let detail = get_game_detail(pool, game_id).await?;
    crate::game_state::revert_turn_state_changes(
        pool,
        game_id,
        turn_id,
        &turn.state_changes,
        &detail.actors,
    )
    .await?;
    sqlx::query(
        "UPDATE game_turns SET prose='', scene_beats='[]', state_changes='[]', phase='rolled', updated_at=?1 WHERE id=?2",
    )
    .bind(Utc::now().to_rfc3339())
    .bind(turn_id)
    .execute(pool)
    .await?;
    enqueue_game_job(
        pool,
        JobType::GameTurnResolve,
        game_id,
        Some(turn_id),
        String::new(),
    )
    .await
}

pub async fn get_turn(pool: &SqlitePool, game_id: i64, turn_id: i64) -> AppResult<GameTurn> {
    let row = sqlx::query_as::<_, TurnRow>(
        "SELECT id, game_id, sort_order, player_action, phase, scene_beats, prose, state_changes, created_at, updated_at FROM game_turns WHERE id = ?1 AND game_id = ?2",
    )
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

pub async fn update_turn_scene_beats(
    pool: &SqlitePool,
    turn_id: i64,
    beats: &[String],
) -> AppResult<()> {
    let json = serde_json::to_string(beats).unwrap_or_else(|_| "[]".to_string());
    let now = Utc::now().to_rfc3339();
    sqlx::query("UPDATE game_turns SET scene_beats=?1, updated_at=?2 WHERE id=?3")
        .bind(&json)
        .bind(&now)
        .bind(turn_id)
        .execute(pool)
        .await?;
    Ok(())
}

pub async fn update_turn_prose(pool: &SqlitePool, turn_id: i64, prose: &str) -> AppResult<()> {
    let now = Utc::now().to_rfc3339();
    sqlx::query("UPDATE game_turns SET prose=?1, updated_at=?2 WHERE id=?3")
        .bind(prose)
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
    resolution_system: String,
    modifier_min: i64,
    modifier_max: i64,
    merge_resolve_scene: i64,
    step_mode: i64,
    created_at: String,
    updated_at: String,
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
    source_turn: i64,
    updated_at: String,
}

#[derive(sqlx::FromRow)]
struct TurnRow {
    id: i64,
    game_id: i64,
    sort_order: i64,
    player_action: String,
    phase: String,
    scene_beats: String,
    prose: String,
    state_changes: String,
    created_at: String,
    updated_at: String,
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
