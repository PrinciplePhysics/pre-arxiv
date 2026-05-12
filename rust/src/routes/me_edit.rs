//! /me/edit — real profile editor. Replaces the previous stub.

use axum::extract::{Form, State};
use axum::response::{Html, IntoResponse, Redirect, Response};
use serde::Deserialize;
use tower_sessions::Session;

use crate::auth::{verify_csrf, MaybeUser, RequireUser};
use crate::email_change;
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
    State(state): State<AppState>,
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
    let pending_email = email_change::pending_for_user(&state.pool, u.id)
        .await
        .ok()
        .flatten()
        .map(|(addr, _)| addr);
    let mut ctx = build_ctx(&session, maybe_user, "/me/edit").await;
    ctx.no_index = true;
    Ok(Html(
        templates::me_edit::render(&ctx, &values, &[], pending_email.as_deref()).into_string(),
    ))
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
        let pending_email = email_change::pending_for_user(&state.pool, u.id)
            .await
            .ok()
            .flatten()
            .map(|(addr, _)| addr);
        let mut ctx = build_ctx(&session, maybe_user, "/me/edit").await;
        ctx.no_index = true;
        return Ok(Html(
            templates::me_edit::render(&ctx, &values, &errors, pending_email.as_deref()).into_string(),
        ).into_response());
    }

    // If the ORCID iD changed (or got blanked), drop any prior
    // verification — the user must re-verify against the new value.
    let prior_orcid = u.orcid.as_deref().unwrap_or("");
    let normalised = if orcid.is_empty() { String::new() }
                     else { crate::orcid::normalize(orcid).unwrap_or_else(|| orcid.to_string()) };
    let orcid_changed = normalised != prior_orcid;
    let reset_verified = orcid_changed && u.orcid_verified != 0;

    if reset_verified {
        sqlx::query(
            "UPDATE users
                SET display_name = ?, affiliation = ?, bio = ?, orcid = ?,
                    orcid_verified = 0
              WHERE id = ?",
        )
        .bind(opt(display_name))
        .bind(opt(affiliation))
        .bind(opt(bio))
        .bind(opt(if normalised.is_empty() { orcid } else { &normalised }))
        .bind(u.id)
        .execute(&state.pool)
        .await?;
    } else {
        sqlx::query(
            "UPDATE users SET display_name = ?, affiliation = ?, bio = ?, orcid = ?
              WHERE id = ?",
        )
        .bind(opt(display_name))
        .bind(opt(affiliation))
        .bind(opt(bio))
        .bind(opt(if normalised.is_empty() { orcid } else { &normalised }))
        .bind(u.id)
        .execute(&state.pool)
        .await?;
    }
    set_flash(&session, "Profile updated.").await;
    Ok(Redirect::to(&format!("/u/{}", u.username)).into_response())
}

// ─── POST /me/verify-orcid ────────────────────────────────────────────
//
// Fetches the public ORCID record for the user's stored iD and flips
// `orcid_verified = 1` if the name on file matches their PreXiv
// display name. Surfaces a flash with the ORCID-side name on
// mismatch so the user knows what to align.
#[derive(Deserialize)]
pub struct VerifyOrcidForm {
    pub csrf_token: String,
}

pub async fn verify_orcid(
    State(state): State<AppState>,
    session: Session,
    _maybe_user: MaybeUser,
    RequireUser(u): RequireUser,
    Form(form): Form<VerifyOrcidForm>,
) -> AppResult<Response> {
    if !verify_csrf(&session, &form.csrf_token).await {
        set_flash(&session, "Form expired — please try again.").await;
        return Ok(Redirect::to("/me/edit").into_response());
    }
    let orcid_raw = u.orcid.as_deref().unwrap_or("");
    let Some(orcid) = crate::orcid::normalize(orcid_raw) else {
        set_flash(
            &session,
            "Set a valid ORCID iD (form 0000-0000-0000-000X) first, then click Verify.",
        )
        .await;
        return Ok(Redirect::to("/me/edit").into_response());
    };
    let record = match crate::orcid::fetch_record(&orcid).await {
        Ok(r) => r,
        Err(e) => {
            tracing::warn!(orcid=%orcid, error=%e, "ORCID fetch failed");
            set_flash(
                &session,
                "Couldn't reach ORCID just now, or that iD doesn't exist. Try again in a minute.",
            )
            .await;
            return Ok(Redirect::to("/me/edit").into_response());
        }
    };
    let name = record
        .person
        .as_ref()
        .and_then(|p| p.name.as_ref())
        .map(|n| n.assembled())
        .unwrap_or_default();
    let display = u.display_name.as_deref().unwrap_or(&u.username);
    if !crate::orcid::name_matches(&name, display) {
        let msg = if name.is_empty() {
            "That ORCID record has no public name on file. Either make your ORCID name public, \
             or use the institutional-email path instead.".to_string()
        } else {
            format!(
                "ORCID record shows \"{name}\" but your PreXiv display name is \"{display}\". \
                 Update your display name to match (under \"Display name\") and try again."
            )
        };
        set_flash(&session, &msg).await;
        return Ok(Redirect::to("/me/edit").into_response());
    }
    sqlx::query("UPDATE users SET orcid_verified = 1 WHERE id = ?")
        .bind(u.id)
        .execute(&state.pool)
        .await?;
    set_flash(
        &session,
        &format!("ORCID iD {orcid} verified — your manuscripts will now carry the verified-scholar badge."),
    )
    .await;
    Ok(Redirect::to("/me/edit").into_response())
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
