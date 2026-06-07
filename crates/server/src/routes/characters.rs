use axum::{
    extract::{Multipart, Path, State},
    routing::{get, post},
    Json, Router,
};
use dreamwell_types::{
    Character, CharacterCreate, CharacterUpdate, ImportCharacterResponse, OkResponse,
};

use crate::character_import::parse_character_import;
use crate::db;
use crate::error::{AppError, AppResult};
use crate::routes::AppState;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/", get(list_characters).post(create_character))
        .route("/import", post(import_character))
        .route(
            "/:id",
            get(get_character)
                .patch(update_character)
                .delete(delete_character),
        )
}

async fn list_characters(State(state): State<AppState>) -> AppResult<Json<Vec<Character>>> {
    Ok(Json(db::list_characters(&state.pool).await?))
}

async fn create_character(
    State(state): State<AppState>,
    Json(payload): Json<CharacterCreate>,
) -> AppResult<Json<Character>> {
    Ok(Json(db::create_character(&state.pool, payload).await?))
}

async fn get_character(
    State(state): State<AppState>,
    Path(id): Path<i64>,
) -> AppResult<Json<Character>> {
    Ok(Json(db::get_character(&state.pool, id).await?))
}

async fn update_character(
    State(state): State<AppState>,
    Path(id): Path<i64>,
    Json(payload): Json<CharacterUpdate>,
) -> AppResult<Json<Character>> {
    Ok(Json(db::update_character(&state.pool, id, payload).await?))
}

async fn delete_character(
    State(state): State<AppState>,
    Path(id): Path<i64>,
) -> AppResult<Json<OkResponse>> {
    db::delete_character(&state.pool, id).await?;
    Ok(Json(OkResponse { ok: true }))
}

async fn import_character(
    State(state): State<AppState>,
    mut multipart: Multipart,
) -> AppResult<Json<ImportCharacterResponse>> {
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
    let payload = parse_character_import(&filename, &content)?;
    let character = db::create_character(&state.pool, payload).await?;
    let source = if filename.to_lowercase().ends_with(".png") {
        "png"
    } else {
        "json"
    };
    Ok(Json(ImportCharacterResponse {
        character,
        source: source.to_string(),
    }))
}
