//! /m/{id}/versions — list every version of a manuscript with its
//! revision note and timestamp.
//!
//! /m/{id}/v/{n} — render a specific historical version. Same chrome
//! as the main detail page, but with a banner stating this is a
//! historical view and a link back to the latest.

use axum::extract::{Path, State};
use axum::response::{Html, IntoResponse, Response};
use tower_sessions::Session;

use crate::auth::MaybeUser;
use crate::error::{AppError, AppResult};
use crate::helpers::build_ctx;
use crate::models::Manuscript;
use crate::state::AppState;
use crate::templates;
use crate::versions;

async fn load_manuscript(state: &AppState, id: &str) -> AppResult<Manuscript> {
    let m: Option<Manuscript> = sqlx::query_as::<_, Manuscript>(crate::db::pg(
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
    ))
    .bind(id)
    .bind(id)
    .fetch_optional(&state.pool)
    .await?;
    m.ok_or(AppError::NotFound)
}

// ─── GET /m/{id}/versions ─────────────────────────────────────────────────

pub async fn list_versions(
    State(state): State<AppState>,
    session: Session,
    maybe_user: MaybeUser,
    Path(id): Path<String>,
) -> AppResult<Html<String>> {
    let m = load_manuscript(&state, &id).await?;
    let vs = versions::list_versions(&state.pool, m.id)
        .await
        .map_err(|e| AppError::Other(anyhow::anyhow!("{e}")))?;
    let ctx = build_ctx(&session, maybe_user, "/m").await;
    Ok(Html(
        templates::versions::render_list(&ctx, &m, &vs).into_string(),
    ))
}

// ─── GET /m/{id}/v/{n} ────────────────────────────────────────────────────

pub async fn show_version(
    State(state): State<AppState>,
    session: Session,
    maybe_user: MaybeUser,
    Path((id, n)): Path<(String, i64)>,
) -> AppResult<Response> {
    let m = load_manuscript(&state, &id).await?;
    if n == m.current_version {
        // Redirect to the canonical URL for the current version.
        let slug = m.arxiv_like_id.clone().unwrap_or_else(|| m.id.to_string());
        return Ok(axum::response::Redirect::to(&format!("/abs/{slug}")).into_response());
    }
    let v = versions::get_version(&state.pool, m.id, n)
        .await
        .map_err(|e| AppError::Other(anyhow::anyhow!("{e}")))?
        .ok_or(AppError::NotFound)?;
    let ctx = build_ctx(&session, maybe_user, "/m").await;
    Ok(Html(templates::versions::render_version(&ctx, &m, &v).into_string()).into_response())
}
