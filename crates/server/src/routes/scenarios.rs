use axum::{
    extract::{Multipart, Path, State},
    routing::{get, post},
    Json, Router,
};
use dreamwell_types::{
    scenario_create_from_iw_json, ImportScenarioResponse, OkResponse, Scenario, ScenarioCreate,
    ScenarioUpdate,
};

use crate::character_import::parse_character_import;
use crate::error::{AppError, AppResult};
use crate::routes::AppState;
use crate::scenario_db;
use crate::scenario_import::scenario_create_from_character;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/", get(list_scenarios).post(create_scenario))
        .route("/import", post(import_scenario))
        .route("/import-iw", post(import_iw_scenario))
        .route(
            "/:id",
            get(get_scenario)
                .patch(update_scenario)
                .delete(delete_scenario),
        )
}

async fn list_scenarios(State(state): State<AppState>) -> AppResult<Json<Vec<Scenario>>> {
    Ok(Json(scenario_db::list_scenarios(&state.pool).await?))
}

async fn create_scenario(
    State(state): State<AppState>,
    Json(payload): Json<ScenarioCreate>,
) -> AppResult<Json<Scenario>> {
    Ok(Json(
        scenario_db::create_scenario(&state.pool, payload).await?,
    ))
}

async fn get_scenario(
    State(state): State<AppState>,
    Path(id): Path<i64>,
) -> AppResult<Json<Scenario>> {
    Ok(Json(scenario_db::get_scenario(&state.pool, id).await?))
}

async fn update_scenario(
    State(state): State<AppState>,
    Path(id): Path<i64>,
    Json(payload): Json<ScenarioUpdate>,
) -> AppResult<Json<Scenario>> {
    Ok(Json(
        scenario_db::update_scenario(&state.pool, id, payload).await?,
    ))
}

async fn delete_scenario(
    State(state): State<AppState>,
    Path(id): Path<i64>,
) -> AppResult<Json<OkResponse>> {
    scenario_db::delete_scenario(&state.pool, id).await?;
    Ok(Json(OkResponse { ok: true }))
}

async fn import_scenario(
    State(state): State<AppState>,
    mut multipart: Multipart,
) -> AppResult<Json<ImportScenarioResponse>> {
    let mut filename = "scenario.json".to_string();
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
    let payload = scenario_create_from_character(card);
    let scenario = scenario_db::create_scenario(&state.pool, payload).await?;
    let source = if filename.to_lowercase().ends_with(".png") {
        "png"
    } else {
        "json"
    };
    Ok(Json(ImportScenarioResponse {
        scenario,
        source: source.to_string(),
    }))
}

async fn import_iw_scenario(
    State(state): State<AppState>,
    mut multipart: Multipart,
) -> AppResult<Json<ImportScenarioResponse>> {
    let mut content = Vec::new();
    while let Some(field) = multipart
        .next_field()
        .await
        .map_err(|e| AppError::bad_request(e.to_string()))?
    {
        if field.name() == Some("file") {
            content = field
                .bytes()
                .await
                .map_err(|e| AppError::bad_request(e.to_string()))?
                .to_vec();
        }
    }
    if content.is_empty() {
        return Err(AppError::bad_request("No file uploaded"));
    }
    let json = std::str::from_utf8(&content)
        .map_err(|e| AppError::bad_request(format!("Invalid UTF-8 in upload: {e}")))?;
    let payload = scenario_create_from_iw_json(json)
        .map_err(|e| AppError::bad_request(format!("Invalid IW export JSON: {e}")))?;
    let scenario = scenario_db::create_scenario(&state.pool, payload).await?;
    Ok(Json(ImportScenarioResponse {
        scenario,
        source: "iw".to_string(),
    }))
}
