//! /me/edit — real profile editor. Replaces the previous stub.

use axum::extract::{Form, State};
use axum::response::{Html, IntoResponse, Redirect, Response};
use serde::Deserialize;
use tower_sessions::Session;

use crate::auth::{verify_csrf, MaybeUser, RequireUser};
use crate::error::AppResult;
use crate::helpers::{build_ctx, set_flash};
use crate::state::AppState;
use crate::templates;

pub struct EditValues {
    pub display_name: String,
    pub affiliation: String,
    pub bio: String,
    pub orcid: String,
}

pub async fn show(
    session: Session,
    maybe_user: MaybeUser,
    RequireUser(u): RequireUser,
) -> AppResult<Html<String>> {
    let values = EditValues {
        display_name: u.display_name.clone().unwrap_or_default(),
        affiliation:  u.affiliation.clone().unwrap_or_default(),
        bio:          u.bio.clone().unwrap_or_default(),
        orcid:        u.orcid.clone().unwrap_or_default(),
    };
    let mut ctx = build_ctx(&session, maybe_user, "/me/edit").await;
    ctx.no_index = true;
    Ok(Html(templates::me_edit::render(&ctx, &values, &[]).into_string()))
}

#[derive(Deserialize)]
pub struct EditForm {
    pub csrf_token: String,
    #[serde(default)] pub display_name: String,
    #[serde(default)] pub affiliation: String,
    #[serde(default)] pub bio: String,
    #[serde(default)] pub orcid: String,
}

pub async fn submit(
    State(state): State<AppState>,
    session: Session,
    maybe_user: MaybeUser,
    RequireUser(u): RequireUser,
    Form(form): Form<EditForm>,
) -> AppResult<Response> {
    if !verify_csrf(&session, &form.csrf_token).await {
        set_flash(&session, "Form expired — please try again.").await;
        return Ok(Redirect::to("/me/edit").into_response());
    }

    let display_name = form.display_name.trim();
    let affiliation = form.affiliation.trim();
    let bio = form.bio.trim();
    let orcid = form.orcid.trim();

    let mut errors: Vec<String> = vec![];
    if display_name.len() > 200 { errors.push("Display name must be ≤200 chars".into()); }
    if affiliation.len() > 200 { errors.push("Affiliation must be ≤200 chars".into()); }
    if bio.len() > 2000 { errors.push("Bio must be ≤2000 chars".into()); }
    if !orcid.is_empty() && !valid_orcid(orcid) {
        errors.push("ORCID must look like 0000-0000-0000-0000 (last char may be X)".into());
    }
    if !errors.is_empty() {
        let values = EditValues {
            display_name: form.display_name.clone(),
            affiliation:  form.affiliation.clone(),
            bio:          form.bio.clone(),
            orcid:        form.orcid.clone(),
        };
        let mut ctx = build_ctx(&session, maybe_user, "/me/edit").await;
        ctx.no_index = true;
        return Ok(Html(templates::me_edit::render(&ctx, &values, &errors).into_string()).into_response());
    }

    sqlx::query(
        "UPDATE users SET display_name = ?, affiliation = ?, bio = ?, orcid = ? WHERE id = ?",
    )
    .bind(opt(display_name))
    .bind(opt(affiliation))
    .bind(opt(bio))
    .bind(opt(orcid))
    .bind(u.id)
    .execute(&state.pool)
    .await?;
    set_flash(&session, "Profile updated.").await;
    Ok(Redirect::to(&format!("/u/{}", u.username)).into_response())
}

fn opt(s: &str) -> Option<&str> { if s.is_empty() { None } else { Some(s) } }

fn valid_orcid(s: &str) -> bool {
    let s = s.as_bytes();
    if s.len() != 19 { return false; }
    for (i, &b) in s.iter().enumerate() {
        match i {
            4 | 9 | 14 => { if b != b'-' { return false; } }
            18 => { if !(b.is_ascii_digit() || b == b'X') { return false; } }
            _ => { if !b.is_ascii_digit() { return false; } }
        }
    }
    true
}
