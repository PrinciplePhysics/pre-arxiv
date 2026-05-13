//! /me/edit — real profile editor. Replaces the previous stub.

use axum::extract::{Form, Query, State};
use axum::response::{Html, IntoResponse, Redirect, Response};
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};
use rand::rngs::OsRng;
use rand::RngCore;
use serde::Deserialize;
use tower_sessions::Session;

use crate::auth::{verify_csrf, MaybeUser, RequireUser};
use crate::email_change;
use crate::error::AppResult;
use crate::helpers::{build_ctx, set_flash, set_orcid_flash, take_orcid_flash};
use crate::state::AppState;
use crate::templates;

const ORCID_OAUTH_STATE_KEY: &str = "orcid_oauth_state";
const ORCID_OAUTH_NONCE_KEY: &str = "orcid_oauth_nonce";

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
        affiliation: u.affiliation.clone().unwrap_or_default(),
        bio: u.bio.clone().unwrap_or_default(),
        orcid: u.orcid.clone().unwrap_or_default(),
    };
    let pending_email = email_change::pending_for_user(&state.pool, u.id)
        .await
        .ok()
        .flatten()
        .map(|(addr, _)| addr);
    let orcid_flash = take_orcid_flash(&session).await;
    let orcid_oauth_unavailable = orcid_oauth_unavailable_message(&state);
    let mut ctx = build_ctx(&session, maybe_user, "/me/edit").await;
    ctx.no_index = true;
    Ok(Html(
        templates::me_edit::render(
            &ctx,
            &values,
            &[],
            pending_email.as_deref(),
            orcid_flash.as_ref().map(|(m, e)| (m.as_str(), *e)),
            orcid_oauth_unavailable.as_deref(),
        )
        .into_string(),
    ))
}

#[derive(Deserialize)]
pub struct EditForm {
    pub csrf_token: String,
    #[serde(default)]
    pub display_name: String,
    #[serde(default)]
    pub affiliation: String,
    #[serde(default)]
    pub bio: String,
    #[serde(default)]
    pub orcid: String,
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
    if display_name.len() > 200 {
        errors.push("Display name must be ≤200 chars".into());
    }
    if affiliation.len() > 200 {
        errors.push("Affiliation must be ≤200 chars".into());
    }
    if bio.len() > 2000 {
        errors.push("Bio must be ≤2000 chars".into());
    }
    if !orcid.is_empty() && !valid_orcid(orcid) {
        errors.push("ORCID must look like 0000-0000-0000-0000 (last char may be X)".into());
    }
    if !errors.is_empty() {
        let values = EditValues {
            display_name: form.display_name.clone(),
            affiliation: form.affiliation.clone(),
            bio: form.bio.clone(),
            orcid: form.orcid.clone(),
        };
        let pending_email = email_change::pending_for_user(&state.pool, u.id)
            .await
            .ok()
            .flatten()
            .map(|(addr, _)| addr);
        let mut ctx = build_ctx(&session, maybe_user, "/me/edit").await;
        ctx.no_index = true;
        let orcid_oauth_unavailable = orcid_oauth_unavailable_message(&state);
        return Ok(Html(
            templates::me_edit::render(
                &ctx,
                &values,
                &errors,
                pending_email.as_deref(),
                None,
                orcid_oauth_unavailable.as_deref(),
            )
            .into_string(),
        )
        .into_response());
    }

    // If the ORCID iD changed (or got blanked), drop any prior
    // public-name match and OAuth binding — the user must re-verify
    // against the new value.
    let prior_orcid = u.orcid.as_deref().unwrap_or("");
    let normalised = if orcid.is_empty() {
        String::new()
    } else {
        crate::orcid::normalize(orcid).unwrap_or_else(|| orcid.to_string())
    };
    let orcid_changed = normalised != prior_orcid;
    let reset_verified = orcid_changed && (u.orcid_verified != 0 || u.orcid_oauth_verified != 0);

    if reset_verified {
        sqlx::query(
            "UPDATE users
                SET display_name = ?, affiliation = ?, bio = ?, orcid = ?,
                    orcid_verified = 0,
                    orcid_oauth_verified = 0,
                    orcid_oauth_verified_at = NULL,
                    orcid_oauth_sub = NULL
              WHERE id = ?",
        )
        .bind(opt(display_name))
        .bind(opt(affiliation))
        .bind(opt(bio))
        .bind(opt(if normalised.is_empty() {
            orcid
        } else {
            &normalised
        }))
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
        .bind(opt(if normalised.is_empty() {
            orcid
        } else {
            &normalised
        }))
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
// display name. This is a profile trust signal, not account-ownership
// proof. Surfaces a flash with the ORCID-side name on mismatch so the
// user knows what to align.
/// Body of POST /me/verify-orcid. Accepts the full /me/edit field set
/// so the user can paste an ORCID iD and click Verify in a single
/// gesture — we save the form first, then verify. The display_name is
/// the field used in the name match, so it has to be current.
#[derive(Deserialize)]
pub struct VerifyOrcidForm {
    pub csrf_token: String,
    #[serde(default)]
    pub display_name: String,
    #[serde(default)]
    pub affiliation: String,
    #[serde(default)]
    pub bio: String,
    #[serde(default)]
    pub orcid: String,
}

pub async fn verify_orcid(
    State(state): State<AppState>,
    session: Session,
    _maybe_user: MaybeUser,
    RequireUser(u): RequireUser,
    Form(form): Form<VerifyOrcidForm>,
) -> AppResult<Response> {
    if !verify_csrf(&session, &form.csrf_token).await {
        set_orcid_flash(&session, "Form expired — please try again.", true).await;
        return Ok(Redirect::to("/me/edit").into_response());
    }

    // First, save any field edits — same shape as the /me/edit submit
    // handler. The user clicked "Save & name-match" so they expect their
    // typed values to be persisted even if verification then fails.
    let display_name = form.display_name.trim();
    let affiliation = form.affiliation.trim();
    let bio = form.bio.trim();
    let orcid_form = form.orcid.trim();
    if display_name.len() > 200 || affiliation.len() > 200 || bio.len() > 2000 {
        set_orcid_flash(
            &session,
            "One of the fields is too long — go back and shorten it.",
            true,
        )
        .await;
        return Ok(Redirect::to("/me/edit").into_response());
    }
    if !orcid_form.is_empty() && !crate::orcid::normalize(orcid_form).is_some() {
        set_orcid_flash(
            &session,
            "ORCID iD must look like 0000-0000-0000-000X (last char may be X).",
            true,
        )
        .await;
        return Ok(Redirect::to("/me/edit").into_response());
    }
    let normalised_orcid = if orcid_form.is_empty() {
        String::new()
    } else {
        crate::orcid::normalize(orcid_form).unwrap_or_else(|| orcid_form.to_string())
    };
    let prior_orcid = u.orcid.as_deref().unwrap_or("");
    let orcid_changed = normalised_orcid != prior_orcid;
    // Reset prior public-name match and OAuth binding if ORCID iD changed.
    if orcid_changed && (u.orcid_verified != 0 || u.orcid_oauth_verified != 0) {
        sqlx::query(
            "UPDATE users
                SET display_name = ?, affiliation = ?, bio = ?, orcid = ?,
                    orcid_verified = 0,
                    orcid_oauth_verified = 0,
                    orcid_oauth_verified_at = NULL,
                    orcid_oauth_sub = NULL
              WHERE id = ?",
        )
        .bind(opt(display_name))
        .bind(opt(affiliation))
        .bind(opt(bio))
        .bind(opt(if normalised_orcid.is_empty() {
            orcid_form
        } else {
            &normalised_orcid
        }))
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
        .bind(opt(if normalised_orcid.is_empty() {
            orcid_form
        } else {
            &normalised_orcid
        }))
        .bind(u.id)
        .execute(&state.pool)
        .await?;
    }

    let Some(orcid) = (if normalised_orcid.is_empty() {
        crate::orcid::normalize(u.orcid.as_deref().unwrap_or(""))
    } else {
        Some(normalised_orcid)
    }) else {
        set_orcid_flash(
            &session,
            "Paste a valid ORCID iD (0000-0000-0000-000X) in the field above, then click Save & name-match ORCID.",
            true,
        )
        .await;
        return Ok(Redirect::to("/me/edit").into_response());
    };
    let record = match crate::orcid::fetch_record(&orcid).await {
        Ok(r) => r,
        Err(e) => {
            tracing::warn!(orcid=%orcid, error=%e, "ORCID fetch failed");
            set_orcid_flash(
                &session,
                "Couldn't reach ORCID just now, or that iD doesn't exist. Try again in a minute.",
                true,
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
    // Use the just-saved display_name (the user may have typed a new one
    // in the same submit). Empty display_name falls back to username,
    // which almost never matches a human ORCID record — that's the
    // expected nudge to enter a real name.
    let display_owned = if display_name.is_empty() {
        u.username.clone()
    } else {
        display_name.to_string()
    };
    if !crate::orcid::name_matches(&name, &display_owned) {
        let msg = if name.is_empty() {
            "That ORCID record has no public name on file. Either make your ORCID name public, \
             or use the institutional-email path instead."
                .to_string()
        } else {
            format!(
                "ORCID record shows \"{name}\" but your PreXiv display name is \"{display_owned}\". \
                 Update your display name to match (top of the form), then click Save & name-match ORCID."
            )
        };
        set_orcid_flash(&session, &msg, true).await;
        return Ok(Redirect::to("/me/edit").into_response());
    }
    sqlx::query("UPDATE users SET orcid_verified = 1 WHERE id = ?")
        .bind(u.id)
        .execute(&state.pool)
        .await?;
    set_orcid_flash(
        &session,
        format!("ORCID iD {orcid} public name matched — your profile can show the ORCID link. This is not ownership-grade verification and does not grant curated-listing status; use Connect with ORCID for that."),
        false,
    )
    .await;
    Ok(Redirect::to("/me/edit").into_response())
}

pub async fn connect_orcid(
    State(state): State<AppState>,
    session: Session,
    RequireUser(_u): RequireUser,
) -> AppResult<Response> {
    let cfg = match crate::orcid::oauth_config(state.app_url.as_deref()) {
        Ok(Some(cfg)) => cfg,
        Ok(None) => {
            set_orcid_flash(
                &session,
                "ORCID OAuth is not configured yet. Set ORCID_CLIENT_ID, ORCID_CLIENT_SECRET, and ORCID_REDIRECT_URI on the server.",
                true,
            )
            .await;
            return Ok(Redirect::to("/me/edit").into_response());
        }
        Err(e) => {
            set_orcid_flash(
                &session,
                format!("ORCID OAuth configuration error: {e}"),
                true,
            )
            .await;
            return Ok(Redirect::to("/me/edit").into_response());
        }
    };
    let mut bytes = [0u8; 32];
    OsRng.fill_bytes(&mut bytes);
    let oauth_state = URL_SAFE_NO_PAD.encode(bytes);
    let mut nonce_bytes = [0u8; 32];
    OsRng.fill_bytes(&mut nonce_bytes);
    let oauth_nonce = URL_SAFE_NO_PAD.encode(nonce_bytes);
    session
        .insert(ORCID_OAUTH_STATE_KEY, oauth_state.clone())
        .await
        .map_err(|e| crate::error::AppError::Other(anyhow::anyhow!(e)))?;
    session
        .insert(ORCID_OAUTH_NONCE_KEY, oauth_nonce.clone())
        .await
        .map_err(|e| crate::error::AppError::Other(anyhow::anyhow!(e)))?;
    Ok(Redirect::to(&cfg.authorize_url(&oauth_state, &oauth_nonce)).into_response())
}

#[derive(Deserialize)]
pub struct OrcidCallbackQuery {
    #[serde(default)]
    pub code: Option<String>,
    #[serde(default)]
    pub state: Option<String>,
    #[serde(default)]
    pub error: Option<String>,
    #[serde(default)]
    pub error_description: Option<String>,
}

pub async fn orcid_callback(
    State(state): State<AppState>,
    session: Session,
    RequireUser(u): RequireUser,
    Query(q): Query<OrcidCallbackQuery>,
) -> AppResult<Response> {
    let expected_state: Option<String> = session
        .get(ORCID_OAUTH_STATE_KEY)
        .await
        .map_err(|e| crate::error::AppError::Other(anyhow::anyhow!(e)))?;
    let expected_nonce: Option<String> = session
        .get(ORCID_OAUTH_NONCE_KEY)
        .await
        .map_err(|e| crate::error::AppError::Other(anyhow::anyhow!(e)))?;
    let _ = session.remove::<String>(ORCID_OAUTH_STATE_KEY).await;
    let _ = session.remove::<String>(ORCID_OAUTH_NONCE_KEY).await;

    if let Some(err) = q.error.as_deref() {
        let msg = q
            .error_description
            .as_deref()
            .filter(|s| !s.trim().is_empty())
            .unwrap_or(err);
        set_orcid_flash(
            &session,
            format!("ORCID sign-in was not completed: {msg}"),
            true,
        )
        .await;
        return Ok(Redirect::to("/me/edit").into_response());
    }

    let Some(expected_state) = expected_state else {
        set_orcid_flash(
            &session,
            "ORCID sign-in state was missing. Start again from the Connect with ORCID button.",
            true,
        )
        .await;
        return Ok(Redirect::to("/me/edit").into_response());
    };
    let Some(expected_nonce) = expected_nonce else {
        set_orcid_flash(
            &session,
            "ORCID sign-in nonce was missing. Start again from the Connect with ORCID button.",
            true,
        )
        .await;
        return Ok(Redirect::to("/me/edit").into_response());
    };
    if q.state.as_deref() != Some(expected_state.as_str()) {
        set_orcid_flash(
            &session,
            "ORCID sign-in state did not match. Start again from the Connect with ORCID button.",
            true,
        )
        .await;
        return Ok(Redirect::to("/me/edit").into_response());
    }
    let Some(code) = q.code.as_deref().map(str::trim).filter(|s| !s.is_empty()) else {
        set_orcid_flash(
            &session,
            "ORCID did not return an authorization code.",
            true,
        )
        .await;
        return Ok(Redirect::to("/me/edit").into_response());
    };
    let cfg = match crate::orcid::oauth_config(state.app_url.as_deref()) {
        Ok(Some(cfg)) => cfg,
        Ok(None) => {
            set_orcid_flash(
                &session,
                "ORCID OAuth is not configured on the server anymore. Try again later.",
                true,
            )
            .await;
            return Ok(Redirect::to("/me/edit").into_response());
        }
        Err(e) => {
            set_orcid_flash(
                &session,
                format!("ORCID OAuth configuration error: {e}"),
                true,
            )
            .await;
            return Ok(Redirect::to("/me/edit").into_response());
        }
    };
    let authenticated = match crate::orcid::exchange_authorization_code(&cfg, code, &expected_nonce)
        .await
    {
        Ok(v) => v,
        Err(e) => {
            tracing::warn!(user_id = u.id, error = %e, "ORCID OAuth exchange failed");
            set_orcid_flash(
                &session,
                "ORCID sign-in failed while exchanging the authorization code. Try again in a minute.",
                true,
            )
            .await;
            return Ok(Redirect::to("/me/edit").into_response());
        }
    };
    let existing: Option<(i64, String)> = sqlx::query_as(
        "SELECT id, username FROM users
          WHERE orcid_oauth_sub = ? AND id != ?
          LIMIT 1",
    )
    .bind(&authenticated.orcid)
    .bind(u.id)
    .fetch_optional(&state.pool)
    .await?;
    if let Some((_id, username)) = existing {
        set_orcid_flash(
            &session,
            format!(
                "ORCID iD {} is already connected to account @{username}. Disconnect it there or contact an admin.",
                authenticated.orcid
            ),
            true,
        )
        .await;
        return Ok(Redirect::to("/me/edit").into_response());
    }

    sqlx::query(
        "UPDATE users
            SET orcid = ?,
                orcid_oauth_sub = ?,
                orcid_oauth_verified = 1,
                orcid_oauth_verified_at = CURRENT_TIMESTAMP,
                orcid_verified = 0
          WHERE id = ?",
    )
    .bind(&authenticated.orcid)
    .bind(&authenticated.orcid)
    .bind(u.id)
    .execute(&state.pool)
    .await?;

    let who = authenticated
        .name
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(|name| format!(" as {name}"))
        .unwrap_or_default();
    set_orcid_flash(
        &session,
        format!(
            "ORCID iD {} authenticated{who}. This now counts as verified-scholar status.",
            authenticated.orcid
        ),
        false,
    )
    .await;
    Ok(Redirect::to("/me/edit").into_response())
}

fn opt(s: &str) -> Option<&str> {
    if s.is_empty() {
        None
    } else {
        Some(s)
    }
}

fn orcid_oauth_unavailable_message(state: &AppState) -> Option<String> {
    match crate::orcid::oauth_config(state.app_url.as_deref()) {
        Ok(Some(_)) => None,
        Ok(None) => Some(
            "ORCID sign-in is not configured on this server yet. Add ORCID_CLIENT_ID and ORCID_CLIENT_SECRET to enable ownership-grade ORCID binding."
                .to_string(),
        ),
        Err(_) => Some(
            "ORCID sign-in is configured incorrectly on this server. Check ORCID_CLIENT_ID, ORCID_CLIENT_SECRET, and ORCID_REDIRECT_URI."
                .to_string(),
        ),
    }
}

fn valid_orcid(s: &str) -> bool {
    let s = s.as_bytes();
    if s.len() != 19 {
        return false;
    }
    for (i, &b) in s.iter().enumerate() {
        match i {
            4 | 9 | 14 => {
                if b != b'-' {
                    return false;
                }
            }
            18 => {
                if !(b.is_ascii_digit() || b == b'X') {
                    return false;
                }
            }
            _ => {
                if !b.is_ascii_digit() {
                    return false;
                }
            }
        }
    }
    true
}
