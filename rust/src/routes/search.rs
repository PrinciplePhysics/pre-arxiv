use axum::extract::{Query, State};
use axum::response::Html;
use serde::Deserialize;
use tower_sessions::Session;

use crate::auth::MaybeUser;
use crate::error::AppResult;
use crate::helpers::build_ctx;
use crate::models::ManuscriptListItem;
use crate::state::AppState;
use crate::templates;

#[derive(Debug, Deserialize)]
pub struct SearchParams {
    #[serde(default)]
    pub q: String,
}

pub async fn search(
    State(state): State<AppState>,
    session: Session,
    maybe_user: MaybeUser,
    Query(params): Query<SearchParams>,
) -> AppResult<Html<String>> {
    let q = params.q.trim();
    let ctx = build_ctx(&session, maybe_user, "/search").await;
    if q.is_empty() {
        return Ok(Html(templates::search::render(&ctx, q, &[]).into_string()));
    }

    let fts_query = build_fts_query(q);

    let rows: Vec<ManuscriptListItem> = sqlx::query_as::<_, ManuscriptListItem>(
        r#"
        SELECT m.id, m.arxiv_like_id, m.doi, m.title, m.authors, m.category,
               m.conductor_type, m.conductor_ai_model, m.conductor_ai_model_public,
               m.conductor_human, m.conductor_human_public,
               m.score, m.comment_count, m.withdrawn, m.created_at
        FROM manuscripts m
        JOIN manuscripts_fts f ON f.rowid = m.id
        WHERE manuscripts_fts MATCH ?
        ORDER BY rank
        LIMIT 100
        "#,
    )
    .bind(&fts_query)
    .fetch_all(&state.pool)
    .await?;

    Ok(Html(templates::search::render(&ctx, q, &rows).into_string()))
}

fn build_fts_query(q: &str) -> String {
    let tokens: Vec<String> = q
        .split(|c: char| !c.is_alphanumeric())
        .filter(|t| !t.is_empty())
        .map(|t| format!("{}*", t))
        .collect();
    if tokens.is_empty() {
        return "zzzznonexistentzzz".to_string();
    }
    tokens.join(" ")
}
