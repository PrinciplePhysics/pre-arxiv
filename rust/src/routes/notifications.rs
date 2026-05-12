//! /me/notifications — list + mark-read.

use axum::extract::{Form, Path, State};
use axum::response::{Html, IntoResponse, Redirect, Response};
use serde::Deserialize;
use tower_sessions::Session;

use crate::auth::{verify_csrf, MaybeUser, RequireUser};
use crate::error::AppResult;
use crate::helpers::{build_ctx, set_flash};
use crate::notifications;
use crate::state::AppState;
use crate::templates;

pub async fn show(
    State(state): State<AppState>,
    session: Session,
    maybe_user: MaybeUser,
    RequireUser(user): RequireUser,
) -> AppResult<Html<String>> {
    let rows = notifications::list_for(&state.pool, user.id, 100)
        .await
        .map_err(|e| crate::error::AppError::Other(anyhow::anyhow!("{e}")))?;
    let mut ctx = build_ctx(&session, maybe_user, "/me/notifications").await;
    ctx.no_index = true;
    Ok(Html(templates::notifications::render(&ctx, &rows).into_string()))
}

#[derive(Deserialize)]
pub struct CsrfOnly { pub csrf_token: String }

pub async fn mark_read(
    State(state): State<AppState>,
    session: Session,
    RequireUser(user): RequireUser,
    Path(id): Path<i64>,
    Form(form): Form<CsrfOnly>,
) -> AppResult<Response> {
    if !verify_csrf(&session, &form.csrf_token).await {
        return Ok(Redirect::to("/me/notifications").into_response());
    }
    let _ = notifications::mark_read(&state.pool, user.id, id).await;
    Ok(Redirect::to("/me/notifications").into_response())
}

pub async fn mark_all_read(
    State(state): State<AppState>,
    session: Session,
    RequireUser(user): RequireUser,
    Form(form): Form<CsrfOnly>,
) -> AppResult<Response> {
    if !verify_csrf(&session, &form.csrf_token).await {
        return Ok(Redirect::to("/me/notifications").into_response());
    }
    let _ = notifications::mark_all_read(&state.pool, user.id).await;
    set_flash(&session, "All notifications marked as read.").await;
    Ok(Redirect::to("/me/notifications").into_response())
}
