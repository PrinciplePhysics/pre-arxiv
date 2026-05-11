use axum::extract::{Path, State};
use axum::response::Html;
use tower_sessions::Session;

use crate::auth::MaybeUser;
use crate::error::{AppError, AppResult};
use crate::helpers::build_ctx;
use crate::models::comment::CommentWithAuthor;
use crate::models::Manuscript;
use crate::state::AppState;
use crate::templates;

pub async fn view(
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

    let comments: Vec<CommentWithAuthor> = sqlx::query_as::<_, CommentWithAuthor>(
        r#"
        SELECT c.id, c.manuscript_id, c.author_id,
               u.username AS author_username,
               c.parent_id, c.content, c.score, c.created_at
        FROM comments c
        JOIN users u ON u.id = c.author_id
        WHERE c.manuscript_id = ?
        ORDER BY c.created_at ASC
        "#,
    )
    .bind(m.id)
    .fetch_all(&state.pool)
    .await?;

    let submitter: Option<(String, Option<String>)> = sqlx::query_as(
        "SELECT username, display_name FROM users WHERE id = ?",
    )
    .bind(m.submitter_id)
    .fetch_optional(&state.pool)
    .await?;

    // Category counts for the sidebar "Subject Areas" index — same shape
    // as bioRxiv's category sidebar.
    let cats: Vec<(String, i64)> = sqlx::query_as::<_, (String, i64)>(
        "SELECT category, COUNT(*) FROM manuscripts WHERE withdrawn = 0 GROUP BY category ORDER BY category"
    )
    .fetch_all(&state.pool)
    .await?;

    sqlx::query("UPDATE manuscripts SET view_count = COALESCE(view_count, 0) + 1 WHERE id = ?")
        .bind(m.id)
        .execute(&state.pool)
        .await
        .ok();

    let ctx = build_ctx(&session, maybe_user, "/m").await;
    Ok(Html(templates::manuscript::render(&ctx, &m, &comments, submitter.as_ref(), &cats).into_string()))
}
