//! /u/{username} — public profile page.

use axum::extract::{Path, State};
use axum::response::Html;
use tower_sessions::Session;

use crate::auth::MaybeUser;
use crate::error::{AppError, AppResult};
use crate::helpers::build_ctx;
use crate::models::{ManuscriptListItem, User};
use crate::state::AppState;
use crate::templates;

pub async fn show(
    State(state): State<AppState>,
    session: Session,
    maybe_user: MaybeUser,
    Path(username): Path<String>,
) -> AppResult<Html<String>> {
    let u: Option<User> = sqlx::query_as::<_, User>(
        r#"SELECT id, username, email, display_name, affiliation, bio,
                  karma, is_admin, email_verified, orcid, created_at
           FROM users WHERE username = ? LIMIT 1"#,
    )
    .bind(&username)
    .fetch_optional(&state.pool)
    .await?;
    let u = u.ok_or(AppError::NotFound)?;

    let rows: Vec<ManuscriptListItem> = sqlx::query_as::<_, ManuscriptListItem>(
        r#"SELECT id, arxiv_like_id, doi, title, authors, category,
                  conductor_type, conductor_ai_model, conductor_ai_model_public,
                  conductor_human, conductor_human_public,
                  score, comment_count, withdrawn, created_at
           FROM manuscripts WHERE submitter_id = ? ORDER BY created_at DESC LIMIT 50"#,
    )
    .bind(u.id)
    .fetch_all(&state.pool)
    .await?;

    let ctx = build_ctx(&session, maybe_user, &format!("/u/{username}")).await;
    Ok(Html(templates::profile::render(&ctx, &u, &rows).into_string()))
}
