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

pub struct ProfileStats {
    pub follower_count: i64,
    pub following_count: i64,
    pub viewer_follows: bool,
}

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
                  has_auditor, auditor_name,
                  score, comment_count, withdrawn, created_at
           FROM manuscripts WHERE submitter_id = ? ORDER BY created_at DESC LIMIT 50"#,
    )
    .bind(u.id)
    .fetch_all(&state.pool)
    .await?;

    let (follower_count,): (i64,) = sqlx::query_as("SELECT COUNT(*) FROM follows WHERE followee_id = ?")
        .bind(u.id).fetch_one(&state.pool).await?;
    let (following_count,): (i64,) = sqlx::query_as("SELECT COUNT(*) FROM follows WHERE follower_id = ?")
        .bind(u.id).fetch_one(&state.pool).await?;
    let viewer_follows = match &maybe_user.0 {
        Some(viewer) if viewer.id != u.id => {
            let (c,): (i64,) = sqlx::query_as(
                "SELECT COUNT(*) FROM follows WHERE follower_id = ? AND followee_id = ?",
            )
            .bind(viewer.id).bind(u.id)
            .fetch_one(&state.pool).await?;
            c > 0
        }
        _ => false,
    };

    let stats = ProfileStats { follower_count, following_count, viewer_follows };

    let ctx = build_ctx(&session, maybe_user, &format!("/u/{username}")).await;
    Ok(Html(templates::profile::render(&ctx, &u, &rows, &stats).into_string()))
}
