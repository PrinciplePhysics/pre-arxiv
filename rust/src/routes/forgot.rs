//! /forgot-password and /reset-password/{token} — anonymous-user password
//! recovery flow.
//!
//! Threat-model notes:
//!
//!   * **Email enumeration.** The /forgot-password POST handler ALWAYS
//!     renders the same "if an account exists, we've sent a link"
//!     response, whether or not the supplied email or username matches
//!     a user. The bcrypt-style timing trick we apply on /login isn't
//!     needed here because no password is involved, but the wording
//!     itself must not betray the lookup outcome.
//!
//!   * **Token replay**. Tokens are single-use (consume_and_set deletes
//!     the row), 1-hour TTL, and minting invalidates any prior token
//!     for the same user, so the only redeemable link is the freshest.
//!
//!   * **Post-reset session**. We rotate the session id on success
//!     (login_session calls cycle_id internally) and seed the new
//!     session with the user_id, so the user is logged in immediately
//!     and any pre-reset cookie is dead.

use axum::extract::{Form, Path, State};
use axum::response::{Html, IntoResponse, Redirect, Response};
use serde::Deserialize;
use tower_sessions::Session;

use crate::auth::{hash_password, is_password_pwned, login_session, verify_csrf, MaybeUser};
use crate::error::AppResult;
use crate::helpers::{build_ctx, set_flash};
use crate::passwords;
use crate::state::AppState;
use crate::templates;

// ─── GET /forgot-password ──────────────────────────────────────────────────

pub async fn show_forgot(session: Session, maybe_user: MaybeUser) -> AppResult<Html<String>> {
    // A logged-in user shouldn't be using forgot-password — bounce them.
    if maybe_user.0.is_some() {
        return Ok(Html(redirect_html("/me/password")));
    }
    let mut ctx = build_ctx(&session, maybe_user, "/forgot-password").await;
    ctx.no_index = true;
    Ok(Html(
        templates::forgot::render_forgot(&ctx, None).into_string(),
    ))
}

#[derive(Deserialize)]
pub struct ForgotForm {
    pub csrf_token: String,
    pub identifier: String,
}

pub async fn submit_forgot(
    State(state): State<AppState>,
    session: Session,
    maybe_user: MaybeUser,
    Form(form): Form<ForgotForm>,
) -> AppResult<Response> {
    if !verify_csrf(&session, &form.csrf_token).await {
        let mut ctx = build_ctx(&session, maybe_user, "/forgot-password").await;
        ctx.no_index = true;
        let body = templates::forgot::render_forgot(&ctx, Some("Form expired — please try again."))
            .into_string();
        return Ok(Html(body).into_response());
    }

    let needle = form.identifier.trim();
    if needle.is_empty() {
        let mut ctx = build_ctx(&session, maybe_user, "/forgot-password").await;
        ctx.no_index = true;
        let body = templates::forgot::render_forgot(&ctx, Some("Enter your email or username."))
            .into_string();
        return Ok(Html(body).into_response());
    }

    // Look up the user; ALWAYS render the same confirmation page
    // regardless of hit/miss. The match arm only differs in whether we
    // bother minting a token + firing the email.
    match passwords::find_user_by_email_or_username(&state.pool, needle).await {
        Ok(Some((user_id, username, email))) => {
            // Best-effort: send the email; log the link either way.
            let _ = passwords::mint_and_send(
                &state.pool,
                user_id,
                &email,
                &username,
                state.app_url.as_deref(),
            )
            .await;
        }
        Ok(None) => {
            tracing::info!(
                target: "prexiv::passwords",
                %needle,
                "password reset requested for unknown identifier (no-op response)"
            );
        }
        Err(e) => {
            // DB error — log and continue; we still render the generic
            // confirmation so the response shape doesn't leak anything.
            tracing::error!(target: "prexiv::passwords", error = %e, "user lookup failed");
        }
    }

    set_flash(
        &session,
        "If an account exists for that email or username, we've sent a password-reset link. The link expires in 1 hour.",
    ).await;
    Ok(Redirect::to("/forgot-password/sent").into_response())
}

pub async fn show_sent(session: Session, maybe_user: MaybeUser) -> AppResult<Html<String>> {
    let mut ctx = build_ctx(&session, maybe_user, "/forgot-password/sent").await;
    ctx.no_index = true;
    Ok(Html(templates::forgot::render_sent(&ctx).into_string()))
}

// ─── GET /reset-password/{token} ──────────────────────────────────────────

pub async fn show_reset(
    State(state): State<AppState>,
    session: Session,
    maybe_user: MaybeUser,
    Path(token): Path<String>,
) -> AppResult<Html<String>> {
    let mut ctx = build_ctx(&session, maybe_user, "/reset-password").await;
    ctx.no_index = true;

    let resolved = passwords::resolve_token(&state.pool, &token).await?;
    let token_valid = resolved.is_some();
    Ok(Html(
        templates::forgot::render_reset(&ctx, &token, token_valid, None).into_string(),
    ))
}

#[derive(Deserialize)]
pub struct ResetForm {
    pub csrf_token: String,
    pub new_password: String,
    #[serde(default)]
    pub new_password_confirm: String,
}

pub async fn submit_reset(
    State(state): State<AppState>,
    session: Session,
    maybe_user: MaybeUser,
    Path(token): Path<String>,
    Form(form): Form<ResetForm>,
) -> AppResult<Response> {
    let render_err = async |msg: &str, token_valid: bool, maybe_user: MaybeUser| -> Response {
        let mut ctx = build_ctx(&session, maybe_user, "/reset-password").await;
        ctx.no_index = true;
        Html(templates::forgot::render_reset(&ctx, &token, token_valid, Some(msg)).into_string())
            .into_response()
    };

    if !verify_csrf(&session, &form.csrf_token).await {
        return Ok(render_err(
            "Form expired — request a fresh reset link.",
            false,
            maybe_user,
        )
        .await);
    }

    // Re-resolve the token at POST time — it might have expired or been
    // consumed between GET and POST.
    let Some((token_id, user_id)) = passwords::resolve_token(&state.pool, &token).await? else {
        return Ok(render_err(
            "This reset link is invalid or has expired. Request a new one from /forgot-password.",
            false,
            maybe_user,
        )
        .await);
    };

    if form.new_password.len() < 8 {
        return Ok(render_err(
            "New password must be at least 8 characters.",
            true,
            maybe_user,
        )
        .await);
    }
    if form.new_password != form.new_password_confirm {
        return Ok(render_err(
            "The two new-password fields don't match. Re-type the confirmation.",
            true,
            maybe_user,
        )
        .await);
    }
    if is_password_pwned(&form.new_password).await {
        return Ok(render_err(
            "That password appears in a known data breach. Please pick another.",
            true,
            maybe_user,
        )
        .await);
    }

    let new_hash = hash_password(&form.new_password)
        .map_err(|e| crate::error::AppError::Other(anyhow::anyhow!("bcrypt: {e}")))?;
    passwords::consume_and_set(&state.pool, token_id, user_id, &new_hash)
        .await
        .map_err(|e| crate::error::AppError::Other(anyhow::anyhow!("{e}")))?;

    // Log the user in immediately. login_session calls cycle_id, so
    // any pre-existing session for any user on this browser is dropped.
    login_session(&session, user_id)
        .await
        .map_err(crate::error::AppError::Other)?;

    set_flash(
        &session,
        "Password updated and you're signed in. Welcome back.",
    )
    .await;
    Ok(Redirect::to("/").into_response())
}

fn redirect_html(to: &str) -> String {
    format!(r#"<!doctype html><meta http-equiv="refresh" content="0;url={to}">"#)
}
