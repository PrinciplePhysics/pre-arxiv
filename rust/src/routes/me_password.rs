//! /me/password — change-password page for the logged-in user.
//!
//! Three-field form: current password, new password, confirm new. The
//! current-password check defends against session-hijack attacks (an
//! attacker with the session can't lock the legitimate owner out
//! without also knowing the existing password). On a successful
//! change we cycle the session id so old cookies stop authenticating.

use axum::extract::{Form, State};
use axum::response::{Html, IntoResponse, Redirect, Response};
use serde::Deserialize;
use tower_sessions::Session;

use crate::auth::{
    hash_password, is_password_pwned, login_session, verify_csrf,
    verify_password_timing_safe, MaybeUser, RequireUser,
};
use crate::error::AppResult;
use crate::helpers::{build_ctx, set_flash};
use crate::state::AppState;
use crate::templates;

pub async fn show(
    session: Session,
    maybe_user: MaybeUser,
    RequireUser(_user): RequireUser,
) -> AppResult<Html<String>> {
    let mut ctx = build_ctx(&session, maybe_user, "/me/password").await;
    ctx.no_index = true;
    Ok(Html(templates::me_password::render(&ctx, None).into_string()))
}

#[derive(Deserialize)]
pub struct ChangeForm {
    pub csrf_token: String,
    pub current_password: String,
    pub new_password: String,
    #[serde(default)]
    pub new_password_confirm: String,
}

pub async fn submit(
    State(state): State<AppState>,
    session: Session,
    maybe_user: MaybeUser,
    RequireUser(user): RequireUser,
    Form(form): Form<ChangeForm>,
) -> AppResult<Response> {
    let render_err = async |msg: &str, maybe_user: MaybeUser| -> Response {
        let mut ctx = build_ctx(&session, maybe_user, "/me/password").await;
        ctx.no_index = true;
        Html(templates::me_password::render(&ctx, Some(msg)).into_string()).into_response()
    };

    if !verify_csrf(&session, &form.csrf_token).await {
        return Ok(render_err("Form expired — please try again.", maybe_user).await);
    }

    // Pull the current password_hash. We don't reuse `user` (it's the
    // RequireUser snapshot without password_hash); fetch fresh.
    let row: Option<(String,)> =
        sqlx::query_as("SELECT password_hash FROM users WHERE id = ?")
            .bind(user.id)
            .fetch_optional(&state.pool)
            .await?;
    let Some((current_hash,)) = row else {
        // Should be impossible — RequireUser implies the row exists. Fail
        // loud rather than silently rendering a form.
        return Ok(render_err("Account not found. Please log out and back in.", maybe_user).await);
    };

    if !verify_password_timing_safe(&form.current_password, Some(&current_hash)) {
        return Ok(render_err("Current password is incorrect.", maybe_user).await);
    }
    if form.new_password.len() < 8 {
        return Ok(render_err("New password must be at least 8 characters.", maybe_user).await);
    }
    if form.new_password != form.new_password_confirm {
        return Ok(render_err(
            "The two new-password fields don't match. Re-type the confirmation.",
            maybe_user,
        ).await);
    }
    if form.new_password == form.current_password {
        return Ok(render_err(
            "New password must differ from the current one.",
            maybe_user,
        ).await);
    }
    if is_password_pwned(&form.new_password).await {
        return Ok(render_err(
            "That password appears in a known data breach. Please pick another.",
            maybe_user,
        ).await);
    }

    let new_hash = hash_password(&form.new_password)
        .map_err(|e| crate::error::AppError::Other(anyhow::anyhow!("bcrypt: {e}")))?;

    sqlx::query("UPDATE users SET password_hash = ? WHERE id = ?")
        .bind(&new_hash)
        .bind(user.id)
        .execute(&state.pool)
        .await?;

    // Rotate the session id so any cookie that observed the pre-change
    // session is now useless. Keep the user_id mapped so they don't
    // get bounced to /login. Same pattern as login_session itself.
    login_session(&session, user.id)
        .await
        .map_err(crate::error::AppError::Other)?;

    set_flash(&session, "Password updated. You're still logged in on this browser.").await;
    Ok(Redirect::to("/me/edit").into_response())
}
