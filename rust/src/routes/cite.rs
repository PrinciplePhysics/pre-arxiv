use axum::extract::{Path, State};
use axum::response::Html;
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
    let m: Option<Manuscript> = sqlx::query_as::<_, Manuscript>(
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
    )
    .bind(&id)
    .bind(&id)
    .fetch_optional(&state.pool)
    .await?;
    let m = m.ok_or(AppError::NotFound)?;
    let ctx = build_ctx(&session, maybe_user, "/m").await;
    Ok(Html(templates::cite::render(&ctx, &m).into_string()))
}
