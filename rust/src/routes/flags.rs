//! User-facing flag/report endpoints. Submits a row to `flag_reports`
//! (schema in migration 0009); the admin queue at /admin/flags picks
//! up unresolved rows. Each (reporter, target) pair is UNIQUE in the
//! schema, so a single user can't flood the queue by re-flagging the
//! same item — repeat clicks are absorbed silently.
//!
//! Two surfaces:
//!
//!   POST /m/{id}/flag    — flag a manuscript by its arxiv-like id
//!   POST /c/{id}/flag    — flag a comment by its DB id
//!
//! Both require a logged-in account (RequireUser) and CSRF. Reason is
//! free-form text capped at 500 chars. We don't gate on email-verified
//! here — flagging is a low-trust action and the rate limiter caps
//! abuse from a single IP. Anonymous reports are not supported, by
//! design: every flag carries a reporter we can trace if the system
//! is abused.

use axum::extract::{Form, Path, State};
use axum::response::{IntoResponse, Redirect, Response};
use serde::Deserialize;
use tower_sessions::Session;

use crate::auth::{verify_csrf, RequireUser};
use crate::error::{AppError, AppResult};
use crate::helpers::set_flash;
use crate::state::AppState;

#[derive(Deserialize)]
pub struct FlagForm {
    pub csrf_token: String,
    #[serde(default)]
    pub reason: String,
}

const MAX_REASON_LEN: usize = 500;

fn clean_reason(raw: &str) -> String {
    let mut r = raw.trim().to_string();
    if r.len() > MAX_REASON_LEN {
        r.truncate(MAX_REASON_LEN);
    }
    if r.is_empty() {
        // Default reason when the form is submitted with no text.
        // Keeps the row valid (`reason` is NOT NULL) and tells the
        // moderator this was a one-click flag with no context.
        r = "(no reason given)".to_string();
    }
    r
}

/// POST /m/{id}/flag — flag a manuscript.
pub async fn flag_manuscript(
    State(state): State<AppState>,
    session: Session,
    RequireUser(user): RequireUser,
    Path(id): Path<String>,
    Form(form): Form<FlagForm>,
) -> AppResult<Response> {
    let row: Option<(i64, String)> = sqlx::query_as::<_, (i64, String)>(
        "SELECT id, COALESCE(arxiv_like_id, CAST(id AS TEXT))
           FROM manuscripts
          WHERE arxiv_like_id = ? OR CAST(id AS TEXT) = ?
          LIMIT 1",
    )
    .bind(&id)
    .bind(&id)
    .fetch_optional(&state.pool)
    .await?;
    let Some((manuscript_id, slug)) = row else {
        return Err(AppError::NotFound);
    };
    let back = format!("/m/{slug}");

    if !verify_csrf(&session, &form.csrf_token).await {
        set_flash(&session, "Form expired — please try again.").await;
        return Ok(Redirect::to(&back).into_response());
    }

    let reason = clean_reason(&form.reason);
    insert_flag(&state.pool, "manuscript", manuscript_id, user.id, &reason).await?;

    // Notify all admins. Notification recipient must exist, so we
    // fan out instead of a single broadcast row.
    let admin_ids: Vec<(i64,)> = sqlx::query_as("SELECT id FROM users WHERE is_admin = 1")
        .fetch_all(&state.pool)
        .await
        .unwrap_or_default();
    let snippet: String = format!("Flag on manuscript {slug}: {}", first_chars(&reason, 80));
    for (aid,) in admin_ids {
        let _ = crate::notifications::notify(
            &state.pool,
            aid,
            Some(user.id),
            "flag_filed",
            Some("manuscript"),
            Some(manuscript_id),
            Some(&snippet),
        )
        .await;
    }

    set_flash(
        &session,
        "Thanks — your report was logged. A moderator will review it.",
    )
    .await;
    Ok(Redirect::to(&back).into_response())
}

/// POST /c/{id}/flag — flag a comment.
pub async fn flag_comment(
    State(state): State<AppState>,
    session: Session,
    RequireUser(user): RequireUser,
    Path(comment_id): Path<i64>,
    Form(form): Form<FlagForm>,
) -> AppResult<Response> {
    let row: Option<(i64, String)> = sqlx::query_as::<_, (i64, String)>(
        "SELECT c.manuscript_id,
                COALESCE(m.arxiv_like_id, CAST(m.id AS TEXT))
           FROM comments c JOIN manuscripts m ON m.id = c.manuscript_id
          WHERE c.id = ?",
    )
    .bind(comment_id)
    .fetch_optional(&state.pool)
    .await?;
    let Some((_manuscript_id, slug)) = row else {
        return Err(AppError::NotFound);
    };
    let back = format!("/m/{slug}#comment-{comment_id}");

    if !verify_csrf(&session, &form.csrf_token).await {
        set_flash(&session, "Form expired — please try again.").await;
        return Ok(Redirect::to(&back).into_response());
    }

    let reason = clean_reason(&form.reason);
    insert_flag(&state.pool, "comment", comment_id, user.id, &reason).await?;

    let admin_ids: Vec<(i64,)> = sqlx::query_as("SELECT id FROM users WHERE is_admin = 1")
        .fetch_all(&state.pool)
        .await
        .unwrap_or_default();
    let snippet = format!(
        "Flag on comment #{comment_id} (on {slug}): {}",
        first_chars(&reason, 80)
    );
    for (aid,) in admin_ids {
        let _ = crate::notifications::notify(
            &state.pool,
            aid,
            Some(user.id),
            "flag_filed",
            Some("comment"),
            Some(comment_id),
            Some(&snippet),
        )
        .await;
    }

    set_flash(
        &session,
        "Thanks — your report was logged. A moderator will review it.",
    )
    .await;
    Ok(Redirect::to(&back).into_response())
}

/// Single-row insert with idempotency via the UNIQUE(target_type,
/// target_id, reporter_id) constraint. Silently swallow the
/// uniqueness violation — re-flagging the same target by the same
/// user is a no-op (not an error).
async fn insert_flag(
    pool: &sqlx::SqlitePool,
    target_type: &str,
    target_id: i64,
    reporter_id: i64,
    reason: &str,
) -> Result<(), sqlx::Error> {
    let res = sqlx::query(
        "INSERT OR IGNORE INTO flag_reports
            (target_type, target_id, reporter_id, reason)
         VALUES (?, ?, ?, ?)",
    )
    .bind(target_type)
    .bind(target_id)
    .bind(reporter_id)
    .bind(reason)
    .execute(pool)
    .await?;
    let _ = res.rows_affected(); // 0 if INSERT IGNORE absorbed it
    Ok(())
}

fn first_chars(s: &str, n: usize) -> String {
    s.chars().take(n).collect()
}
