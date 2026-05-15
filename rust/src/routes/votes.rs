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
    // Same-origin-only redirect target derived from Referer. Browsers
    // populate Referer with the full URL of the page that submitted the
    // form, including scheme + host. We accept it only when it's a path
    // (no scheme, no host), defending against a malicious page that
    // managed to plant a forged Referer turning /vote into an open
    // redirect to an attacker-controlled site.
    let back = headers
        .get("referer")
        .and_then(|v| v.to_str().ok())
        .and_then(safe_back_path)
        .unwrap_or_else(|| "/".to_string());

    if !verify_csrf(&session, &form.csrf_token).await {
        set_flash(&session, "Form expired — please try again.").await;
        return Ok(Redirect::to(&back).into_response());
    }
    if !user.is_verified_or_admin() {
        set_flash(&session, "Connect GitHub or verify email before voting.").await;
        return Ok(Redirect::to(&back).into_response());
    }
    if !matches!(form.target_type.as_str(), "manuscript" | "comment") {
        return Ok(Redirect::to(&back).into_response());
    }
    // Three legal values:
    //   -1, +1 → cast or replace the existing vote (or, if it already
    //            equals the new value, toggle to neutral — Reddit-style).
    //    0    → explicit clear. REST-friendly "DELETE my vote." Sent by
    //            JSON clients and by the topbar's "neutral" click target.
    if !matches!(form.value, -1..=1) {
        return Ok(Redirect::to(&back).into_response());
    }

    // Reject votes on withdrawn manuscripts. The HTML hides the vote
    // buttons for withdrawn rows, but a hand-crafted POST would otherwise
    // succeed. Withdrawal is the user's signal that the manuscript should
    // no longer accrue social signal.
    if form.target_type == "manuscript" {
        let w: Option<(i64,)> = sqlx::query_as(crate::db::pg(
            "SELECT withdrawn FROM manuscripts WHERE id = ?",
        ))
        .bind(form.target_id)
        .fetch_optional(&state.pool)
        .await?;
        if matches!(w, Some((1,))) {
            set_flash(
                &session,
                "This manuscript has been withdrawn and can no longer be voted on.",
            )
            .await;
            return Ok(Redirect::to(&back).into_response());
        }
    }

    let mut tx = state.pool.begin().await?;

    // Upsert vote — clicking the same direction twice flips to neutral (delete).
    let existing: Option<(i64, i64)> = sqlx::query_as::<_, (i64, i64)>(crate::db::pg(
        "SELECT id, value FROM votes WHERE user_id = ? AND target_type = ? AND target_id = ?",
    ))
    .bind(user.id)
    .bind(&form.target_type)
    .bind(form.target_id)
    .fetch_optional(&mut *tx)
    .await?;

    match (existing, form.value) {
        // Explicit clear (value=0) on an existing row, OR re-posting the
        // same direction (Reddit-style toggle to neutral). Both delete.
        (Some((vote_id, _)), 0) => {
            sqlx::query(crate::db::pg("DELETE FROM votes WHERE id = ?"))
                .bind(vote_id)
                .execute(&mut *tx)
                .await?;
        }
        (Some((vote_id, prev)), v) if prev == v => {
            sqlx::query(crate::db::pg("DELETE FROM votes WHERE id = ?"))
                .bind(vote_id)
                .execute(&mut *tx)
                .await?;
        }
        // Change direction: update the row in place.
        (Some((vote_id, _)), v) => {
            sqlx::query(crate::db::pg("UPDATE votes SET value = ? WHERE id = ?"))
                .bind(v)
                .bind(vote_id)
                .execute(&mut *tx)
                .await?;
        }
        // No existing vote + clear request → nothing to do; recompute
        // below still runs so the response is consistent.
        (None, 0) => {}
        (None, v) => {
            sqlx::query(crate::db::pg(
                "INSERT INTO votes (user_id, target_type, target_id, value) VALUES (?, ?, ?, ?)",
            ))
            .bind(user.id)
            .bind(&form.target_type)
            .bind(form.target_id)
            .bind(v)
            .execute(&mut *tx)
            .await?;
        }
    }

    // Recompute score from the votes table.
    let score: Option<(i64,)> = sqlx::query_as::<_, (i64,)>(crate::db::pg(
        "SELECT COALESCE(SUM(value), 0) FROM votes WHERE target_type = ? AND target_id = ?",
    ))
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
            sqlx::query(crate::db::pg(
                "UPDATE manuscripts SET score = ? WHERE id = ?",
            ))
            .bind(score)
            .bind(form.target_id)
            .execute(&mut *tx)
            .await?;
        }
        "comment" => {
            sqlx::query(crate::db::pg("UPDATE comments SET score = ? WHERE id = ?"))
                .bind(score)
                .bind(form.target_id)
                .execute(&mut *tx)
                .await?;
        }
        _ => unreachable!("target_type validated above"),
    }

    tx.commit().await?;
    Ok(Redirect::to(&back).into_response())
}

/// Extract a same-origin path from a possibly-absolute Referer URL.
///
/// Accepts a path starting with `/` (but not `//` which browsers treat as
/// protocol-relative cross-origin) and rejects anything with scheme,
/// authority, CR/LF, or excessive length. Strips an `http(s)://host`
/// prefix and keeps just the path so a Referer of
/// `https://attacker.example/x` collapses to a safe default.
fn safe_back_path(s: &str) -> Option<String> {
    let path_part = if let Some(rest) = s
        .strip_prefix("https://")
        .or_else(|| s.strip_prefix("http://"))
    {
        // Drop scheme+authority — keep only path-and-query.
        match rest.find('/') {
            Some(i) => &rest[i..],
            None => "/",
        }
    } else {
        s
    };
    if path_part.starts_with('/')
        && !path_part.starts_with("//")
        && !path_part.starts_with("/\\")
        && !path_part.contains('\n')
        && !path_part.contains('\r')
        && path_part.len() <= 512
    {
        Some(path_part.to_string())
    } else {
        None
    }
}
