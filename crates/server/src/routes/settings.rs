use axum::{
    extract::{Query, State},
    routing::get,
    Json, Router,
};
use dreamwell_types::{ModelCapabilities, ModelInfo, Settings, SettingsUpdate};
use serde::Deserialize;

use crate::db;
use crate::error::AppResult;
use crate::inference::{list_models, probe_model_capabilities};
use crate::routes::AppState;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/", get(get_settings).patch(patch_settings))
        .route("/models", get(get_models))
        .route("/model-capabilities", get(get_model_capabilities))
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
    let settings = db::get_settings(&state.pool).await?;
    Ok(Json(list_models(&settings.inference_url).await?))
}

#[derive(Debug, Deserialize)]
struct ModelCapabilitiesQuery {
    model: String,
}

async fn get_model_capabilities(
    State(state): State<AppState>,
    Query(query): Query<ModelCapabilitiesQuery>,
) -> AppResult<Json<ModelCapabilities>> {
    let settings = db::get_settings(&state.pool).await?;
    Ok(Json(
        probe_model_capabilities(&settings.inference_url, &query.model).await,
    ))
}
