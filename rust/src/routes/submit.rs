use std::path::PathBuf;

use axum::extract::{Multipart, State};
use axum::response::{Html, IntoResponse, Redirect, Response};
use chrono::Datelike;
use rand::Rng;
use tokio::fs;
use tokio::io::AsyncWriteExt;
use tower_sessions::Session;

use crate::auth::{verify_csrf, AuthSource, MaybeUser, RequireAuthUser};
use crate::error::AppResult;
use crate::helpers::{build_ctx, set_flash};
use crate::state::AppState;
use crate::templates;

pub async fn show_submit(
    State(_state): State<AppState>,
    session: Session,
    auth: RequireAuthUser,
) -> AppResult<Html<String>> {
    let maybe_user = MaybeUser(Some(auth.user));
    let mut ctx = build_ctx(&session, maybe_user, "/submit").await;
    ctx.no_index = true;
    Ok(Html(templates::submit::render(&ctx, None).into_string()))
}

pub async fn do_submit(
    State(state): State<AppState>,
    session: Session,
    auth: RequireAuthUser,
    mut multipart: Multipart,
) -> AppResult<Response> {
    let bearer_authenticated = auth.source == AuthSource::Bearer;
    let user = auth.user;
    let maybe_user = MaybeUser(Some(user.clone()));
    let mut fields: SubmitFields = SubmitFields::default();
    // Buffer either the PDF OR the LaTeX-source upload in memory first;
    // only write to disk AFTER CSRF + field validation pass, so a forged
    // multipart POST can't fill the upload directory. The pdf buffer
    // also gets a magic-byte sanity check; the source buffer is left
    // raw because zip / tar.gz / single-.tex have wildly different
    // signatures (compile module re-detects).
    let mut pdf_buf: Option<(String, axum::body::Bytes)> = None;
    let mut source_buf: Option<(String, axum::body::Bytes)> = None;

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
            if !data.starts_with(b"%PDF-") {
                return Ok(err_page(
                    &session,
                    maybe_user,
                    "Uploaded file is not a valid PDF (missing %PDF header).",
                )
                .await);
            }
            pdf_buf = Some((safe, data));
        } else if name == "source" {
            // LaTeX-source upload (.tex / .zip / .tar.gz). We don't
            // magic-byte-check here — the compile module re-detects.
            let file_name = field.file_name().unwrap_or("source.tex").to_string();
            let safe = sanitize_filename(&file_name);
            let data = field
                .bytes()
                .await
                .map_err(|e| crate::error::AppError::Other(anyhow::anyhow!("multipart: {e}")))?;
            if data.is_empty() {
                continue;
            }
            if data.len() > 30 * 1024 * 1024 {
                return Ok(err_page(&session, maybe_user, "Source upload exceeds 30 MB.").await);
            }
            source_buf = Some((safe, data));
        } else {
            let value = field.text().await.unwrap_or_default();
            fields.set(&name, value);
        }
    }

    if !bearer_authenticated && !verify_csrf(&session, &fields.csrf_token).await {
        return Ok(err_page(&session, maybe_user, "Form expired — please try again.").await);
    }
    if !user.is_verified_or_admin() {
        return Ok(err_page(
            &session, maybe_user,
            "Your email isn't verified yet. Check your inbox for the verification link, or go to /me/edit to resend it. Submission is gated on verification to deter spam.",
        ).await);
    }
    if fields.title.trim().is_empty() || fields.r#abstract.trim().len() < 100 {
        return Ok(err_page(
            &session,
            maybe_user,
            "Title required; abstract must be at least 100 chars.",
        )
        .await);
    }
    if fields.authors.trim().is_empty() {
        return Ok(err_page(&session, maybe_user, "At least one author required.").await);
    }
    // Allow multiple AI model disclosures — the form joins them with commas in
    // a hidden input. Normalize to a clean, dedup'd, comma+space-joined
    // string for storage; require at least one non-empty entry.
    let ai_models_joined =
        crate::models::manuscript::normalize_ai_models(&fields.conductor_ai_model);
    if ai_models_joined.is_empty() {
        return Ok(err_page(
            &session,
            maybe_user,
            "At least one AI model is required. Type the model name and press Enter / comma.",
        )
        .await);
    }
    if fields.conductor_type == "human-ai" && fields.conductor_human.trim().is_empty() {
        return Ok(err_page(
            &session,
            maybe_user,
            "Human conductor name required for Human + AI submissions.",
        )
        .await);
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
    let (
        has_auditor,
        auditor_name,
        auditor_affiliation,
        auditor_role,
        auditor_statement,
        auditor_orcid,
    ) = match fields.audit_status.as_str() {
        "self" if fields.conductor_type == "human-ai" => {
            if fields.self_audit_statement.trim().is_empty() {
                return Ok(err_page(&session, maybe_user, "Self-audit statement is required when you tick 'I am the auditor'. Say what you actually verified.").await);
            }
            (
                true,
                Some(fields.conductor_human.trim().to_string()),
                None,
                if fields.conductor_role.trim().is_empty() {
                    None
                } else {
                    Some(fields.conductor_role.trim().to_string())
                },
                Some(fields.self_audit_statement.trim().to_string()),
                None,
            )
        }
        "other" => {
            if fields.auditor_name.trim().is_empty() {
                return Ok(err_page(
                    &session,
                    maybe_user,
                    "Auditor name is required when you select 'Someone else audited this'.",
                )
                .await);
            }
            if fields.auditor_statement.trim().is_empty() {
                return Ok(err_page(
                    &session,
                    maybe_user,
                    "Auditor statement is required when you select 'Someone else audited this'.",
                )
                .await);
            }
            (
                true,
                Some(fields.auditor_name.trim().to_string()),
                if fields.auditor_affiliation.trim().is_empty() {
                    None
                } else {
                    Some(fields.auditor_affiliation.trim().to_string())
                },
                if fields.auditor_role.trim().is_empty() {
                    None
                } else {
                    Some(fields.auditor_role.trim().to_string())
                },
                Some(fields.auditor_statement.trim().to_string()),
                if fields.auditor_orcid.trim().is_empty() {
                    None
                } else {
                    Some(fields.auditor_orcid.trim().to_string())
                },
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
        return Ok(err_page(
            &session,
            maybe_user,
            "Unknown reader license. Pick from the dropdown.",
        )
        .await);
    };
    let ai_training = if fields.ai_training.trim().is_empty() {
        "allow"
    } else if crate::licenses::ai_training_lookup(fields.ai_training.trim()).is_some() {
        fields.ai_training.trim()
    } else {
        return Ok(err_page(
            &session,
            maybe_user,
            "Unknown AI-training option. Pick from the dropdown.",
        )
        .await);
    };

    // id + doi are allocated just before the INSERT (inside the retry
    // loop below) so a UNIQUE collision can re-allocate without
    // throwing away the validated form payload.

    // Source-type gate. The form has three branches; enforce the
    // required artefact for each.
    let source_type = if fields.source_type.is_empty() {
        "tex".to_string()
    } else {
        fields.source_type.clone()
    };
    match source_type.as_str() {
        "tex" => {
            if source_buf.is_none() {
                return Ok(err_page(&session, maybe_user,
                    "LaTeX source upload is required. Choose 'PDF directly' if you don't have the .tex. External URL is only a supplemental link.").await);
            }
        }
        "pdf" => {
            if !fields.conductor_ai_model_public
                || (fields.conductor_type == "human-ai" && !fields.conductor_human_public)
            {
                return Ok(err_page(&session, maybe_user,
                    "Private conductor/model fields require a LaTeX source upload so PreXiv can black out the public source and compiled PDF.").await);
            }
            if pdf_buf.is_none() {
                return Ok(err_page(
                    &session,
                    maybe_user,
                    "PDF upload is required for the 'PDF directly' option.",
                )
                .await);
            }
        }
        "url" => return Ok(err_page(
            &session,
            maybe_user,
            "External URL-only submissions are no longer supported. Upload a LaTeX source or PDF so PreXiv can host the paper; use External URL only as a supplemental link.",
        ).await),
        other => {
            return Ok(err_page(
                &session,
                maybe_user,
                &format!("Unknown source_type '{other}'."),
            )
            .await);
        }
    }

    let review_confirmed =
        fields.responsibility_ack && fields.artifact_ack && fields.provenance_ack;
    if !(bearer_authenticated || review_confirmed) {
        return Ok(err_page(
            &session,
            maybe_user,
            "Before submitting, confirm responsibility, hosted-artifact, and provenance/audit disclosure in the Review & submit section.",
        )
        .await);
    }

    // All validation passed → now (and only now) persist files and (for
    // tex) run the compiler.
    let upload_dir = upload_dir();
    fs::create_dir_all(&upload_dir)
        .await
        .map_err(|e| crate::error::AppError::Other(e.into()))?;
    let stamp = chrono::Utc::now().timestamp_millis();
    let rnd: u32 = rand::thread_rng().gen_range(100_000..1_000_000);
    let arxiv_like_id = make_prexiv_id();
    let synthetic_doi = format!("10.99999/{}", arxiv_like_id);
    let app_url = state.app_url.as_deref().unwrap_or("http://localhost:3001");

    let mut pdf_path: Option<String> = None;
    let mut source_path: Option<String> = None;

    if source_type == "pdf" {
        let Some((safe, data)) = &pdf_buf else {
            return Ok(err_page(
                &session,
                maybe_user,
                "PDF upload is required for the 'PDF directly' option.",
            )
            .await);
        };
        let watermarked = match crate::pdf_watermark::watermark_pdf(
            data,
            &arxiv_like_id,
            fields.category.trim(),
            app_url,
        )
        .await
        {
            Ok(pdf) => pdf,
            Err(e) => {
                return Ok(err_page(
                    &session,
                    maybe_user,
                    &format!("PDF watermarking failed: {e}"),
                )
                .await);
            }
        };
        // Direct-PDF path: no compilation. Persist only the public,
        // watermarked PDF; the original upload is never written to disk.
        let stored = format!("{stamp}-{rnd}-{safe}");
        let full = upload_dir.join(&stored);
        let mut f = fs::File::create(&full)
            .await
            .map_err(|e| crate::error::AppError::Other(e.into()))?;
        f.write_all(&watermarked)
            .await
            .map_err(|e| crate::error::AppError::Other(e.into()))?;
        pdf_path = Some(stored);
    }

    if source_type == "tex" {
        let Some((safe, data)) = &source_buf else {
            return Ok(err_page(&session, maybe_user, "LaTeX source upload is required.").await);
        };
        let redaction = crate::compile::RedactionOptions {
            hide_human: fields.conductor_type == "human-ai" && !fields.conductor_human_public,
            hide_ai_model: !fields.conductor_ai_model_public,
            human_name: opt(&fields.conductor_human).map(str::to_string),
            ai_models: ai_models_joined
                .split(',')
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .map(str::to_string)
                .collect(),
        };
        let prepared = match crate::compile::prepare_source(safe, data, &redaction) {
            Ok(prepared) => prepared,
            Err(e) => {
                return Ok(err_page(
                    &session,
                    maybe_user,
                    &format!("LaTeX source preparation failed: {e}"),
                )
                .await);
            }
        };

        // Persist only the source artifact that will be offered publicly.
        // For private conductor/model fields, this is the blacked-out source.
        let stored_src = format!(
            "{stamp}-{rnd}-src-{}",
            sanitize_filename(&prepared.filename)
        );
        let full_src = upload_dir.join(&stored_src);
        let mut f = fs::File::create(&full_src)
            .await
            .map_err(|e| crate::error::AppError::Other(e.into()))?;
        f.write_all(&prepared.data)
            .await
            .map_err(|e| crate::error::AppError::Other(e.into()))?;
        source_path = Some(stored_src);

        // Compile.
        match crate::compile::compile(&prepared.filename, &prepared.data).await {
            Ok(compiled) => {
                let watermarked = match crate::pdf_watermark::watermark_pdf(
                    &compiled.pdf,
                    &arxiv_like_id,
                    fields.category.trim(),
                    app_url,
                )
                .await
                {
                    Ok(pdf) => pdf,
                    Err(e) => {
                        let _ =
                            fs::remove_file(upload_dir.join(source_path.as_deref().unwrap_or("")))
                                .await;
                        return Ok(err_page(
                            &session,
                            maybe_user,
                            &format!("PDF watermarking failed: {e}"),
                        )
                        .await);
                    }
                };
                let pdf_name = format!("{stamp}-{rnd}-compiled.pdf");
                let pdf_full = upload_dir.join(&pdf_name);
                let mut pf = fs::File::create(&pdf_full)
                    .await
                    .map_err(|e| crate::error::AppError::Other(e.into()))?;
                pf.write_all(&watermarked)
                    .await
                    .map_err(|e| crate::error::AppError::Other(e.into()))?;
                pdf_path = Some(pdf_name);
            }
            Err(e) => {
                // Compile failed. Surface the error + the LaTeX log tail
                // to the user; drop the source we just wrote (no manuscript
                // row gets created).
                let _ =
                    fs::remove_file(upload_dir.join(source_path.as_deref().unwrap_or(""))).await;
                let log_excerpt = e.log().map(|s| s.to_string());
                let msg = match log_excerpt {
                    Some(log) => format!(
                        "LaTeX compile failed: {e}\n\nLast lines of the compile log:\n\n{log}"
                    ),
                    None => format!("LaTeX compile failed: {e}"),
                };
                return Ok(err_page(&session, maybe_user, &msg).await);
            }
        }
    }

    // The PDF watermark contains the PreXiv id, so allocation happens before
    // file persistence. Collisions are vanishingly rare with the 30-bit daily
    // suffix; if one still happens, ask the user to retry rather than storing
    // a PDF stamped with a different id.
    let result = sqlx::query(
        r#"INSERT INTO manuscripts (
                arxiv_like_id, doi, submitter_id, title, abstract, authors, category,
                pdf_path, external_url, source_path,
                conductor_type, conductor_ai_model, conductor_ai_model_public,
                conductor_human, conductor_human_public, conductor_role, conductor_notes,
                agent_framework,
                has_auditor, auditor_name, auditor_affiliation, auditor_role,
                auditor_statement, auditor_orcid,
                license, ai_training
            ) VALUES (
                ?, ?, ?, ?, ?, ?, ?,
                ?, ?, ?,
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
    .bind(source_path.as_deref())
    .bind(&fields.conductor_type)
    .bind(&ai_models_joined)
    .bind(if fields.conductor_ai_model_public {
        1i64
    } else {
        0
    })
    .bind(opt(&fields.conductor_human))
    .bind(if fields.conductor_human_public {
        1i64
    } else {
        0
    })
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
    .await;
    let result = match result {
        Ok(rr) => rr,
        Err(e) if is_unique_violation(&e) => {
            cleanup_uploads(&upload_dir, pdf_path.as_deref(), source_path.as_deref()).await;
            return Err(crate::error::AppError::Other(anyhow::anyhow!(
                "could not allocate a unique prexiv id; please retry"
            )));
        }
        Err(e) => {
            cleanup_uploads(&upload_dir, pdf_path.as_deref(), source_path.as_deref()).await;
            return Err(e.into());
        }
    };

    let new_id = result.last_insert_rowid();
    // Record v1 in manuscript_versions so the version log is complete
    // from the moment of original submission.
    let v1 = crate::versions::VersionInput {
        title: fields.title.trim(),
        r#abstract: fields.r#abstract.trim(),
        authors: fields.authors.trim(),
        category: fields.category.trim(),
        pdf_path: pdf_path.as_deref(),
        external_url: opt(&fields.external_url),
        conductor_notes: opt(&fields.conductor_notes),
        license,
        ai_training,
        revision_note: None,
    };
    let _ = crate::versions::insert_initial(&state.pool, new_id, &v1).await;

    set_flash(
        &session,
        format!("Manuscript submitted as {arxiv_like_id}."),
    )
    .await;
    Ok(Redirect::to(&format!("/abs/{arxiv_like_id}")).into_response())
}

fn opt(s: &str) -> Option<&str> {
    let t = s.trim();
    if t.is_empty() {
        None
    } else {
        Some(t)
    }
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

/// Allocate a fresh `prexiv:YYMMDD.xxxxxx` id.
///
/// Format breakdown:
///   - `YYMMDD` — UTC year/month/day, two digits each.
///   - `xxxxxx` — 6-character lowercase Crockford base-32 random suffix.
///
/// Two design properties:
///
/// 1. **Day-resolution chronological ordering by id alone.** Two ids
///    submitted on different days lex-sort the same way they
///    chronologically sort, because `YYMMDD` already does. Within
///    the same day the suffix is random rather than monotonic, so
///    two same-day ids aren't strictly ordered by time — but at the
///    granularity that matters for a preprint server (which day did
///    this drop?), the id alone is enough.
///
/// 2. **Headroom for agent-driven load.** 32^6 ≈ 1.07 × 10^9 suffixes
///    per day. Birthday-paradox 50 % collision probability sits at
///    ~32 700 same-day submissions — orders of magnitude above any
///    realistic rate. The DB insert still enforces uniqueness; if the
///    direct-submit path hits the unlucky collision, the user can retry and
///    receive a fresh id.
///
/// Retired legacy ids stay valid via the `prexiv_id_aliases` table;
/// new manuscripts always receive the `prexiv:YYMMDD.xxxxxx` form.
fn make_prexiv_id() -> String {
    let now = chrono::Utc::now();
    let yymmdd = format!("{:02}{:02}{:02}", now.year() % 100, now.month(), now.day());
    // 30 bits of randomness — fits in u32, encodes in exactly 6
    // Crockford-32 chars without any digit hitting modular wrap.
    let suffix_n: u32 = rand::thread_rng().gen_range(0..(1u32 << 30));
    format!(
        "prexiv:{yymmdd}.{}",
        crate::crockford::encode(suffix_n as u64, 6)
    )
}

/// Sqlx error -> "is this a UNIQUE-constraint violation?" predicate.
/// SQLite returns extended code 2067 (SQLITE_CONSTRAINT_UNIQUE).
fn is_unique_violation(e: &sqlx::Error) -> bool {
    if let sqlx::Error::Database(db) = e {
        if db.code().as_deref() == Some("2067") {
            return true;
        }
        let m = db.message().to_ascii_lowercase();
        return m.contains("unique constraint") || m.contains("constraint failed");
    }
    false
}

async fn cleanup_uploads(
    upload_dir: &std::path::Path,
    pdf_path: Option<&str>,
    source_path: Option<&str>,
) {
    if let Some(path) = pdf_path {
        let _ = fs::remove_file(upload_dir.join(path)).await;
    }
    if let Some(path) = source_path {
        let _ = fs::remove_file(upload_dir.join(path)).await;
    }
}

async fn err_page(session: &Session, maybe_user: MaybeUser, msg: &str) -> Response {
    let mut ctx = build_ctx(session, maybe_user, "/submit").await;
    ctx.no_index = true;
    Html(templates::submit::render(&ctx, Some(msg)).into_string()).into_response()
}

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
    /// "tex" / "pdf"
    source_type: String,
    responsibility_ack: bool,
    artifact_ack: bool,
    provenance_ack: bool,
}

impl Default for SubmitFields {
    fn default() -> Self {
        Self {
            csrf_token: String::new(),
            title: String::new(),
            r#abstract: String::new(),
            authors: String::new(),
            category: String::new(),
            external_url: String::new(),
            conductor_type: String::new(),
            conductor_ai_model: String::new(),
            conductor_ai_model_public: true,
            conductor_human: String::new(),
            conductor_human_public: true,
            conductor_role: String::new(),
            conductor_notes: String::new(),
            agent_framework: String::new(),
            audit_status: String::new(),
            self_audit_statement: String::new(),
            auditor_name: String::new(),
            auditor_affiliation: String::new(),
            auditor_role: String::new(),
            auditor_statement: String::new(),
            auditor_orcid: String::new(),
            license: String::new(),
            ai_training: String::new(),
            source_type: String::new(),
            responsibility_ack: false,
            artifact_ack: false,
            provenance_ack: false,
        }
    }
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
            "source_type" => self.source_type = v,
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
            "responsibility_ack" => self.responsibility_ack = is_truthy(&v),
            "artifact_ack" => self.artifact_ack = is_truthy(&v),
            "provenance_ack" => self.provenance_ack = is_truthy(&v),
            _ => {}
        }
    }
}

fn is_truthy(s: &str) -> bool {
    matches!(s, "1" | "on" | "true" | "yes")
}
