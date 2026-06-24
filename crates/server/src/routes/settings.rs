use axum::{
    extract::{Path, Query, State},
    routing::get,
    Json, Router,
};
use dreamwell_types::{
    InferenceConnection, InferenceConnectionCreate, InferenceConnectionUpdate, ModelCapabilities,
    ModelInfo, Settings, SettingsUpdate,
};
use serde::Deserialize;

use crate::db;
use crate::error::AppResult;
use crate::inference::{list_models, probe_model_capabilities};
use crate::routes::AppState;
use crate::tool_stream::list_tool_parsers;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/", get(get_settings).patch(patch_settings))
        .route("/models", get(get_models))
        .route("/tool-parsers", get(get_tool_parsers))
        .route("/model-capabilities", get(get_model_capabilities))
        .route(
            "/connections",
            get(list_connections).post(create_connection),
        )
        .route(
            "/connections/:id/clone",
            axum::routing::post(clone_connection),
        )
        .route(
            "/connections/:id",
            get(get_connection)
                .patch(update_connection)
                .delete(delete_connection),
        )
}

async fn get_settings(State(state): State<AppState>) -> AppResult<Json<Settings>> {
    Ok(Json(db::get_settings(&state.pool).await?))
}

async fn patch_settings(
    State(state): State<AppState>,
    Json(payload): Json<SettingsUpdate>,
) -> AppResult<Json<Settings>> {
    Ok(Json(db::update_settings(&state.pool, payload).await?))
}

async fn get_models(State(state): State<AppState>) -> AppResult<Json<Vec<ModelInfo>>> {
    let inference = db::get_inference_config(&state.pool).await?;
    Ok(Json(list_models(&inference).await?))
}

async fn get_tool_parsers() -> AppResult<Json<Vec<String>>> {
    Ok(Json(
        list_tool_parsers()
            .into_iter()
            .map(str::to_string)
            .collect(),
    ))
}

#[derive(Debug, Deserialize)]
struct ModelCapabilitiesQuery {
    model: String,
}

async fn get_model_capabilities(
    State(state): State<AppState>,
    Query(query): Query<ModelCapabilitiesQuery>,
) -> AppResult<Json<ModelCapabilities>> {
    let inference = db::get_inference_config(&state.pool).await?;
    Ok(Json(
        probe_model_capabilities(&inference, &query.model).await,
    ))
}

async fn list_connections(
    State(state): State<AppState>,
) -> AppResult<Json<Vec<InferenceConnection>>> {
    Ok(Json(db::list_inference_connections(&state.pool).await?))
}

async fn create_connection(
    State(state): State<AppState>,
    Json(payload): Json<InferenceConnectionCreate>,
) -> AppResult<Json<InferenceConnection>> {
    Ok(Json(
        db::create_inference_connection(&state.pool, payload).await?,
    ))
}

async fn get_connection(
    State(state): State<AppState>,
    Path(id): Path<i64>,
) -> AppResult<Json<InferenceConnection>> {
    Ok(Json(db::get_inference_connection(&state.pool, id).await?))
}

async fn update_connection(
    State(state): State<AppState>,
    Path(id): Path<i64>,
    Json(payload): Json<InferenceConnectionUpdate>,
) -> AppResult<Json<InferenceConnection>> {
    Ok(Json(
        db::update_inference_connection(&state.pool, id, payload).await?,
    ))
}

async fn delete_connection(State(state): State<AppState>, Path(id): Path<i64>) -> AppResult<()> {
    db::delete_inference_connection(&state.pool, id).await
}

async fn clone_connection(
    State(state): State<AppState>,
    Path(id): Path<i64>,
) -> AppResult<Json<InferenceConnection>> {
    Ok(Json(db::clone_inference_connection(&state.pool, id).await?))
}
