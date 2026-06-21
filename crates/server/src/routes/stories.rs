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
    GenerateRequest, Job, OkResponse, Story, StoryBeatCreate, StoryBeatUpdate, StoryChapterCreate,
    StoryChapterUpdate, StoryCreate, StoryDetail, StoryStateEntryUpdate, StoryStreamPayload,
    StoryUpdate, StoryVariable, StoryVariableUpdate,
};

use crate::db;
use crate::error::AppResult;
use crate::queue::enqueue_story_generation;
use crate::routes::AppState;
use crate::variables::strip_variable_key_from_story_beats;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/", get(list_stories).post(create_story))
        .route(
            "/:id",
            get(get_story).patch(update_story).delete(delete_story),
        )
        .route("/:id/stream", get(stream_story))
        .route("/:id/generate-chapter", post(generate_chapter))
        .route("/:id/propose-chapters", post(propose_chapters))
        .route("/:id/chapters", post(create_chapter))
        .route(
            "/:id/chapters/:chapter_id",
            axum::routing::patch(update_chapter).delete(delete_chapter),
        )
        .route(
            "/:id/chapters/:chapter_id/generate-beat",
            post(generate_beat),
        )
        .route(
            "/:id/chapters/:chapter_id/propose-beats",
            post(propose_beats),
        )
        .route("/:id/chapters/:chapter_id/beats", post(create_beat))
        .route(
            "/:id/chapters/:chapter_id/beats/:beat_id",
            axum::routing::patch(update_beat).delete(delete_beat),
        )
        .route(
            "/:id/chapters/:chapter_id/beats/:beat_id/generate-mechanical",
            post(generate_mechanical),
        )
        .route(
            "/:id/chapters/:chapter_id/beats/:beat_id/generate-prose",
            post(generate_prose),
        )
        .route(
            "/:id/chapters/:chapter_id/beats/:beat_id/continue-prose",
            post(continue_prose),
        )
        .route(
            "/:id/chapters/:chapter_id/beats/:beat_id/align-prose",
            post(align_beat_prose),
        )
        .route(
            "/:id/chapters/:chapter_id/summarize-prose",
            post(summarize_chapter_prose),
        )
        .route(
            "/:id/chapters/:chapter_id/beats/:beat_id/variables/recheck",
            post(recheck_beat_variables),
        )
        .route(
            "/:id/variables",
            get(list_story_variables).put(upsert_story_variable),
        )
        .route(
            "/:id/variables/:variable_id",
            axum::routing::delete(delete_story_variable),
        )
        .route(
            "/:id/state/:entry_id",
            axum::routing::patch(update_story_state_entry),
        )
}

async fn update_story_state_entry(
    State(state): State<AppState>,
    Path((id, entry_id)): Path<(i64, i64)>,
    Json(payload): Json<StoryStateEntryUpdate>,
) -> AppResult<Json<StoryDetail>> {
    db::update_story_state_entry(&state.pool, id, entry_id, payload).await?;
    db::touch_story(&state.pool, id).await?;
    Ok(Json(db::get_story_detail(&state.pool, id).await?))
}

async fn list_stories(State(state): State<AppState>) -> AppResult<Json<Vec<Story>>> {
    Ok(Json(db::list_stories(&state.pool).await?))
}

async fn create_story(
    State(state): State<AppState>,
    Json(payload): Json<StoryCreate>,
) -> AppResult<Json<StoryDetail>> {
    let story = db::create_story(&state.pool, payload).await?;
    Ok(Json(db::get_story_detail(&state.pool, story.id).await?))
}

async fn get_story(
    State(state): State<AppState>,
    Path(id): Path<i64>,
) -> AppResult<Json<StoryDetail>> {
    Ok(Json(db::get_story_detail(&state.pool, id).await?))
}

async fn update_story(
    State(state): State<AppState>,
    Path(id): Path<i64>,
    Json(payload): Json<StoryUpdate>,
) -> AppResult<Json<StoryDetail>> {
    db::update_story(&state.pool, id, payload).await?;
    Ok(Json(db::get_story_detail(&state.pool, id).await?))
}

async fn delete_story(
    State(state): State<AppState>,
    Path(id): Path<i64>,
) -> AppResult<Json<OkResponse>> {
    db::delete_story(&state.pool, id).await?;
    Ok(Json(OkResponse { ok: true }))
}

async fn create_chapter(
    State(state): State<AppState>,
    Path(id): Path<i64>,
    Json(payload): Json<StoryChapterCreate>,
) -> AppResult<Json<StoryDetail>> {
    db::create_chapter(&state.pool, id, payload).await?;
    Ok(Json(db::get_story_detail(&state.pool, id).await?))
}

async fn update_chapter(
    State(state): State<AppState>,
    Path((id, chapter_id)): Path<(i64, i64)>,
    Json(payload): Json<StoryChapterUpdate>,
) -> AppResult<Json<StoryDetail>> {
    db::update_chapter(&state.pool, id, chapter_id, payload).await?;
    Ok(Json(db::get_story_detail(&state.pool, id).await?))
}

async fn delete_chapter(
    State(state): State<AppState>,
    Path((id, chapter_id)): Path<(i64, i64)>,
) -> AppResult<Json<OkResponse>> {
    db::delete_chapter(&state.pool, id, chapter_id).await?;
    Ok(Json(OkResponse { ok: true }))
}

async fn create_beat(
    State(state): State<AppState>,
    Path((id, chapter_id)): Path<(i64, i64)>,
    Json(payload): Json<StoryBeatCreate>,
) -> AppResult<Json<StoryDetail>> {
    db::create_beat(&state.pool, id, chapter_id, payload).await?;
    Ok(Json(db::get_story_detail(&state.pool, id).await?))
}

async fn update_beat(
    State(state): State<AppState>,
    Path((id, chapter_id, beat_id)): Path<(i64, i64, i64)>,
    Json(payload): Json<StoryBeatUpdate>,
) -> AppResult<Json<StoryDetail>> {
    db::update_beat(&state.pool, id, chapter_id, beat_id, payload).await?;
    Ok(Json(db::get_story_detail(&state.pool, id).await?))
}

async fn delete_beat(
    State(state): State<AppState>,
    Path((id, chapter_id, beat_id)): Path<(i64, i64, i64)>,
) -> AppResult<Json<OkResponse>> {
    db::delete_beat(&state.pool, id, chapter_id, beat_id).await?;
    Ok(Json(OkResponse { ok: true }))
}

async fn generate_chapter(
    State(state): State<AppState>,
    Path(id): Path<i64>,
    Json(payload): Json<GenerateRequest>,
) -> AppResult<Json<StoryDetail>> {
    let (_chapter, job) = db::prepare_generate_chapter(&state.pool, id, &payload).await?;
    enqueue_story_generation(&state.queue, job).await?;
    Ok(Json(db::get_story_detail(&state.pool, id).await?))
}

async fn propose_chapters(
    State(state): State<AppState>,
    Path(id): Path<i64>,
    Json(payload): Json<GenerateRequest>,
) -> AppResult<Json<StoryDetail>> {
    let job = db::prepare_propose_chapters(&state.pool, id, &payload).await?;
    enqueue_story_generation(&state.queue, job).await?;
    Ok(Json(db::get_story_detail(&state.pool, id).await?))
}

async fn propose_beats(
    State(state): State<AppState>,
    Path((id, chapter_id)): Path<(i64, i64)>,
    Json(payload): Json<GenerateRequest>,
) -> AppResult<Json<StoryDetail>> {
    let job = db::prepare_propose_beats(&state.pool, id, chapter_id, &payload).await?;
    enqueue_story_generation(&state.queue, job).await?;
    Ok(Json(db::get_story_detail(&state.pool, id).await?))
}

async fn generate_beat(
    State(state): State<AppState>,
    Path((id, chapter_id)): Path<(i64, i64)>,
    Json(payload): Json<GenerateRequest>,
) -> AppResult<Json<StoryDetail>> {
    let (_beat, job) = db::prepare_generate_beat(&state.pool, id, chapter_id, &payload).await?;
    enqueue_story_generation(&state.queue, job).await?;
    Ok(Json(db::get_story_detail(&state.pool, id).await?))
}

async fn generate_mechanical(
    State(state): State<AppState>,
    Path((id, chapter_id, beat_id)): Path<(i64, i64, i64)>,
    Json(payload): Json<GenerateRequest>,
) -> AppResult<Json<StoryDetail>> {
    let job =
        db::prepare_generate_mechanical(&state.pool, id, chapter_id, beat_id, &payload).await?;
    enqueue_story_generation(&state.queue, job).await?;
    Ok(Json(db::get_story_detail(&state.pool, id).await?))
}

async fn generate_prose(
    State(state): State<AppState>,
    Path((id, chapter_id, beat_id)): Path<(i64, i64, i64)>,
    Json(payload): Json<GenerateRequest>,
) -> AppResult<Json<StoryDetail>> {
    let job = db::prepare_generate_prose(&state.pool, id, chapter_id, beat_id, &payload).await?;
    enqueue_story_generation(&state.queue, job).await?;
    Ok(Json(db::get_story_detail(&state.pool, id).await?))
}

async fn continue_prose(
    State(state): State<AppState>,
    Path((id, chapter_id, beat_id)): Path<(i64, i64, i64)>,
    Json(payload): Json<GenerateRequest>,
) -> AppResult<Json<StoryDetail>> {
    let job = db::prepare_continue_prose(&state.pool, id, chapter_id, beat_id, &payload).await?;
    enqueue_story_generation(&state.queue, job).await?;
    Ok(Json(db::get_story_detail(&state.pool, id).await?))
}

async fn align_beat_prose(
    State(state): State<AppState>,
    Path((id, chapter_id, beat_id)): Path<(i64, i64, i64)>,
    Json(payload): Json<GenerateRequest>,
) -> AppResult<Json<Job>> {
    let settings = db::get_settings(&state.pool).await?;
    let job = state
        .queue
        .enqueue_story_beat_prose_recheck(
            &state.pool,
            id,
            chapter_id,
            beat_id,
            &payload.guidance_notes,
            &settings,
        )
        .await?;
    db::touch_story(&state.pool, id).await?;
    Ok(Json(job))
}

async fn list_story_variables(
    State(state): State<AppState>,
    Path(id): Path<i64>,
) -> AppResult<Json<Vec<StoryVariable>>> {
    let _ = db::get_story(&state.pool, id).await?;
    Ok(Json(db::list_story_variables(&state.pool, id).await?))
}

async fn upsert_story_variable(
    State(state): State<AppState>,
    Path(id): Path<i64>,
    Json(payload): Json<StoryVariableUpdate>,
) -> AppResult<Json<StoryVariable>> {
    let _ = db::get_story(&state.pool, id).await?;
    Ok(Json(
        db::upsert_story_variable_manual(&state.pool, id, payload).await?,
    ))
}

async fn delete_story_variable(
    State(state): State<AppState>,
    Path((id, variable_id)): Path<(i64, i64)>,
) -> AppResult<Json<OkResponse>> {
    let _ = db::get_story(&state.pool, id).await?;
    let key = db::get_story_variable(&state.pool, id, variable_id)
        .await?
        .key;
    db::delete_story_variable(&state.pool, id, variable_id).await?;
    strip_variable_key_from_story_beats(&state.pool, id, &key).await?;
    db::touch_story(&state.pool, id).await?;
    Ok(Json(OkResponse { ok: true }))
}

async fn summarize_chapter_prose(
    State(state): State<AppState>,
    Path((id, chapter_id)): Path<(i64, i64)>,
) -> AppResult<Json<StoryDetail>> {
    let job = db::prepare_summarize_chapter(&state.pool, id, chapter_id).await?;
    enqueue_story_generation(&state.queue, job).await?;
    Ok(Json(db::get_story_detail(&state.pool, id).await?))
}

async fn recheck_beat_variables(
    State(state): State<AppState>,
    Path((id, chapter_id, beat_id)): Path<(i64, i64, i64)>,
    Json(payload): Json<GenerateRequest>,
) -> AppResult<Json<Job>> {
    let settings = db::get_settings(&state.pool).await?;
    let job = state
        .queue
        .enqueue_story_beat_variable_recheck(
            &state.pool,
            id,
            chapter_id,
            beat_id,
            &payload.guidance_notes,
            &settings,
        )
        .await?;
    db::touch_story(&state.pool, id).await?;
    Ok(Json(job))
}

async fn stream_story(
    State(state): State<AppState>,
    Path(id): Path<i64>,
) -> AppResult<impl IntoResponse> {
    let _ = db::get_story(&state.pool, id).await?;
    let pool = state.pool.clone();
    let interval = Duration::from_millis(state.sse_poll_interval_ms);

    let event_stream = stream! {
        let mut last_payload = String::new();
        loop {
            let detail = match db::get_story_detail(&pool, id).await {
                Ok(detail) => detail,
                Err(_) => {
                    yield Ok::<_, Infallible>(Event::default().event("error").data("{\"detail\":\"not found\"}"));
                    break;
                }
            };
            let active_job = db::get_active_story_job(&pool, id).await.ok().flatten();
            let has_active_job = active_job.is_some();
            let payload = serde_json::to_string(&StoryStreamPayload {
                detail: detail.clone(),
                active_job: active_job.clone(),
            }).unwrap_or_default();

            if payload != last_payload {
                last_payload = payload.clone();
                yield Ok(Event::default().data(payload));
            }

            if !has_active_job {
                yield Ok(Event::default().event("idle").data(format!("{{\"story_id\":{id}}}")));
                break;
            }

            tokio::time::sleep(interval).await;
        }
    };

    Ok(Sse::new(event_stream).keep_alive(KeepAlive::default()))
}
