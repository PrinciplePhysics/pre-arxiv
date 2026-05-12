//! /m/{id}/revise — submit a new VERSION of an existing manuscript.
//!
//! Permissions: submitter or admin. Withdrawn manuscripts are frozen
//! (a tombstoned record can't be revised — withdraw it first to retract,
//! then submit a fresh manuscript if you want to replace it).
//!
//! What's revisable: title, abstract, authors, category, the PDF
//! (optional — keeps prior PDF if no new one uploaded), external_url,
//! conductor_notes, license, ai_training, AND a required
//! revision_note short string. Everything else (conductor identity,
//! audit, ids) stays put — those are submission-level facts.

use std::path::PathBuf;

use axum::extract::{Multipart, Path, State};
use axum::response::{Html, IntoResponse, Redirect, Response};
use chrono::Datelike;
use rand::Rng;
use tokio::fs;
use tokio::io::AsyncWriteExt;
use tower_sessions::Session;

use crate::auth::{verify_csrf, MaybeUser, RequireUser};
use crate::error::{AppError, AppResult};
use crate::helpers::{build_ctx, set_flash};
use crate::models::Manuscript;
use crate::state::AppState;
use crate::templates;
use crate::versions;

fn upload_dir() -> PathBuf {
    if let Ok(d) = std::env::var("UPLOAD_DIR") {
        return PathBuf::from(d);
    }
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .map(|p| p.join("public").join("uploads"))
        .unwrap_or_else(|| PathBuf::from("./public/uploads"))
}

fn sanitize_filename(name: &str) -> String {
    let s: String = name
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-') { c } else { '_' })
        .collect();
    if s.len() > 80 { s.chars().take(80).collect() } else { s }
}

async fn load_manuscript(state: &AppState, id: &str) -> AppResult<Manuscript> {
    let m: Option<Manuscript> = sqlx::query_as::<_, Manuscript>(
        r#"SELECT id, arxiv_like_id, doi, submitter_id, title, abstract, authors, category,
                  pdf_path, external_url,
                  conductor_type, conductor_ai_model, conductor_ai_model_public,
                  conductor_human, conductor_human_public, conductor_role, conductor_notes,
                  agent_framework,
                  has_auditor, auditor_name, auditor_affiliation, auditor_role,
                  auditor_statement, auditor_orcid,
                  view_count, score, comment_count,
                  withdrawn, withdrawn_reason, withdrawn_at,
                  created_at, updated_at,
                  license, ai_training, current_version
           FROM manuscripts WHERE arxiv_like_id = ? OR CAST(id AS TEXT) = ?
           LIMIT 1"#,
    )
    .bind(id)
    .bind(id)
    .fetch_optional(&state.pool)
    .await?;
    m.ok_or(AppError::NotFound)
}

fn slug_for(m: &Manuscript) -> String {
    m.arxiv_like_id.clone().unwrap_or_else(|| m.id.to_string())
}

// ─── GET /m/{id}/revise ───────────────────────────────────────────────────

pub async fn show(
    State(state): State<AppState>,
    session: Session,
    maybe_user: MaybeUser,
    RequireUser(user): RequireUser,
    Path(id): Path<String>,
) -> AppResult<Response> {
    let m = load_manuscript(&state, &id).await?;
    if m.submitter_id != user.id && !user.is_admin() {
        set_flash(&session, "Only the submitter (or an admin) may revise a manuscript.").await;
        return Ok(Redirect::to(&format!("/m/{}", slug_for(&m))).into_response());
    }
    if m.is_withdrawn() {
        set_flash(&session, "This manuscript is withdrawn and can't be revised.").await;
        return Ok(Redirect::to(&format!("/m/{}", slug_for(&m))).into_response());
    }
    let mut ctx = build_ctx(&session, maybe_user, "/m").await;
    ctx.no_index = true;
    Ok(Html(templates::revise::render(&ctx, &m, None).into_string()).into_response())
}

// ─── POST /m/{id}/revise ──────────────────────────────────────────────────

pub async fn submit(
    State(state): State<AppState>,
    session: Session,
    maybe_user: MaybeUser,
    RequireUser(user): RequireUser,
    Path(id): Path<String>,
    mut multipart: Multipart,
) -> AppResult<Response> {
    let m = load_manuscript(&state, &id).await?;
    if m.submitter_id != user.id && !user.is_admin() {
        set_flash(&session, "Only the submitter (or an admin) may revise a manuscript.").await;
        return Ok(Redirect::to(&format!("/m/{}", slug_for(&m))).into_response());
    }
    if m.is_withdrawn() {
        set_flash(&session, "This manuscript is withdrawn and can't be revised.").await;
        return Ok(Redirect::to(&format!("/m/{}", slug_for(&m))).into_response());
    }

    // Collect form fields and any PDF upload in memory. Matches the
    // initial-submission pattern: parse all fields, validate CSRF +
    // contents, only then write the PDF to disk.
    let mut csrf_token = String::new();
    let mut title = String::new();
    let mut r#abstract = String::new();
    let mut authors = String::new();
    let mut category = String::new();
    let mut external_url = String::new();
    let mut conductor_notes = String::new();
    let mut license = String::new();
    let mut ai_training = String::new();
    let mut revision_note = String::new();
    let mut keep_existing_pdf: bool = true;
    let mut pdf_buf: Option<(String, axum::body::Bytes)> = None;

    while let Some(field) = multipart
        .next_field()
        .await
        .map_err(|e| AppError::Other(anyhow::anyhow!("multipart: {e}")))?
    {
        let name = field.name().unwrap_or("").to_string();
        match name.as_str() {
            "pdf" => {
                let file_name = field.file_name().unwrap_or("upload.pdf").to_string();
                let safe = sanitize_filename(&file_name);
                if !safe.to_ascii_lowercase().ends_with(".pdf") { continue; }
                let data = field.bytes().await
                    .map_err(|e| AppError::Other(anyhow::anyhow!("multipart: {e}")))?;
                if data.is_empty() { continue; }
                if data.len() > 30 * 1024 * 1024 {
                    let mut ctx = build_ctx(&session, maybe_user, "/m").await;
                    ctx.no_index = true;
                    return Ok(Html(templates::revise::render(&ctx, &m, Some("New PDF exceeds 30 MB.")).into_string()).into_response());
                }
                if !data.starts_with(b"%PDF-") {
                    let mut ctx = build_ctx(&session, maybe_user, "/m").await;
                    ctx.no_index = true;
                    return Ok(Html(templates::revise::render(&ctx, &m, Some("Uploaded file is not a valid PDF (missing %PDF header).")).into_string()).into_response());
                }
                pdf_buf = Some((safe, data));
                keep_existing_pdf = false;
            }
            "remove_pdf" => {
                if field.text().await.unwrap_or_default() == "1" {
                    keep_existing_pdf = false;
                    pdf_buf = None;
                }
            }
            _ => {
                let value = field.text().await.unwrap_or_default();
                match name.as_str() {
                    "csrf_token"      => csrf_token = value,
                    "title"           => title = value,
                    "abstract"        => r#abstract = value,
                    "authors"         => authors = value,
                    "category"        => category = value,
                    "external_url"    => external_url = value,
                    "conductor_notes" => conductor_notes = value,
                    "license"         => license = value,
                    "ai_training"     => ai_training = value,
                    "revision_note"   => revision_note = value,
                    _ => {}
                }
            }
        }
    }

    let mut ctx_err = build_ctx(&session, maybe_user, "/m").await;
    ctx_err.no_index = true;
    if !verify_csrf(&session, &csrf_token).await {
        return Ok(Html(templates::revise::render(&ctx_err, &m, Some("Form expired — please try again.")).into_string()).into_response());
    }
    let title_t = title.trim();
    let abstract_t = r#abstract.trim();
    let authors_t = authors.trim();
    let category_t = category.trim();
    let revision_note_t = revision_note.trim();
    if title_t.is_empty() {
        return Ok(Html(templates::revise::render(&ctx_err, &m, Some("Title is required.")).into_string()).into_response());
    }
    if abstract_t.len() < 100 {
        return Ok(Html(templates::revise::render(&ctx_err, &m, Some("Abstract must be at least 100 characters.")).into_string()).into_response());
    }
    if authors_t.is_empty() {
        return Ok(Html(templates::revise::render(&ctx_err, &m, Some("At least one author required.")).into_string()).into_response());
    }
    if category_t.is_empty() {
        return Ok(Html(templates::revise::render(&ctx_err, &m, Some("Category is required.")).into_string()).into_response());
    }
    if revision_note_t.is_empty() {
        return Ok(Html(templates::revise::render(&ctx_err, &m, Some("Revision note is required — a short summary of what changed (e.g. \"Fixed typo in Theorem 2.1\").")).into_string()).into_response());
    }
    let license_resolved: &str = if license.trim().is_empty() {
        m.license.as_deref().unwrap_or("CC-BY-4.0")
    } else if crate::licenses::lookup(license.trim()).is_some() {
        license.trim()
    } else {
        return Ok(Html(templates::revise::render(&ctx_err, &m, Some("Unknown reader license.")).into_string()).into_response());
    };
    let ai_training_resolved: &str = if ai_training.trim().is_empty() {
        m.ai_training.as_deref().unwrap_or("allow")
    } else if crate::licenses::ai_training_lookup(ai_training.trim()).is_some() {
        ai_training.trim()
    } else {
        return Ok(Html(templates::revise::render(&ctx_err, &m, Some("Unknown AI-training option.")).into_string()).into_response());
    };

    // Persist PDF (if a new one was uploaded). Otherwise reuse the
    // existing pdf_path. Or, if remove_pdf was checked, drop the PDF
    // entirely and keep only external_url.
    let pdf_path_to_store: Option<String> = match (&pdf_buf, keep_existing_pdf) {
        (Some((safe, data)), _) => {
            let upload_dir = upload_dir();
            fs::create_dir_all(&upload_dir).await
                .map_err(|e| AppError::Other(e.into()))?;
            let stored = format!(
                "{}-{}-{}",
                chrono::Utc::now().timestamp_millis(),
                rand::thread_rng().gen_range(100_000..1_000_000),
                safe
            );
            let full = upload_dir.join(&stored);
            let mut f = fs::File::create(&full).await
                .map_err(|e| AppError::Other(e.into()))?;
            f.write_all(data).await
                .map_err(|e| AppError::Other(e.into()))?;
            Some(stored)
        }
        (None, true)  => m.pdf_path.clone(),
        (None, false) => None,
    };
    let _ = chrono::Utc::now().year();

    let ext_url_opt: Option<&str> = if external_url.trim().is_empty() {
        None
    } else {
        Some(external_url.trim())
    };
    let conductor_notes_opt: Option<&str> = if conductor_notes.trim().is_empty() {
        None
    } else {
        Some(conductor_notes.trim())
    };

    let v = versions::VersionInput {
        title: title_t,
        r#abstract: abstract_t,
        authors: authors_t,
        category: category_t,
        pdf_path: pdf_path_to_store.as_deref(),
        external_url: ext_url_opt,
        conductor_notes: conductor_notes_opt,
        license: license_resolved,
        ai_training: ai_training_resolved,
        revision_note: Some(revision_note_t),
    };
    let new_version = versions::mint_revision(&state.pool, m.id, &v)
        .await
        .map_err(|e| AppError::Other(anyhow::anyhow!("{e}")))?;

    let _ = sqlx::query(
        "INSERT INTO audit_log (actor_user_id, action, target_type, target_id, detail) VALUES (?, 'manuscript_revise', 'manuscript', ?, ?)",
    )
    .bind(user.id)
    .bind(m.id)
    .bind(format!("v{new_version}: {revision_note_t}"))
    .execute(&state.pool)
    .await;

    let slug = slug_for(&m);
    set_flash(&session, format!("Revision saved as v{new_version}. The latest version is now live.")).await;
    Ok(Redirect::to(&format!("/m/{slug}")).into_response())
}
