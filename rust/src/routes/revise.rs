//! /m/{id}/revise — submit a new VERSION of an existing manuscript.
//!
//! Permissions: submitter or admin. Withdrawn manuscripts are frozen
//! (a tombstoned record can't be revised — withdraw it first to retract,
//! then submit a fresh manuscript if you want to replace it).
//!
//! What's revisable: title, abstract, authors, category, stored source/PDF
//! artifacts, external_url, conductor disclosure flags, conductor_notes,
//! license, ai_training, AND a required revision_note short string.
//! Everything else (underlying conductor identity, audit, ids) stays put —
//! those are submission-level facts.

use std::path::{Path as FsPath, PathBuf};

use axum::extract::{Multipart, Path, State};
use axum::response::{Html, IntoResponse, Redirect, Response};
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
        .map(|c| {
            if c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-') {
                c
            } else {
                '_'
            }
        })
        .collect();
    if s.len() > 80 {
        s.chars().take(80).collect()
    } else {
        s
    }
}

async fn load_manuscript(state: &AppState, id: &str) -> AppResult<Manuscript> {
    let m: Option<Manuscript> = sqlx::query_as::<_, Manuscript>(crate::db::pg(
        r#"SELECT id, arxiv_like_id, doi, submitter_id, title, abstract, authors, category,
                  pdf_path, external_url, source_path,
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
    ))
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
        set_flash(
            &session,
            "Only the submitter (or an admin) may revise a manuscript.",
        )
        .await;
        return Ok(Redirect::to(&format!("/abs/{}", slug_for(&m))).into_response());
    }
    if !user.is_verified_or_admin() {
        set_flash(&session, "Verify your email before revising a manuscript.").await;
        return Ok(Redirect::to(&format!("/abs/{}", slug_for(&m))).into_response());
    }
    if m.is_withdrawn() {
        set_flash(
            &session,
            "This manuscript is withdrawn and can't be revised.",
        )
        .await;
        return Ok(Redirect::to(&format!("/abs/{}", slug_for(&m))).into_response());
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
        set_flash(
            &session,
            "Only the submitter (or an admin) may revise a manuscript.",
        )
        .await;
        return Ok(Redirect::to(&format!("/abs/{}", slug_for(&m))).into_response());
    }
    if !user.is_verified_or_admin() {
        set_flash(&session, "Verify your email before revising a manuscript.").await;
        return Ok(Redirect::to(&format!("/abs/{}", slug_for(&m))).into_response());
    }
    if m.is_withdrawn() {
        set_flash(
            &session,
            "This manuscript is withdrawn and can't be revised.",
        )
        .await;
        return Ok(Redirect::to(&format!("/abs/{}", slug_for(&m))).into_response());
    }

    // Collect form fields and any source/PDF upload in memory. Matches
    // the initial-submission pattern: parse all fields, validate CSRF +
    // contents, only then write artifacts to disk.
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
    let mut conductor_ai_model_public = m.conductor_ai_model_public != 0;
    let mut conductor_human_public = m.conductor_human_public != 0;
    let mut pdf_buf: Option<(String, axum::body::Bytes)> = None;
    let mut source_buf: Option<(String, axum::body::Bytes)> = None;

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
                if !safe.to_ascii_lowercase().ends_with(".pdf") {
                    continue;
                }
                let data = field
                    .bytes()
                    .await
                    .map_err(|e| AppError::Other(anyhow::anyhow!("multipart: {e}")))?;
                if data.is_empty() {
                    continue;
                }
                if data.len() > 30 * 1024 * 1024 {
                    let mut ctx = build_ctx(&session, maybe_user, "/m").await;
                    ctx.no_index = true;
                    return Ok(Html(
                        templates::revise::render(&ctx, &m, Some("New PDF exceeds 30 MB."))
                            .into_string(),
                    )
                    .into_response());
                }
                if !data.starts_with(b"%PDF-") {
                    let mut ctx = build_ctx(&session, maybe_user, "/m").await;
                    ctx.no_index = true;
                    return Ok(Html(
                        templates::revise::render(
                            &ctx,
                            &m,
                            Some("Uploaded file is not a valid PDF (missing %PDF header)."),
                        )
                        .into_string(),
                    )
                    .into_response());
                }
                pdf_buf = Some((safe, data));
            }
            "source" => {
                let file_name = field.file_name().unwrap_or("source.tex").to_string();
                let safe = sanitize_filename(&file_name);
                let data = field
                    .bytes()
                    .await
                    .map_err(|e| AppError::Other(anyhow::anyhow!("multipart: {e}")))?;
                if data.is_empty() {
                    continue;
                }
                if data.len() > 30 * 1024 * 1024 {
                    let mut ctx = build_ctx(&session, maybe_user, "/m").await;
                    ctx.no_index = true;
                    return Ok(Html(
                        templates::revise::render(&ctx, &m, Some("Source upload exceeds 30 MB."))
                            .into_string(),
                    )
                    .into_response());
                }
                source_buf = Some((safe, data));
            }
            _ => {
                let value = field.text().await.unwrap_or_default();
                match name.as_str() {
                    "csrf_token" => csrf_token = value,
                    "title" => title = value,
                    "abstract" => r#abstract = value,
                    "authors" => authors = value,
                    "category" => category = value,
                    "external_url" => external_url = value,
                    "conductor_notes" => conductor_notes = value,
                    "license" => license = value,
                    "ai_training" => ai_training = value,
                    "revision_note" => revision_note = value,
                    "conductor_ai_model_public" => conductor_ai_model_public = is_truthy(&value),
                    "conductor_human_public" => conductor_human_public = is_truthy(&value),
                    _ => {}
                }
            }
        }
    }

    let mut ctx_err = build_ctx(&session, maybe_user, "/m").await;
    ctx_err.no_index = true;
    if !verify_csrf(&session, &csrf_token).await {
        return Ok(Html(
            templates::revise::render(&ctx_err, &m, Some("Form expired — please try again."))
                .into_string(),
        )
        .into_response());
    }
    let title_t = title.trim();
    let abstract_t = r#abstract.trim();
    let authors_t = authors.trim();
    let category_t = category.trim();
    let revision_note_t = revision_note.trim();
    if title_t.is_empty() {
        return Ok(Html(
            templates::revise::render(&ctx_err, &m, Some("Title is required.")).into_string(),
        )
        .into_response());
    }
    if abstract_t.len() < 100 {
        return Ok(Html(
            templates::revise::render(
                &ctx_err,
                &m,
                Some("Abstract must be at least 100 characters."),
            )
            .into_string(),
        )
        .into_response());
    }
    if authors_t.is_empty() {
        return Ok(Html(
            templates::revise::render(&ctx_err, &m, Some("At least one author required."))
                .into_string(),
        )
        .into_response());
    }
    if category_t.is_empty() {
        return Ok(Html(
            templates::revise::render(&ctx_err, &m, Some("Category is required.")).into_string(),
        )
        .into_response());
    }
    if revision_note_t.is_empty() {
        return Ok(Html(templates::revise::render(&ctx_err, &m, Some("Revision note is required — a short summary of what changed (e.g. \"Fixed typo in Theorem 2.1\").")).into_string()).into_response());
    }
    let license_resolved: &str = if license.trim().is_empty() {
        m.license.as_deref().unwrap_or("CC-BY-4.0")
    } else if crate::licenses::lookup(license.trim()).is_some() {
        license.trim()
    } else {
        return Ok(Html(
            templates::revise::render(&ctx_err, &m, Some("Unknown reader license.")).into_string(),
        )
        .into_response());
    };
    let ai_training_resolved: &str = if ai_training.trim().is_empty() {
        m.ai_training.as_deref().unwrap_or("allow")
    } else if crate::licenses::ai_training_lookup(ai_training.trim()).is_some() {
        ai_training.trim()
    } else {
        return Ok(Html(
            templates::revise::render(&ctx_err, &m, Some("Unknown AI-training option."))
                .into_string(),
        )
        .into_response());
    };

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

    let revised_human_public = if m.conductor_type == "human-ai" {
        conductor_human_public
    } else {
        m.conductor_human_public != 0
    };
    let final_hides_identity =
        !conductor_ai_model_public || (m.conductor_type == "human-ai" && !revised_human_public);
    let privacy_tightened = (m.conductor_ai_model_public != 0 && !conductor_ai_model_public)
        || (m.conductor_type == "human-ai"
            && m.conductor_human_public != 0
            && !revised_human_public);

    if source_buf.is_some() && pdf_buf.is_some() {
        return Ok(Html(templates::revise::render(&ctx_err, &m, Some("Upload either replacement LaTeX source or a replacement PDF, not both. Source uploads compile their own PDF.")).into_string()).into_response());
    }
    if final_hides_identity && pdf_buf.is_some() {
        return Ok(Html(templates::revise::render(&ctx_err, &m, Some("Private conductor/model fields require a LaTeX source upload so PreXiv can black out the public source and compiled PDF. Direct PDF replacement cannot be automatically redacted.")).into_string()).into_response());
    }
    if final_hides_identity
        && source_buf.is_none()
        && ((privacy_tightened && (m.pdf_path.is_some() || m.source_path.is_some()))
            || (m.pdf_path.is_some() && m.source_path.is_none()))
    {
        return Ok(Html(templates::revise::render(&ctx_err, &m, Some("Changing a public conductor/model field to private requires replacement LaTeX source. Otherwise PreXiv could keep serving an older unredacted PDF/source artifact.")).into_string()).into_response());
    }

    // Persist replacement artifacts if present. A source upload replaces
    // both public source and PDF; a direct PDF upload clears any older
    // source artifact because the source would no longer match the PDF.
    // Revisions cannot remove the stored artifact entirely: PreXiv must
    // continue hosting the paper, with external_url only as a supplement.
    let upload_dir = upload_dir();
    let watermark_id = m
        .arxiv_like_id
        .clone()
        .unwrap_or_else(|| format!("prexiv:{}", m.id));
    let app_url = state.app_url.as_deref().unwrap_or("http://localhost:3001");
    let stamp = chrono::Utc::now().timestamp_millis();
    let rnd = rand::thread_rng().gen_range(100_000..1_000_000);
    let mut pdf_path_to_store = m.pdf_path.clone();
    let mut source_path_to_store = m.source_path.clone();
    let mut new_pdf_for_cleanup: Option<String> = None;
    let mut new_source_for_cleanup: Option<String> = None;

    if let Some((safe, data)) = &source_buf {
        fs::create_dir_all(&upload_dir)
            .await
            .map_err(|e| AppError::Other(e.into()))?;
        let redaction = crate::compile::RedactionOptions {
            hide_human: m.conductor_type == "human-ai" && !revised_human_public,
            hide_ai_model: !conductor_ai_model_public,
            human_name: m.conductor_human.clone(),
            ai_models: m.ai_models().into_iter().map(str::to_string).collect(),
        };
        let prepared = match crate::compile::prepare_source(safe, data, &redaction) {
            Ok(prepared) => prepared,
            Err(e) => {
                let msg = format!("LaTeX source preparation failed: {e}");
                return Ok(
                    Html(templates::revise::render(&ctx_err, &m, Some(&msg)).into_string())
                        .into_response(),
                );
            }
        };

        let stored_src = format!(
            "{}-{}-src-{}",
            stamp,
            rnd,
            sanitize_filename(&prepared.filename)
        );
        let full_src = upload_dir.join(&stored_src);
        let mut f = fs::File::create(&full_src)
            .await
            .map_err(|e| AppError::Other(e.into()))?;
        f.write_all(&prepared.data)
            .await
            .map_err(|e| AppError::Other(e.into()))?;
        source_path_to_store = Some(stored_src.clone());
        new_source_for_cleanup = Some(stored_src);

        match crate::compile::compile(&prepared.filename, &prepared.data).await {
            Ok(compiled) => {
                let watermarked = match crate::pdf_watermark::watermark_pdf(
                    &compiled.pdf,
                    &watermark_id,
                    category_t,
                    app_url,
                )
                .await
                {
                    Ok(pdf) => pdf,
                    Err(e) => {
                        cleanup_uploads(&upload_dir, None, new_source_for_cleanup.as_deref()).await;
                        let msg = format!("PDF watermarking failed: {e}");
                        return Ok(Html(
                            templates::revise::render(&ctx_err, &m, Some(&msg)).into_string(),
                        )
                        .into_response());
                    }
                };
                let pdf_name = format!("{stamp}-{rnd}-compiled.pdf");
                let pdf_full = upload_dir.join(&pdf_name);
                let mut pf = fs::File::create(&pdf_full)
                    .await
                    .map_err(|e| AppError::Other(e.into()))?;
                pf.write_all(&watermarked)
                    .await
                    .map_err(|e| AppError::Other(e.into()))?;
                pdf_path_to_store = Some(pdf_name.clone());
                new_pdf_for_cleanup = Some(pdf_name);
            }
            Err(e) => {
                cleanup_uploads(&upload_dir, None, new_source_for_cleanup.as_deref()).await;
                let log_excerpt = e.log().map(|s| s.to_string());
                let msg = match log_excerpt {
                    Some(log) => format!(
                        "LaTeX compile failed: {e}\n\nLast lines of the compile log:\n\n{log}"
                    ),
                    None => format!("LaTeX compile failed: {e}"),
                };
                return Ok(
                    Html(templates::revise::render(&ctx_err, &m, Some(&msg)).into_string())
                        .into_response(),
                );
            }
        }
    } else if let Some((safe, data)) = &pdf_buf {
        fs::create_dir_all(&upload_dir)
            .await
            .map_err(|e| AppError::Other(e.into()))?;
        let watermarked =
            match crate::pdf_watermark::watermark_pdf(data, &watermark_id, category_t, app_url)
                .await
            {
                Ok(pdf) => pdf,
                Err(e) => {
                    let msg = format!("PDF watermarking failed: {e}");
                    return Ok(Html(
                        templates::revise::render(&ctx_err, &m, Some(&msg)).into_string(),
                    )
                    .into_response());
                }
            };
        let stored = format!("{stamp}-{rnd}-{safe}");
        let full = upload_dir.join(&stored);
        let mut f = fs::File::create(&full)
            .await
            .map_err(|e| AppError::Other(e.into()))?;
        f.write_all(&watermarked)
            .await
            .map_err(|e| AppError::Other(e.into()))?;
        pdf_path_to_store = Some(stored.clone());
        source_path_to_store = None;
        new_pdf_for_cleanup = Some(stored);
    }

    if pdf_path_to_store.is_none() {
        cleanup_uploads(
            &upload_dir,
            new_pdf_for_cleanup.as_deref(),
            new_source_for_cleanup.as_deref(),
        )
        .await;
        return Ok(Html(
            templates::revise::render(
                &ctx_err,
                &m,
                Some("A revision must keep or upload a PreXiv-hosted PDF/source artifact. External URL is only a supplemental link."),
            )
            .into_string(),
        )
        .into_response());
    }
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
    let new_version = match versions::mint_revision_with_publication(
        &state.pool,
        m.id,
        &v,
        source_path_to_store.as_deref(),
        if conductor_ai_model_public { 1i64 } else { 0 },
        if revised_human_public { 1i64 } else { 0 },
    )
    .await
    {
        Ok(version) => version,
        Err(e) => {
            cleanup_uploads(
                &upload_dir,
                new_pdf_for_cleanup.as_deref(),
                new_source_for_cleanup.as_deref(),
            )
            .await;
            return Err(AppError::Other(anyhow::anyhow!("{e}")));
        }
    };

    let _ = sqlx::query(
        crate::db::pg("INSERT INTO audit_log (actor_user_id, action, target_type, target_id, detail) VALUES (?, 'manuscript_revise', 'manuscript', ?, ?)"),
    )
    .bind(user.id)
    .bind(m.id)
    .bind(format!("v{new_version}: {revision_note_t}"))
    .execute(&state.pool)
    .await;

    let slug = slug_for(&m);
    set_flash(
        &session,
        format!("Revision saved as v{new_version}. The latest version is now live."),
    )
    .await;
    Ok(Redirect::to(&format!("/abs/{slug}")).into_response())
}

async fn cleanup_uploads(upload_dir: &FsPath, pdf_path: Option<&str>, source_path: Option<&str>) {
    if let Some(path) = pdf_path {
        let _ = fs::remove_file(upload_dir.join(path)).await;
    }
    if let Some(path) = source_path {
        let _ = fs::remove_file(upload_dir.join(path)).await;
    }
}

fn is_truthy(s: &str) -> bool {
    matches!(s, "1" | "on" | "true" | "yes")
}
