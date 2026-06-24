use axum::{
    extract::{Multipart, Path, State},
    http::{header, HeaderMap, HeaderValue},
    response::IntoResponse,
    routing::{get, post},
    Json, Router,
};
use dreamwell_types::{
    GenerateCharacterStateRequest, GenerateCharacterStateResponse, ImportScenarioResponse,
    OkResponse, Scenario, ScenarioCreate, ScenarioExport, ScenarioUpdate,
};

use crate::error::{AppError, AppResult};
use crate::routes::AppState;
use crate::scenario_character_state;
use crate::scenario_db;
use crate::scenario_import::{import_source_label, scenario_create_from_import};

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/", get(list_scenarios).post(create_scenario))
        .route("/import", post(import_scenario))
        .route("/generate-character-state", post(generate_character_state))
        .route("/:id/export", get(export_scenario))
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

async fn generate_character_state(
    State(state): State<AppState>,
    Json(payload): Json<GenerateCharacterStateRequest>,
) -> AppResult<Json<GenerateCharacterStateResponse>> {
    let settings = crate::db::get_settings(&state.pool).await?;
    Ok(Json(
        scenario_character_state::generate_character_state(&state.pool, &payload, &settings)
            .await?,
    ))
}

async fn export_scenario(
    State(state): State<AppState>,
    Path(id): Path<i64>,
) -> AppResult<impl IntoResponse> {
    let scenario = scenario_db::get_scenario(&state.pool, id).await?;
    let export = ScenarioExport::from_scenario(&scenario);
    let filename = export_filename(&scenario.title);
    let mut headers = HeaderMap::new();
    headers.insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static("application/json"),
    );
    headers.insert(
        header::CONTENT_DISPOSITION,
        HeaderValue::from_str(&format!("attachment; filename=\"{filename}\""))
            .map_err(|e| AppError::internal(e.to_string()))?,
    );
    Ok((headers, Json(export)))
}

fn export_filename(title: &str) -> String {
    let slug: String = title
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() {
                ch.to_ascii_lowercase()
            } else {
                '-'
            }
        })
        .collect();
    let slug = slug.trim_matches('-');
    if slug.is_empty() {
        "scenario.json".to_string()
    } else {
        format!("{slug}.json")
    }
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
    if content.is_empty() {
        return Err(AppError::bad_request("No file uploaded"));
    }
    let source = import_source_label(&filename, &content)?;
    let payload = scenario_create_from_import(&filename, &content)?;
    let scenario = scenario_db::create_scenario(&state.pool, payload).await?;
    Ok(Json(ImportScenarioResponse {
        scenario,
        source: source.to_string(),
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn export_filename_slugifies_title() {
        assert_eq!(export_filename("Crystal Quest"), "crystal-quest.json");
        assert_eq!(export_filename("   "), "scenario.json");
    }
}
