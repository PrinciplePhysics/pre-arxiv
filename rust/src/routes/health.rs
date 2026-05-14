use axum::extract::State;
use axum::http::StatusCode;
use axum::Json;
use serde_json::json;

use crate::state::AppState;

pub async fn healthz() -> Json<serde_json::Value> {
    Json(json!({
        "status": "ok",
        "service": "prexiv",
        "check": "process"
    }))
}

pub async fn readyz(State(state): State<AppState>) -> (StatusCode, Json<serde_json::Value>) {
    let db_ok = sqlx::query_scalar::<_, i64>(crate::db::pg("SELECT 1::BIGINT"))
        .fetch_one(&state.pool)
        .await
        .map(|n| n == 1)
        .unwrap_or(false);

    if db_ok {
        (
            StatusCode::OK,
            Json(json!({
                "status": "ready",
                "service": "prexiv",
                "checks": { "database": "ok" }
            })),
        )
    } else {
        (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(json!({
                "status": "not_ready",
                "service": "prexiv",
                "checks": { "database": "failed" }
            })),
        )
    }
}
