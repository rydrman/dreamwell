use std::convert::Infallible;
use std::time::Duration;

use async_stream::stream;
use axum::{
    extract::{Path, State},
    response::{
        sse::{Event, KeepAlive, Sse},
        IntoResponse,
    },
    routing::{get, post},
    Json, Router,
};
use dreamwell_types::{
    Game, GameActorUpdate, GameCreate, GameDetail, GameStateEntryUpdate, GameStreamPayload,
    GameUpdate, GenerateRequest, Job, OkResponse, SubmitTurnRequest,
};

use crate::db;
use crate::error::AppResult;
use crate::queue::enqueue_game_generation;
use crate::routes::AppState;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/", get(list_games).post(create_game))
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
