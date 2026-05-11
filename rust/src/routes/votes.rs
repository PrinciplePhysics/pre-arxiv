use axum::extract::{Form, State};
use axum::http::HeaderMap;
use axum::response::{IntoResponse, Redirect, Response};
use serde::Deserialize;
use tower_sessions::Session;

use crate::auth::{verify_csrf, RequireUser};
use crate::error::AppResult;
use crate::helpers::set_flash;
use crate::state::AppState;

#[derive(Deserialize)]
pub struct VoteForm {
    pub csrf_token: String,
    pub target_type: String,
    pub target_id: i64,
    pub value: i64,
}

pub async fn vote(
    State(state): State<AppState>,
    session: Session,
    RequireUser(user): RequireUser,
    headers: HeaderMap,
    Form(form): Form<VoteForm>,
) -> AppResult<Response> {
    let back = headers
        .get("referer")
        .and_then(|v| v.to_str().ok())
        .map(|s| s.to_string())
        .unwrap_or_else(|| "/".to_string());

    if !verify_csrf(&session, &form.csrf_token).await {
        set_flash(&session, "Form expired — please try again.").await;
        return Ok(Redirect::to(&back).into_response());
    }
    if !matches!(form.target_type.as_str(), "manuscript" | "comment") {
        return Ok(Redirect::to(&back).into_response());
    }
    if !matches!(form.value, -1 | 1) {
        return Ok(Redirect::to(&back).into_response());
    }

    let mut tx = state.pool.begin().await?;

    // Upsert vote — clicking the same direction twice flips to neutral (delete).
    let existing: Option<(i64, i64)> = sqlx::query_as::<_, (i64, i64)>(
        "SELECT id, value FROM votes WHERE user_id = ? AND target_type = ? AND target_id = ?",
    )
    .bind(user.id)
    .bind(&form.target_type)
    .bind(form.target_id)
    .fetch_optional(&mut *tx)
    .await?;

    match existing {
        Some((vote_id, prev)) if prev == form.value => {
            sqlx::query("DELETE FROM votes WHERE id = ?")
                .bind(vote_id)
                .execute(&mut *tx)
                .await?;
        }
        Some((vote_id, _)) => {
            sqlx::query("UPDATE votes SET value = ? WHERE id = ?")
                .bind(form.value)
                .bind(vote_id)
                .execute(&mut *tx)
                .await?;
        }
        None => {
            sqlx::query(
                "INSERT INTO votes (user_id, target_type, target_id, value) VALUES (?, ?, ?, ?)",
            )
            .bind(user.id)
            .bind(&form.target_type)
            .bind(form.target_id)
            .bind(form.value)
            .execute(&mut *tx)
            .await?;
        }
    }

    // Recompute score from the votes table.
    let score: Option<(i64,)> = sqlx::query_as::<_, (i64,)>(
        "SELECT COALESCE(SUM(value), 0) FROM votes WHERE target_type = ? AND target_id = ?",
    )
    .bind(&form.target_type)
    .bind(form.target_id)
    .fetch_optional(&mut *tx)
    .await?;
    let score = score.map(|(s,)| s).unwrap_or(0);

    // Defence in depth: enumerate the two valid target types as exact
    // string literals rather than interpolating `form.target_type` into
    // SQL — even though the input is already validated to be one of two
    // values, never let a future refactor turn this into an injection.
    match form.target_type.as_str() {
        "manuscript" => {
            sqlx::query("UPDATE manuscripts SET score = ? WHERE id = ?")
                .bind(score).bind(form.target_id)
                .execute(&mut *tx).await?;
        }
        "comment" => {
            sqlx::query("UPDATE comments SET score = ? WHERE id = ?")
                .bind(score).bind(form.target_id)
                .execute(&mut *tx).await?;
        }
        _ => unreachable!("target_type validated above"),
    }

    tx.commit().await?;
    Ok(Redirect::to(&back).into_response())
}
