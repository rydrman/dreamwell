use axum::{
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};
use serde_json::json;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum AppError {
    #[error("{0}")]
    BadRequest(String),
    #[error("{0}")]
    NotFound(String),
    #[error("{0}")]
    Inference(String),
    #[error("{0}")]
    Internal(String),
    #[error(transparent)]
    Sqlx(#[from] sqlx::Error),
    #[error(transparent)]
    Migrate(#[from] sqlx::migrate::MigrateError),
    #[error(transparent)]
    Reqwest(#[from] reqwest::Error),
    #[error(transparent)]
    Json(#[from] serde_json::Error),
}

impl AppError {
    pub fn bad_request(msg: impl Into<String>) -> Self {
        Self::BadRequest(msg.into())
    }

    pub fn not_found(msg: impl Into<String>) -> Self {
        Self::NotFound(msg.into())
    }

    pub fn inference(msg: impl Into<String>) -> Self {
        Self::Inference(msg.into())
    }

    pub fn internal(msg: impl Into<String>) -> Self {
        Self::Internal(msg.into())
    }
}

impl IntoResponse for AppError {
    fn into_response(self) -> Response {
        let (status, message) = match &self {
            AppError::BadRequest(msg) => (StatusCode::BAD_REQUEST, msg.clone()),
            AppError::NotFound(msg) => (StatusCode::NOT_FOUND, msg.clone()),
            AppError::Inference(msg) => (StatusCode::BAD_GATEWAY, msg.clone()),
            AppError::Internal(msg) => (StatusCode::INTERNAL_SERVER_ERROR, msg.clone()),
            AppError::Sqlx(err) => (StatusCode::INTERNAL_SERVER_ERROR, err.to_string()),
            AppError::Migrate(err) => (StatusCode::INTERNAL_SERVER_ERROR, err.to_string()),
            AppError::Reqwest(err) => (StatusCode::BAD_GATEWAY, err.to_string()),
            AppError::Json(err) => (StatusCode::BAD_REQUEST, err.to_string()),
        };
        (status, Json(json!({ "detail": message }))).into_response()
    }
}

pub type AppResult<T> = Result<T, AppError>;
