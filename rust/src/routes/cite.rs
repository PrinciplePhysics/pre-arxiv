use axum::extract::{Path, State};
use axum::http::header;
use axum::response::{Html, IntoResponse, Response};
use tower_sessions::Session;

use crate::auth::MaybeUser;
use crate::error::{AppError, AppResult};
use crate::helpers::build_ctx;
use crate::models::Manuscript;
use crate::state::AppState;
use crate::templates;

pub async fn cite(
    State(state): State<AppState>,
    session: Session,
    maybe_user: MaybeUser,
    Path(id): Path<String>,
) -> AppResult<Html<String>> {
    let m = load_manuscript(&state, &id).await?;
    let ctx = build_ctx(&session, maybe_user, "/m").await;
    let base_url = state.app_url.as_deref().unwrap_or("http://localhost:3001");
    Ok(Html(
        templates::cite::render(&ctx, &m, base_url).into_string(),
    ))
}

pub async fn bib(State(state): State<AppState>, Path(id): Path<String>) -> AppResult<Response> {
    let m = load_manuscript(&state, &id).await?;
    let base_url = state.app_url.as_deref().unwrap_or("http://localhost:3001");
    Ok((
        [
            (header::CONTENT_TYPE, "application/x-bibtex; charset=utf-8"),
            (
                header::CONTENT_DISPOSITION,
                "inline; filename=\"prexiv-citation.bib\"",
            ),
        ],
        templates::cite::bibtex(&m, base_url),
    )
        .into_response())
}

pub async fn ris(State(state): State<AppState>, Path(id): Path<String>) -> AppResult<Response> {
    let m = load_manuscript(&state, &id).await?;
    let base_url = state.app_url.as_deref().unwrap_or("http://localhost:3001");
    Ok((
        [
            (
                header::CONTENT_TYPE,
                "application/x-research-info-systems; charset=utf-8",
            ),
            (
                header::CONTENT_DISPOSITION,
                "inline; filename=\"prexiv-citation.ris\"",
            ),
        ],
        templates::cite::ris(&m, base_url),
    )
        .into_response())
}

async fn load_manuscript(state: &AppState, id: &str) -> AppResult<Manuscript> {
    let m: Option<Manuscript> = sqlx::query_as::<_, Manuscript>(crate::db::pg(
        r#"
        SELECT id, arxiv_like_id, doi, submitter_id, title, abstract, authors, category,
               pdf_path, external_url,
               conductor_type, conductor_ai_model, conductor_ai_model_public,
               conductor_human, conductor_human_public, conductor_role, conductor_notes,
               agent_framework,
               has_auditor, auditor_name, auditor_affiliation, auditor_role,
               auditor_statement, auditor_orcid,
               view_count, score, comment_count,
               withdrawn, withdrawn_reason, withdrawn_at,
               created_at, updated_at
        FROM manuscripts
        WHERE arxiv_like_id = ? OR CAST(id AS TEXT) = ?
        LIMIT 1
        "#,
    ))
    .bind(id)
    .bind(id)
    .fetch_optional(&state.pool)
    .await?;
    m.ok_or(AppError::NotFound)
}
