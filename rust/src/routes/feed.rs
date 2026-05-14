//! /feed — authenticated social inbox: manuscripts from users you follow.

use axum::extract::{Query, State};
use axum::response::Html;
use serde::Deserialize;
use tower_sessions::Session;

use crate::auth::{MaybeUser, RequireUser};
use crate::error::AppResult;
use crate::helpers::build_ctx;
use crate::models::ManuscriptListItem;
use crate::state::AppState;
use crate::templates;

#[derive(Deserialize)]
pub struct FeedQuery {
    #[serde(default)]
    pub page: Option<i64>,
}

pub async fn show(
    State(state): State<AppState>,
    session: Session,
    maybe_user: MaybeUser,
    RequireUser(user): RequireUser,
    Query(q): Query<FeedQuery>,
) -> AppResult<Html<String>> {
    let per: i64 = 30;
    let page = q.page.unwrap_or(1).max(1);
    let offset = (page - 1) * per;

    let following: (i64,) = sqlx::query_as(crate::db::pg(
        "SELECT COUNT(*) FROM follows WHERE follower_id = ?",
    ))
    .bind(user.id)
    .fetch_one(&state.pool)
    .await?;

    let rows: Vec<ManuscriptListItem> = sqlx::query_as::<_, ManuscriptListItem>(crate::db::pg(
        r#"SELECT m.id, m.arxiv_like_id, m.doi, m.title, m.authors, m.category,
                  m.conductor_type, m.conductor_ai_model, m.conductor_ai_model_public,
                  m.conductor_human, m.conductor_human_public,
                  m.has_auditor, m.auditor_name,
                  m.score, m.comment_count, m.withdrawn, m.created_at
           FROM manuscripts m
           JOIN follows f ON f.followee_id = m.submitter_id
           WHERE f.follower_id = ?
           ORDER BY m.created_at DESC LIMIT ? OFFSET ?"#,
    ))
    .bind(user.id)
    .bind(per)
    .bind(offset)
    .fetch_all(&state.pool)
    .await?;

    let mut ctx = build_ctx(&session, maybe_user, "/feed").await;
    ctx.no_index = true;
    Ok(Html(
        templates::feed::render(&ctx, &rows, page, per, following.0).into_string(),
    ))
}
