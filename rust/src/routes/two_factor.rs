//! TOTP-based 2FA. Pages:
//!
//!   GET  /me/2fa                 — status + enroll/disable controls
//!   POST /me/2fa/enable          — generate secret, present QR + confirm form
//!   POST /me/2fa/confirm         — verify the first code, flip enabled_at = NOW()
//!   POST /me/2fa/disable         — drop the row (requires current password)
//!
//!   GET  /login/2fa              — second-step form during login
//!   POST /login/2fa              — verify the code, complete login_session
//!
//! Login flow change: routes/auth.rs::do_login, after password verifies,
//! checks totp::is_enabled. If yes, the session stashes `pending_2fa_user_id`
//! and we redirect to /login/2fa. The /login/2fa POST consumes that key,
//! verifies the TOTP code, and calls login_session.

use axum::extract::{Form, Query, State};
use axum::response::{Html, IntoResponse, Redirect, Response};
use serde::Deserialize;
use tower_sessions::Session;

use crate::auth::{
    login_session, verify_csrf, verify_password_timing_safe, MaybeUser, RequireUser,
};
use crate::error::AppResult;
use crate::helpers::{build_ctx, set_flash};
use crate::state::AppState;
use crate::templates;
use crate::totp;

// ── /me/2fa GET ──────────────────────────────────────────────────────

pub async fn show(
    State(state): State<AppState>,
    session: Session,
    maybe_user: MaybeUser,
    RequireUser(user): RequireUser,
) -> AppResult<Html<String>> {
    let row = totp::get_for(&state.pool, user.id).await.ok().flatten();
    let mut ctx = build_ctx(&session, maybe_user, "/me/2fa").await;
    ctx.no_index = true;
    // Mid-enrollment? Show the QR + confirm-form again so the user can
    // finish without re-rolling the secret.
    let enrollment = match &row {
        Some(t) if t.enabled_at.is_none() => {
            Some((t.secret.clone(), totp::qr_svg(&t.secret, &user.email)))
        }
        _ => None,
    };
    let enabled = row.as_ref().and_then(|t| t.enabled_at).is_some();
    Ok(Html(
        templates::two_factor::render_status(&ctx, &user.email, enabled, enrollment.as_ref(), None)
            .into_string(),
    ))
}

// ── /me/2fa/enable POST (start enrollment) ──────────────────────────

#[derive(Deserialize)]
pub struct CsrfOnly {
    pub csrf_token: String,
}

pub async fn start_enroll(
    State(state): State<AppState>,
    session: Session,
    maybe_user: MaybeUser,
    RequireUser(user): RequireUser,
    Form(form): Form<CsrfOnly>,
) -> AppResult<Response> {
    if !verify_csrf(&session, &form.csrf_token).await {
        set_flash(&session, "Form expired — please try again.").await;
        return Ok(Redirect::to("/me/2fa").into_response());
    }
    let _ = totp::start_enrollment(&state.pool, user.id).await;
    set_flash(
        &session,
        "Scan the QR with your authenticator app, then submit the first 6-digit code below.",
    )
    .await;
    let _ = maybe_user; // silence unused-warning; render uses session-derived ctx via show GET
    Ok(Redirect::to("/me/2fa").into_response())
}

// ── /me/2fa/confirm POST (finish enrollment by verifying first code) ──

#[derive(Deserialize)]
pub struct ConfirmForm {
    pub csrf_token: String,
    pub code: String,
}

pub async fn confirm(
    State(state): State<AppState>,
    session: Session,
    maybe_user: MaybeUser,
    RequireUser(user): RequireUser,
    Form(form): Form<ConfirmForm>,
) -> AppResult<Response> {
    let mut ctx = build_ctx(&session, maybe_user, "/me/2fa").await;
    ctx.no_index = true;
    if !verify_csrf(&session, &form.csrf_token).await {
        set_flash(&session, "Form expired — please try again.").await;
        return Ok(Redirect::to("/me/2fa").into_response());
    }
    let row = match totp::get_for(&state.pool, user.id).await.ok().flatten() {
        Some(r) if r.enabled_at.is_none() => r,
        _ => {
            set_flash(
                &session,
                "No pending 2FA enrollment. Click Enable to start.",
            )
            .await;
            return Ok(Redirect::to("/me/2fa").into_response());
        }
    };
    if !totp::verify(&row.secret, &form.code) {
        let enrollment = Some((row.secret.clone(), totp::qr_svg(&row.secret, &user.email)));
        let body = templates::two_factor::render_status(
            &ctx,
            &user.email,
            false,
            enrollment.as_ref(),
            Some("That code didn't match. Make sure your phone's clock is correct, and try the current code (codes rotate every 30 seconds)."),
        )
        .into_string();
        return Ok(Html(body).into_response());
    }
    let _ = totp::confirm_enrollment(&state.pool, user.id).await;
    set_flash(
        &session,
        "Two-factor authentication enabled. Next time you sign in we'll ask for a code.",
    )
    .await;
    Ok(Redirect::to("/me/2fa").into_response())
}

// ── /me/2fa/disable POST (requires current password) ────────────────

#[derive(Deserialize)]
pub struct DisableForm {
    pub csrf_token: String,
    pub current_password: String,
}

pub async fn disable(
    State(state): State<AppState>,
    session: Session,
    maybe_user: MaybeUser,
    RequireUser(user): RequireUser,
    Form(form): Form<DisableForm>,
) -> AppResult<Response> {
    let mut ctx = build_ctx(&session, maybe_user, "/me/2fa").await;
    ctx.no_index = true;
    if !verify_csrf(&session, &form.csrf_token).await {
        set_flash(&session, "Form expired — please try again.").await;
        return Ok(Redirect::to("/me/2fa").into_response());
    }
    let hash: Option<(String,)> = sqlx::query_as(crate::db::pg(
        "SELECT password_hash FROM users WHERE id = ?",
    ))
    .bind(user.id)
    .fetch_optional(&state.pool)
    .await?;
    if !verify_password_timing_safe(&form.current_password, hash.as_ref().map(|(h,)| h.as_str())) {
        let body = templates::two_factor::render_status(
            &ctx,
            &user.email,
            true,
            None,
            Some("Current password is incorrect. 2FA is still enabled."),
        )
        .into_string();
        return Ok(Html(body).into_response());
    }
    let _ = totp::disable(&state.pool, user.id).await;
    set_flash(&session, "Two-factor authentication disabled.").await;
    Ok(Redirect::to("/me/2fa").into_response())
}

// ── /login/2fa — second-step form ───────────────────────────────────

#[derive(Deserialize)]
pub struct NextQuery {
    pub next: Option<String>,
}

pub async fn show_login_2fa(
    session: Session,
    maybe_user: MaybeUser,
    Query(q): Query<NextQuery>,
) -> AppResult<Html<String>> {
    let pending: Option<i64> = session
        .get::<i64>("pending_2fa_user_id")
        .await
        .ok()
        .flatten();
    if pending.is_none() {
        // No mid-login state — bounce back to /login.
        return Ok(Html(redirect_html("/login")));
    }
    let mut ctx = build_ctx(&session, maybe_user, "/login/2fa").await;
    ctx.no_index = true;
    Ok(Html(
        templates::two_factor::render_login_step(&ctx, q.next.as_deref(), None).into_string(),
    ))
}

#[derive(Deserialize)]
pub struct Login2faForm {
    pub csrf_token: String,
    pub code: String,
    #[serde(default)]
    pub next: Option<String>,
}

pub async fn submit_login_2fa(
    State(state): State<AppState>,
    session: Session,
    maybe_user: MaybeUser,
    Form(form): Form<Login2faForm>,
) -> AppResult<Response> {
    let mut ctx = build_ctx(&session, maybe_user, "/login/2fa").await;
    ctx.no_index = true;

    if !verify_csrf(&session, &form.csrf_token).await {
        return Ok(Html(
            templates::two_factor::render_login_step(
                &ctx,
                form.next.as_deref(),
                Some("Form expired — start the sign-in again."),
            )
            .into_string(),
        )
        .into_response());
    }

    let pending: Option<i64> = session
        .get::<i64>("pending_2fa_user_id")
        .await
        .ok()
        .flatten();
    let user_id = match pending {
        Some(uid) => uid,
        None => return Ok(Redirect::to("/login").into_response()),
    };

    let totp_row = totp::get_for(&state.pool, user_id).await.ok().flatten();
    let secret = match totp_row.as_ref() {
        Some(t) if t.enabled_at.is_some() => &t.secret,
        _ => {
            // 2FA somehow not enabled; bail to /login.
            let _ = session.remove::<i64>("pending_2fa_user_id").await;
            return Ok(Redirect::to("/login").into_response());
        }
    };
    if !totp::verify(secret, &form.code) {
        return Ok(Html(
            templates::two_factor::render_login_step(
                &ctx,
                form.next.as_deref(),
                Some("Incorrect code. Try the current 6-digit code from your authenticator app."),
            )
            .into_string(),
        )
        .into_response());
    }

    // Code is good — complete the login.
    let _ = session.remove::<i64>("pending_2fa_user_id").await;
    login_session(&session, user_id)
        .await
        .map_err(crate::error::AppError::Other)?;
    let dest = sanitize_next(form.next.as_deref());
    Ok(Redirect::to(&dest).into_response())
}

fn redirect_html(to: &str) -> String {
    format!(r#"<!doctype html><meta http-equiv="refresh" content="0;url={to}">"#)
}

fn sanitize_next(next: Option<&str>) -> String {
    match next {
        Some(s)
            if s.starts_with('/')
                && !s.starts_with("//")
                && !s.starts_with("/\\")
                && !s.contains('\n')
                && !s.contains('\r')
                && s.len() <= 512 =>
        {
            s.to_string()
        }
        _ => "/".to_string(),
    }
}
