//! /m/{id}/diff/{a}/{b} — unified line-diff between two versions of a
//! manuscript. Uses the `similar` crate. Diffs every revisable field:
//! title, abstract, authors, category, license, ai_training,
//! external_url, conductor_notes. PDF path differences are reported as a
//! single equal/changed line; we don't try to diff the PDF bytes.

use axum::extract::{Path, State};
use axum::response::Html;
use tower_sessions::Session;

use crate::auth::MaybeUser;
use crate::error::{AppError, AppResult};
use crate::helpers::build_ctx;
use crate::models::Manuscript;
use crate::state::AppState;
use crate::templates;
use crate::versions;

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
                  license, ai_training, current_version, secondary_categories
           FROM manuscripts WHERE arxiv_like_id = ? OR CAST(id AS TEXT) = ?
           LIMIT 1"#,
    )
    .bind(id).bind(id)
    .fetch_optional(&state.pool)
    .await?;
    m.ok_or(AppError::NotFound)
}

pub async fn show(
    State(state): State<AppState>,
    session: Session,
    maybe_user: MaybeUser,
    Path((id, a, b)): Path<(String, i64, i64)>,
) -> AppResult<Html<String>> {
    let m = load_manuscript(&state, &id).await?;
    // Tolerate either order; the template renders the lower number on
    // the left ("before") and the higher on the right ("after").
    let (left_n, right_n) = if a <= b { (a, b) } else { (b, a) };

    let left = versions::get_version(&state.pool, m.id, left_n).await
        .map_err(|e| AppError::Other(anyhow::anyhow!("{e}")))?
        .ok_or(AppError::NotFound)?;
    let right = versions::get_version(&state.pool, m.id, right_n).await
        .map_err(|e| AppError::Other(anyhow::anyhow!("{e}")))?
        .ok_or(AppError::NotFound)?;

    let ctx = build_ctx(&session, maybe_user, "/m").await;
    Ok(Html(templates::versions_diff::render(&ctx, &m, &left, &right).into_string()))
}
