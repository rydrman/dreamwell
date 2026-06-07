use axum::{
    extract::State,
    routing::get,
    Json, Router,
};
use dreamwell_types::{ModelInfo, Settings, SettingsUpdate};

use crate::db;
use crate::error::AppResult;
use crate::inference::list_models;
use crate::routes::AppState;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/", get(get_settings).patch(patch_settings))
        .route("/models", get(get_models))
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
