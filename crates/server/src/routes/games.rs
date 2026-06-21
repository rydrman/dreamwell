use std::convert::Infallible;
use std::time::Duration;

use async_stream::stream;
use axum::{
    extract::{Multipart, Path, State},
    response::{
        sse::{Event, KeepAlive, Sse},
        IntoResponse,
    },
    routing::{get, post},
    Json, Router,
};
use dreamwell_types::{
    Game, GameActorUpdate, GameCreate, GameDetail, GameStateEntryUpdate, GameStreamPayload,
    GameUpdate, GenerateRequest, ImportGameDraftResponse, Job, OkResponse, SubmitTurnRequest,
};

use crate::character_import::parse_character_import;
use crate::db;
use crate::error::{AppError, AppResult};
use crate::queue::enqueue_game_generation;
use crate::routes::AppState;
use crate::scenario_import::{game_create_from_character, GameCharacterImportMode};

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/", get(list_games).post(create_game))
        .route("/import", post(import_game_draft))
        .route("/:id", get(get_game).patch(update_game).delete(delete_game))
        .route("/:id/stream", get(stream_game))
        .route("/:id/turns", post(submit_turn))
        .route("/:id/turns/:turn_id/continue", post(continue_turn))
        .route("/:id/turns/:turn_id/regenerate", post(regenerate_turn))
        .route(
            "/:id/turns/:turn_id/prose/recheck",
            post(recheck_turn_prose),
        )
        .route(
            "/:id/turns/:turn_id/state/recheck",
            post(recheck_turn_state),
        )
        .route("/:id/actors/:actor_id", axum::routing::patch(update_actor))
        .route(
            "/:id/state/:entry_id",
            axum::routing::patch(update_state_entry),
        )
}

async fn list_games(State(state): State<AppState>) -> AppResult<Json<Vec<Game>>> {
    Ok(Json(db::list_games(&state.pool).await?))
}

async fn create_game(
    State(state): State<AppState>,
    Json(payload): Json<GameCreate>,
) -> AppResult<Json<GameDetail>> {
    Ok(Json(db::create_game(&state.pool, payload).await?))
}

async fn import_game_draft(
    State(_state): State<AppState>,
    mut multipart: Multipart,
) -> AppResult<Json<ImportGameDraftResponse>> {
    let mut filename = "character.json".to_string();
    let mut content = Vec::new();
    while let Some(field) = multipart
        .next_field()
        .await
        .map_err(|e| AppError::bad_request(e.to_string()))?
    {
        if field.name() == Some("file") {
            if let Some(name) = field.file_name() {
                filename = name.to_string();
            }
            content = field
                .bytes()
                .await
                .map_err(|e| AppError::bad_request(e.to_string()))?
                .to_vec();
        }
    }
    let card = parse_character_import(&filename, &content)?;
    let draft = game_create_from_character(card, GameCharacterImportMode::World, None, None);
    let source = if filename.to_lowercase().ends_with(".png") {
        "png"
    } else {
        "json"
    };
    Ok(Json(ImportGameDraftResponse {
        draft,
        source: source.to_string(),
    }))
}

async fn get_game(
    State(state): State<AppState>,
    Path(id): Path<i64>,
) -> AppResult<Json<GameDetail>> {
    Ok(Json(db::get_game_detail(&state.pool, id).await?))
}

async fn update_game(
    State(state): State<AppState>,
    Path(id): Path<i64>,
    Json(payload): Json<GameUpdate>,
) -> AppResult<Json<GameDetail>> {
    db::update_game(&state.pool, id, payload).await?;
    Ok(Json(db::get_game_detail(&state.pool, id).await?))
}

async fn delete_game(
    State(state): State<AppState>,
    Path(id): Path<i64>,
) -> AppResult<Json<OkResponse>> {
    db::delete_game(&state.pool, id).await?;
    Ok(Json(OkResponse { ok: true }))
}

async fn update_actor(
    State(state): State<AppState>,
    Path((id, actor_id)): Path<(i64, i64)>,
    Json(payload): Json<GameActorUpdate>,
) -> AppResult<Json<GameDetail>> {
    db::update_actor(&state.pool, id, actor_id, payload).await?;
    db::touch_game(&state.pool, id).await?;
    Ok(Json(db::get_game_detail(&state.pool, id).await?))
}

async fn update_state_entry(
    State(state): State<AppState>,
    Path((id, entry_id)): Path<(i64, i64)>,
    Json(payload): Json<GameStateEntryUpdate>,
) -> AppResult<Json<GameDetail>> {
    db::update_state_entry(&state.pool, id, entry_id, payload).await?;
    db::touch_game(&state.pool, id).await?;
    Ok(Json(db::get_game_detail(&state.pool, id).await?))
}

async fn submit_turn(
    State(state): State<AppState>,
    Path(id): Path<i64>,
    Json(payload): Json<SubmitTurnRequest>,
) -> AppResult<Json<GameDetail>> {
    let settings = db::get_settings(&state.pool).await?;
    let game = db::get_game(&state.pool, id).await?;
    crate::game_turn::ensure_model_for_phase(
        &game,
        &settings,
        crate::game_turn::GameModelPhase::Checks,
    )?;
    let (_turn, job) = db::prepare_submit_turn(&state.pool, id, &payload).await?;
    enqueue_game_generation(&state.queue, job).await?;
    Ok(Json(db::get_game_detail(&state.pool, id).await?))
}

async fn continue_turn(
    State(state): State<AppState>,
    Path((id, turn_id)): Path<(i64, i64)>,
) -> AppResult<Json<GameDetail>> {
    let job = db::prepare_continue_turn(&state.pool, id, turn_id).await?;
    enqueue_game_generation(&state.queue, job).await?;
    Ok(Json(db::get_game_detail(&state.pool, id).await?))
}

async fn regenerate_turn(
    State(state): State<AppState>,
    Path((id, turn_id)): Path<(i64, i64)>,
) -> AppResult<Json<GameDetail>> {
    let settings = db::get_settings(&state.pool).await?;
    let game = db::get_game(&state.pool, id).await?;
    let turn = db::get_turn(&state.pool, id, turn_id).await?;
    let phase = if turn.phase == "failed" && turn.checks.is_empty() {
        crate::game_turn::GameModelPhase::Checks
    } else {
        crate::game_turn::GameModelPhase::Resolve
    };
    crate::game_turn::ensure_model_for_phase(&game, &settings, phase)?;
    let job = db::prepare_regenerate_turn(&state.pool, id, turn_id).await?;
    enqueue_game_generation(&state.queue, job).await?;
    Ok(Json(db::get_game_detail(&state.pool, id).await?))
}

async fn recheck_turn_prose(
    State(state): State<AppState>,
    Path((id, turn_id)): Path<(i64, i64)>,
    Json(payload): Json<GenerateRequest>,
) -> AppResult<Json<Job>> {
    let settings = db::get_settings(&state.pool).await?;
    let job = state
        .queue
        .enqueue_game_prose_recheck(&state.pool, id, turn_id, &payload.guidance_notes, &settings)
        .await?;
    db::touch_game(&state.pool, id).await?;
    Ok(Json(job))
}

async fn recheck_turn_state(
    State(state): State<AppState>,
    Path((id, turn_id)): Path<(i64, i64)>,
    Json(payload): Json<GenerateRequest>,
) -> AppResult<Json<Job>> {
    let settings = db::get_settings(&state.pool).await?;
    let job = state
        .queue
        .enqueue_game_state_recheck(&state.pool, id, turn_id, &payload.guidance_notes, &settings)
        .await?;
    db::touch_game(&state.pool, id).await?;
    Ok(Json(job))
}

async fn stream_game(
    State(state): State<AppState>,
    Path(id): Path<i64>,
) -> AppResult<impl IntoResponse> {
    let _ = db::get_game(&state.pool, id).await?;
    let pool = state.pool.clone();
    let interval = Duration::from_millis(state.sse_poll_interval_ms);

    let event_stream = stream! {
        let mut last_payload = String::new();
        loop {
            let detail = match db::get_game_detail(&pool, id).await {
                Ok(detail) => detail,
                Err(_) => {
                    yield Ok::<_, Infallible>(Event::default().event("error").data("{\"detail\":\"not found\"}"));
                    break;
                }
            };
            let active_job = db::get_active_game_job(&pool, id).await.ok().flatten();
            let has_active_job = active_job.is_some();
            let payload = serde_json::to_string(&GameStreamPayload {
                detail: detail.clone(),
                active_job: active_job.clone(),
            }).unwrap_or_default();

            if payload != last_payload {
                last_payload = payload.clone();
                yield Ok(Event::default().data(payload));
            }

            if !has_active_job {
                yield Ok(Event::default().event("idle").data(format!("{{\"game_id\":{id}}}")));
                break;
            }

            tokio::time::sleep(interval).await;
        }
    };

    Ok(Sse::new(event_stream).keep_alive(KeepAlive::default()))
}
