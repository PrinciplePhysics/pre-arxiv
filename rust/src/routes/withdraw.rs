//! POST /m/{id}/withdraw — submitter (or admin) replaces the manuscript
//! body with a tombstone. The id, DOI, title, conductor metadata, and
//! the withdrawal reason remain so existing citations don't break.

use axum::extract::{Form, Path, State};
use axum::response::{IntoResponse, Redirect, Response};
use serde::Deserialize;
use tower_sessions::Session;

use crate::auth::{verify_csrf, RequireUser};
use crate::error::{AppError, AppResult};
use crate::helpers::set_flash;
use crate::state::AppState;

#[derive(Deserialize)]
pub struct WithdrawForm {
    pub csrf_token: String,
    #[serde(default)]
    pub reason: String,
}

pub async fn withdraw(
    State(state): State<AppState>,
    session: Session,
    RequireUser(user): RequireUser,
    Path(id): Path<String>,
    Form(form): Form<WithdrawForm>,
) -> AppResult<Response> {
    if !verify_csrf(&session, &form.csrf_token).await {
        set_flash(&session, "Form expired — please try again.").await;
        return Ok(Redirect::to(&format!("/m/{id}")).into_response());
    }

    let row: Option<(i64, i64, Option<String>, i64)> = sqlx::query_as(
        "SELECT id, submitter_id, arxiv_like_id, withdrawn
         FROM manuscripts
         WHERE arxiv_like_id = ? OR CAST(id AS TEXT) = ?
         LIMIT 1",
    )
    .bind(&id)
    .bind(&id)
    .fetch_optional(&state.pool)
    .await?;
    let (m_id, submitter_id, slug, already_withdrawn) = row.ok_or(AppError::NotFound)?;
    let slug = slug.unwrap_or_else(|| m_id.to_string());

    if already_withdrawn != 0 {
        set_flash(&session, "This manuscript is already withdrawn.").await;
        return Ok(Redirect::to(&format!("/m/{slug}")).into_response());
    }
    if submitter_id != user.id && !user.is_admin() {
        set_flash(&session, "Only the submitter (or an admin) may withdraw a manuscript.").await;
        return Ok(Redirect::to(&format!("/m/{slug}")).into_response());
    }

    let reason = form.reason.trim();
    let reason_opt = if reason.is_empty() {
        None
    } else {
        Some(reason.chars().take(500).collect::<String>())
    };

    sqlx::query(
        "UPDATE manuscripts
         SET withdrawn = 1, withdrawn_reason = ?, withdrawn_at = CURRENT_TIMESTAMP, updated_at = CURRENT_TIMESTAMP
         WHERE id = ?",
    )
    .bind(reason_opt.as_deref())
    .bind(m_id)
    .execute(&state.pool)
    .await?;

    let action = if user.is_admin() && submitter_id != user.id {
        "manuscript_withdraw_admin"
    } else {
        "manuscript_withdraw_self"
    };
    let _ = sqlx::query(
        "INSERT INTO audit_log (actor_user_id, action, target_type, target_id, detail) VALUES (?, ?, 'manuscript', ?, ?)",
    )
    .bind(user.id)
    .bind(action)
    .bind(m_id)
    .bind(reason_opt.as_deref())
    .execute(&state.pool)
    .await;

    set_flash(&session, "Manuscript withdrawn. The page now shows a tombstone.").await;
    Ok(Redirect::to(&format!("/m/{slug}")).into_response())
}
