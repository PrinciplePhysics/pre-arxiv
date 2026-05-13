//! /u/{username}/follow and /u/{username}/unfollow.

use axum::extract::{Form, Path, State};
use axum::response::{IntoResponse, Redirect, Response};
use serde::Deserialize;
use tower_sessions::Session;

use crate::auth::{verify_csrf, RequireUser};
use crate::error::{AppError, AppResult};
use crate::helpers::set_flash;
use crate::state::AppState;

#[derive(Deserialize)]
pub struct FollowForm { pub csrf_token: String }

pub async fn follow(
    State(state): State<AppState>,
    session: Session,
    RequireUser(me): RequireUser,
    Path(username): Path<String>,
    Form(form): Form<FollowForm>,
) -> AppResult<Response> {
    if !verify_csrf(&session, &form.csrf_token).await {
        set_flash(&session, "Form expired — please try again.").await;
        return Ok(Redirect::to(&format!("/u/{username}")).into_response());
    }
    if !me.is_verified_or_admin() {
        set_flash(&session, "Verify your email before following users.").await;
        return Ok(Redirect::to(&format!("/u/{username}")).into_response());
    }
    let target: Option<(i64,)> = sqlx::query_as("SELECT id FROM users WHERE username = ?")
        .bind(&username)
        .fetch_optional(&state.pool)
        .await?;
    let target_id = target.ok_or(AppError::NotFound)?.0;
    if target_id == me.id {
        set_flash(&session, "You can't follow yourself.").await;
        return Ok(Redirect::to(&format!("/u/{username}")).into_response());
    }
    let res = sqlx::query(
        "INSERT INTO follows (follower_id, followee_id) VALUES (?, ?)
         ON CONFLICT(follower_id, followee_id) DO NOTHING",
    )
    .bind(me.id)
    .bind(target_id)
    .execute(&state.pool)
    .await?;
    // Notify the followee, but only on a brand-new follow (rows_affected
    // == 0 means the ON CONFLICT DO NOTHING fired — already following,
    // no notification needed).
    if res.rows_affected() > 0 {
        let _ = crate::notifications::notify(
            &state.pool,
            target_id,
            Some(me.id),
            crate::notifications::KIND_FOLLOWED,
            Some("user"),
            Some(target_id),
            None,
        ).await;
    }
    set_flash(&session, format!("Following @{username}.")).await;
    Ok(Redirect::to(&format!("/u/{username}")).into_response())
}

pub async fn unfollow(
    State(state): State<AppState>,
    session: Session,
    RequireUser(me): RequireUser,
    Path(username): Path<String>,
    Form(form): Form<FollowForm>,
) -> AppResult<Response> {
    if !verify_csrf(&session, &form.csrf_token).await {
        return Ok(Redirect::to(&format!("/u/{username}")).into_response());
    }
    let target: Option<(i64,)> = sqlx::query_as("SELECT id FROM users WHERE username = ?")
        .bind(&username)
        .fetch_optional(&state.pool)
        .await?;
    let target_id = target.ok_or(AppError::NotFound)?.0;
    sqlx::query("DELETE FROM follows WHERE follower_id = ? AND followee_id = ?")
        .bind(me.id)
        .bind(target_id)
        .execute(&state.pool)
        .await?;
    set_flash(&session, format!("Unfollowed @{username}.")).await;
    Ok(Redirect::to(&format!("/u/{username}")).into_response())
}
