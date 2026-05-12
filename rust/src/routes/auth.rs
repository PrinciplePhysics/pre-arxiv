use axum::extract::{Form, Query, State};
use axum::response::{Html, IntoResponse, Redirect, Response};
use serde::Deserialize;
use tower_sessions::Session;

use crate::auth::{
    hash_password, is_password_pwned, load_user, login_session, logout_session, verify_csrf,
    verify_password_timing_safe, MaybeUser,
};
use crate::error::AppResult;
use crate::helpers::{build_ctx, set_flash};
use crate::state::AppState;
use crate::templates::{self, auth::RegisterForm};
use crate::verify;

#[derive(Deserialize)]
pub struct NextParam {
    #[serde(default)]
    pub next: Option<String>,
}

pub async fn show_login(
    State(_state): State<AppState>,
    session: Session,
    maybe_user: MaybeUser,
    Query(q): Query<NextParam>,
) -> AppResult<Html<String>> {
    if maybe_user.0.is_some() {
        return Ok(Html(redirect_html("/")));
    }
    let mut ctx = build_ctx(&session, maybe_user, "/login").await;
    ctx.no_index = true;
    let markup = templates::auth::render_login(&ctx, None, q.next.as_deref());
    Ok(Html(markup.into_string()))
}

#[derive(Deserialize)]
pub struct LoginForm {
    pub csrf_token: String,
    pub identifier: String,
    pub password: String,
    #[serde(default)]
    pub next: Option<String>,
}

pub async fn do_login(
    State(state): State<AppState>,
    session: Session,
    maybe_user: MaybeUser,
    Form(form): Form<LoginForm>,
) -> AppResult<Response> {
    if !verify_csrf(&session, &form.csrf_token).await {
        return Ok(error_response(&session, maybe_user, "Form expired — please try again.", form.next.as_deref()).await);
    }
    let row: Option<(i64, String)> = sqlx::query_as::<_, (i64, String)>(
        "SELECT id, password_hash FROM users WHERE username = ? OR email = ? LIMIT 1",
    )
    .bind(&form.identifier)
    .bind(&form.identifier)
    .fetch_optional(&state.pool)
    .await?;

    // Run bcrypt unconditionally (against a dummy hash if the user didn't
    // exist) so wrong-username and wrong-password cost the same wall-clock
    // time. Returning a single generic message also avoids leaking which
    // of the two branches failed. Defends against user enumeration.
    let real_hash = row.as_ref().map(|(_, h)| h.as_str());
    let password_ok = verify_password_timing_safe(&form.password, real_hash);

    if !password_ok {
        return Ok(error_response(
            &session, maybe_user,
            "Incorrect username/email or password.",
            form.next.as_deref(),
        ).await);
    }
    let user_id = row.expect("password_ok implies row is Some").0;

    // 2FA gate: if the user has TOTP enabled, stash the candidate id in
    // the session and redirect to /login/2fa. login_session is NOT
    // called yet — we're not logged in until the second factor verifies.
    if crate::totp::is_enabled(&state.pool, user_id).await {
        let _ = session.insert("pending_2fa_user_id", user_id).await;
        let target = match form.next.as_deref() {
            Some(n) if !n.is_empty() => format!("/login/2fa?next={}", urlencoding::encode(n)),
            _ => "/login/2fa".to_string(),
        };
        return Ok(Redirect::to(&target).into_response());
    }

    login_session(&session, user_id).await.map_err(crate::error::AppError::Other)?;
    let dest = sanitize_next(form.next.as_deref());
    Ok(Redirect::to(&dest).into_response())
}

async fn error_response(
    session: &Session,
    maybe_user: MaybeUser,
    msg: &str,
    next: Option<&str>,
) -> Response {
    let mut ctx = build_ctx(session, maybe_user, "/login").await;
    ctx.no_index = true;
    let markup = templates::auth::render_login(&ctx, Some(msg), next);
    Html(markup.into_string()).into_response()
}

pub async fn show_register(
    session: Session,
    maybe_user: MaybeUser,
) -> AppResult<Html<String>> {
    if maybe_user.0.is_some() {
        return Ok(Html(redirect_html("/")));
    }
    let mut ctx = build_ctx(&session, maybe_user, "/register").await;
    ctx.no_index = true;
    let markup = templates::auth::render_register(&ctx, None, &RegisterForm::default());
    Ok(Html(markup.into_string()))
}

#[derive(Deserialize)]
pub struct RegisterPost {
    pub csrf_token: String,
    pub username: String,
    pub email: String,
    pub password: String,
    #[serde(default)]
    pub password_confirm: String,
    #[serde(default)]
    pub display_name: String,
}

pub async fn do_register(
    State(state): State<AppState>,
    session: Session,
    maybe_user: MaybeUser,
    Form(form): Form<RegisterPost>,
) -> AppResult<Response> {
    let mk_err = async |msg: &str, form: &RegisterPost, maybe_user: MaybeUser| -> Response {
        let mut ctx = build_ctx(&session, maybe_user, "/register").await;
        ctx.no_index = true;
        let form_state = RegisterForm {
            username: form.username.clone(),
            email: form.email.clone(),
            display_name: form.display_name.clone(),
        };
        let markup = templates::auth::render_register(&ctx, Some(msg), &form_state);
        Html(markup.into_string()).into_response()
    };

    if !verify_csrf(&session, &form.csrf_token).await {
        return Ok(mk_err("Form expired — please try again.", &form, maybe_user).await);
    }
    let username = form.username.trim();
    let email = form.email.trim().to_ascii_lowercase();
    if username.len() < 3 || username.len() > 32
        || !username.chars().all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
    {
        return Ok(mk_err("Username must be 3–32 chars, letters/digits/underscore/hyphen only.", &form, maybe_user).await);
    }
    if !email.contains('@') || email.len() < 5 || email.len() > 254 {
        return Ok(mk_err("Email looks invalid.", &form, maybe_user).await);
    }
    if form.password.len() < 8 {
        return Ok(mk_err("Password must be at least 8 characters.", &form, maybe_user).await);
    }
    if form.password != form.password_confirm {
        return Ok(mk_err("The two passwords don't match. Re-type the confirmation.", &form, maybe_user).await);
    }
    if is_password_pwned(&form.password).await {
        return Ok(mk_err("That password appears in a known data breach. Please pick another.", &form, maybe_user).await);
    }
    let existing: Option<(i64,)> = sqlx::query_as::<_, (i64,)>(
        "SELECT id FROM users WHERE username = ? OR email = ? LIMIT 1",
    )
    .bind(username)
    .bind(&email)
    .fetch_optional(&state.pool)
    .await?;
    if existing.is_some() {
        return Ok(mk_err("That username or email is already taken.", &form, maybe_user).await);
    }
    let hash = hash_password(&form.password)
        .map_err(|e| crate::error::AppError::Other(anyhow::anyhow!("bcrypt: {e}")))?;
    let display_name = if form.display_name.trim().is_empty() {
        None
    } else {
        Some(form.display_name.trim().to_string())
    };

    let result = sqlx::query(
        r#"INSERT INTO users (username, email, password_hash, display_name, email_verified)
           VALUES (?, ?, ?, ?, 0)"#,
    )
    .bind(username)
    .bind(&email)
    .bind(&hash)
    .bind(&display_name)
    .execute(&state.pool)
    .await?;

    let user_id = result.last_insert_rowid();

    // Mint a verification token and fire the email send in the
    // background. `mint_and_send` returns the plaintext token, which we
    // stash in the session so /me/edit can render an inline
    // "Verify my email →" button — this is the fallback path that
    // keeps PreXiv usable while the upstream mail provider's
    // anti-abuse activation is pending. Once outbound mail starts
    // working, the same token also arrives by email; whichever the
    // user clicks works.
    let pending_token = verify::mint_and_send(
        &state.pool, user_id, &email, username, state.app_url.as_deref(),
    )
    .await
    .ok();

    login_session(&session, user_id)
        .await
        .map_err(crate::error::AppError::Other)?;

    if let Some(t) = pending_token {
        let _ = session.insert("pending_verify_token", t).await;
    }

    set_flash(
        &session,
        "Welcome! Click the green button below to verify your email — submission is gated on verification. (We've also queued a verification email; delivery may take a moment.)"
    ).await;
    // Redirect to /me/edit so the verify banner is the first thing the
    // user sees post-register. They can browse and comment from here
    // (the topnav still works), but submit and tokens are gated.
    Ok(Redirect::to("/me/edit").into_response())
}

#[derive(Deserialize)]
pub struct LogoutForm {
    pub csrf_token: String,
}

pub async fn do_logout(
    session: Session,
    Form(form): Form<LogoutForm>,
) -> AppResult<Response> {
    if verify_csrf(&session, &form.csrf_token).await {
        let _ = logout_session(&session).await;
    }
    Ok(Redirect::to("/").into_response())
}

fn redirect_html(to: &str) -> String {
    format!(r#"<!doctype html><meta http-equiv="refresh" content="0;url={to}">"#)
}

/// Open-redirect defence for `?next=…`. Only same-origin paths beginning
/// with a single `/` (not `//`, which browsers interpret as a
/// protocol-relative cross-origin URL) and not `/\` (Windows-style
/// alternate). Falls back to the home page on anything suspicious.
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

#[allow(dead_code)]
async fn _ensure_unused(state: &AppState) {
    let _: Option<crate::models::User> = load_user(&state.pool, 0).await.ok().flatten();
}
