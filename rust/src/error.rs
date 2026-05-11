use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum AppError {
    #[error("not found")]
    NotFound,

    #[error(transparent)]
    Sqlx(#[from] sqlx::Error),

    #[error(transparent)]
    Other(#[from] anyhow::Error),
}

pub type AppResult<T> = Result<T, AppError>;

impl IntoResponse for AppError {
    fn into_response(self) -> Response {
        let (status, body) = match self {
            AppError::NotFound => (StatusCode::NOT_FOUND, "Not found".to_string()),
            AppError::Sqlx(sqlx::Error::RowNotFound) => {
                (StatusCode::NOT_FOUND, "Not found".to_string())
            }
            AppError::Sqlx(e) => {
                tracing::error!(error = %e, "sqlx error");
                (StatusCode::INTERNAL_SERVER_ERROR, "Internal error".to_string())
            }
            AppError::Other(e) => {
                tracing::error!(error = %e, "internal error");
                (StatusCode::INTERNAL_SERVER_ERROR, "Internal error".to_string())
            }
        };
        (status, body).into_response()
    }
}
