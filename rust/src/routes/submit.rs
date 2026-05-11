use std::path::PathBuf;

use axum::extract::{Multipart, State};
use axum::response::{Html, IntoResponse, Redirect, Response};
use chrono::Datelike;
use rand::Rng;
use tokio::fs;
use tokio::io::AsyncWriteExt;
use tower_sessions::Session;

use crate::auth::{verify_csrf, MaybeUser, RequireUser};
use crate::error::AppResult;
use crate::helpers::{build_ctx, set_flash};
use crate::state::AppState;
use crate::templates;

pub async fn show_submit(
    State(_state): State<AppState>,
    session: Session,
    maybe_user: MaybeUser,
    RequireUser(_user): RequireUser,
) -> AppResult<Html<String>> {
    let mut ctx = build_ctx(&session, maybe_user, "/submit").await;
    ctx.no_index = true;
    Ok(Html(templates::submit::render(&ctx, None).into_string()))
}

pub async fn do_submit(
    State(state): State<AppState>,
    session: Session,
    maybe_user: MaybeUser,
    RequireUser(user): RequireUser,
    mut multipart: Multipart,
) -> AppResult<Response> {
    let mut fields: SubmitFields = SubmitFields::default();
    let mut pdf_path: Option<String> = None;

    let upload_dir = upload_dir();
    fs::create_dir_all(&upload_dir)
        .await
        .map_err(|e| crate::error::AppError::Other(e.into()))?;

    while let Some(field) = multipart
        .next_field()
        .await
        .map_err(|e| crate::error::AppError::Other(anyhow::anyhow!("multipart: {e}")))?
    {
        let name = field.name().unwrap_or("").to_string();
        if name == "pdf" {
            let file_name = field.file_name().unwrap_or("upload.pdf").to_string();
            let safe = sanitize_filename(&file_name);
            if !safe.to_ascii_lowercase().ends_with(".pdf") {
                continue;
            }
            let data = field
                .bytes()
                .await
                .map_err(|e| crate::error::AppError::Other(anyhow::anyhow!("multipart: {e}")))?;
            if data.is_empty() {
                continue;
            }
            if data.len() > 30 * 1024 * 1024 {
                return Ok(err_page(&session, maybe_user, "PDF exceeds 30 MB.").await);
            }
            let stored = format!(
                "{}-{}-{}",
                chrono::Utc::now().timestamp_millis(),
                rand::thread_rng().gen_range(100_000..1_000_000),
                safe
            );
            let full = upload_dir.join(&stored);
            let mut f = fs::File::create(&full)
                .await
                .map_err(|e| crate::error::AppError::Other(e.into()))?;
            f.write_all(&data)
                .await
                .map_err(|e| crate::error::AppError::Other(e.into()))?;
            pdf_path = Some(stored);
        } else {
            let value = field
                .text()
                .await
                .unwrap_or_default();
            fields.set(&name, value);
        }
    }

    if !verify_csrf(&session, &fields.csrf_token).await {
        return Ok(err_page(&session, maybe_user, "Form expired — please try again.").await);
    }
    if fields.title.trim().is_empty() || fields.r#abstract.trim().len() < 100 {
        return Ok(err_page(&session, maybe_user, "Title required; abstract must be at least 100 chars.").await);
    }
    if fields.authors.trim().is_empty() {
        return Ok(err_page(&session, maybe_user, "At least one author required.").await);
    }
    if fields.conductor_ai_model.trim().is_empty() {
        return Ok(err_page(&session, maybe_user, "Conductor AI model required.").await);
    }
    if fields.conductor_type == "human-ai" && fields.conductor_human.trim().is_empty() {
        return Ok(err_page(&session, maybe_user, "Human conductor name required for Human + AI submissions.").await);
    }

    // Audit-status post-processing.
    //
    //   "none"  → no auditor row at all
    //   "self"  → has_auditor=1, auditor_name/role copied from conductor;
    //             ORCID left blank (the conductor has already identified
    //             themselves once in the conductor section); only valid for
    //             human-ai conductors
    //   "other" → has_auditor=1, all auditor_* fields from the form
    //
    // The form's CSS hides the inactive blocks but their fields still
    // submit, so we deliberately ignore them and pick only the ones that
    // belong to the chosen audit_status.
    let (has_auditor, auditor_name, auditor_affiliation, auditor_role, auditor_statement, auditor_orcid)
        = match fields.audit_status.as_str() {
            "self" if fields.conductor_type == "human-ai" => {
                if fields.self_audit_statement.trim().is_empty() {
                    return Ok(err_page(&session, maybe_user, "Self-audit statement is required when you tick 'I am the auditor'. Say what you actually verified.").await);
                }
                (
                    true,
                    Some(fields.conductor_human.trim().to_string()),
                    None,
                    if fields.conductor_role.trim().is_empty() { None } else { Some(fields.conductor_role.trim().to_string()) },
                    Some(fields.self_audit_statement.trim().to_string()),
                    None,
                )
            }
            "other" => {
                if fields.auditor_name.trim().is_empty() {
                    return Ok(err_page(&session, maybe_user, "Auditor name is required when you select 'Someone else audited this'.").await);
                }
                if fields.auditor_statement.trim().is_empty() {
                    return Ok(err_page(&session, maybe_user, "Auditor statement is required when you select 'Someone else audited this'.").await);
                }
                (
                    true,
                    Some(fields.auditor_name.trim().to_string()),
                    if fields.auditor_affiliation.trim().is_empty() { None } else { Some(fields.auditor_affiliation.trim().to_string()) },
                    if fields.auditor_role.trim().is_empty() { None } else { Some(fields.auditor_role.trim().to_string()) },
                    Some(fields.auditor_statement.trim().to_string()),
                    if fields.auditor_orcid.trim().is_empty() { None } else { Some(fields.auditor_orcid.trim().to_string()) },
                )
            }
            _ => (false, None, None, None, None, None),
        };

    // Licensing — validate against the canonical lists. Empty defaults
    // accepted (sane fallback for legacy form posts).
    let license = if fields.license.trim().is_empty() {
        "CC-BY-4.0"
    } else if crate::licenses::lookup(fields.license.trim()).is_some() {
        fields.license.trim()
    } else {
        return Ok(err_page(&session, maybe_user, "Unknown reader license. Pick from the dropdown.").await);
    };
    let ai_training = if fields.ai_training.trim().is_empty() {
        "allow"
    } else if crate::licenses::ai_training_lookup(fields.ai_training.trim()).is_some() {
        fields.ai_training.trim()
    } else {
        return Ok(err_page(&session, maybe_user, "Unknown AI-training option. Pick from the dropdown.").await);
    };

    let arxiv_like_id = make_prexiv_id();
    let synthetic_doi = format!("10.99999/{}", arxiv_like_id);

    let result = sqlx::query(
        r#"INSERT INTO manuscripts (
            arxiv_like_id, doi, submitter_id, title, abstract, authors, category,
            pdf_path, external_url,
            conductor_type, conductor_ai_model, conductor_ai_model_public,
            conductor_human, conductor_human_public, conductor_role, conductor_notes,
            agent_framework,
            has_auditor, auditor_name, auditor_affiliation, auditor_role,
            auditor_statement, auditor_orcid,
            license, ai_training
        ) VALUES (
            ?, ?, ?, ?, ?, ?, ?,
            ?, ?,
            ?, ?, ?,
            ?, ?, ?, ?,
            ?,
            ?, ?, ?, ?,
            ?, ?,
            ?, ?
        )"#,
    )
    .bind(&arxiv_like_id)
    .bind(&synthetic_doi)
    .bind(user.id)
    .bind(fields.title.trim())
    .bind(fields.r#abstract.trim())
    .bind(fields.authors.trim())
    .bind(fields.category.trim())
    .bind(pdf_path.as_deref())
    .bind(opt(&fields.external_url))
    .bind(&fields.conductor_type)
    .bind(fields.conductor_ai_model.trim())
    .bind(if fields.conductor_ai_model_public { 1i64 } else { 0 })
    .bind(opt(&fields.conductor_human))
    .bind(if fields.conductor_human_public { 1i64 } else { 0 })
    .bind(opt(&fields.conductor_role))
    .bind(opt(&fields.conductor_notes))
    .bind(opt(&fields.agent_framework))
    .bind(if has_auditor { 1i64 } else { 0 })
    .bind(auditor_name.as_deref())
    .bind(auditor_affiliation.as_deref())
    .bind(auditor_role.as_deref())
    .bind(auditor_statement.as_deref())
    .bind(auditor_orcid.as_deref())
    .bind(license)
    .bind(ai_training)
    .execute(&state.pool)
    .await?;

    let _ = result.last_insert_rowid();
    set_flash(&session, format!("Manuscript submitted as {arxiv_like_id}.")).await;
    Ok(Redirect::to(&format!("/m/{arxiv_like_id}")).into_response())
}

fn opt(s: &str) -> Option<&str> {
    let t = s.trim();
    if t.is_empty() { None } else { Some(t) }
}

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
    if s.len() > 80 {
        s.chars().take(80).collect()
    } else {
        s
    }
}

fn make_prexiv_id() -> String {
    let now = chrono::Utc::now();
    let yy = now.year() % 100;
    let mm = now.month();
    let serial: u32 = rand::thread_rng().gen_range(0..100_000);
    format!("prexiv:{:02}{:02}.{:05}", yy, mm, serial)
}

async fn err_page(session: &Session, maybe_user: MaybeUser, msg: &str) -> Response {
    let mut ctx = build_ctx(session, maybe_user, "/submit").await;
    ctx.no_index = true;
    Html(templates::submit::render(&ctx, Some(msg)).into_string()).into_response()
}

#[derive(Default)]
struct SubmitFields {
    csrf_token: String,
    title: String,
    r#abstract: String,
    authors: String,
    category: String,
    external_url: String,
    conductor_type: String,
    conductor_ai_model: String,
    conductor_ai_model_public: bool,
    conductor_human: String,
    conductor_human_public: bool,
    conductor_role: String,
    conductor_notes: String,
    agent_framework: String,
    /// "none" / "self" / "other"
    audit_status: String,
    self_audit_statement: String,
    auditor_name: String,
    auditor_affiliation: String,
    auditor_role: String,
    auditor_statement: String,
    auditor_orcid: String,
    license: String,
    ai_training: String,
}

impl SubmitFields {
    fn set(&mut self, name: &str, v: String) {
        match name {
            "csrf_token" => self.csrf_token = v,
            "title" => self.title = v,
            "abstract" => self.r#abstract = v,
            "authors" => self.authors = v,
            "category" => self.category = v,
            "external_url" => self.external_url = v,
            "conductor_type" => self.conductor_type = v,
            "conductor_ai_model" => self.conductor_ai_model = v,
            "conductor_ai_model_public" => self.conductor_ai_model_public = is_truthy(&v),
            "conductor_human" => self.conductor_human = v,
            "conductor_human_public" => self.conductor_human_public = is_truthy(&v),
            "conductor_role" => self.conductor_role = v,
            "conductor_notes" => self.conductor_notes = v,
            "agent_framework" => self.agent_framework = v,
            "audit_status" => self.audit_status = v,
            "self_audit_statement" => self.self_audit_statement = v,
            "auditor_name" => self.auditor_name = v,
            "auditor_affiliation" => self.auditor_affiliation = v,
            "auditor_role" => self.auditor_role = v,
            "auditor_statement" => self.auditor_statement = v,
            "auditor_orcid" => self.auditor_orcid = v,
            "license" => self.license = v,
            "ai_training" => self.ai_training = v,
            _ => {}
        }
    }
}

fn is_truthy(s: &str) -> bool {
    matches!(s, "1" | "on" | "true" | "yes")
}
