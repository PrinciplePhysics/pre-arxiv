use axum::extract::{Query, State};
use axum::response::Html;
use serde::Deserialize;

use crate::error::AppResult;
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
    Query(params): Query<SearchParams>,
) -> AppResult<Html<String>> {
    let q = params.q.trim();
    if q.is_empty() {
        return Ok(Html(templates::search::render(q, &[]).into_string()));
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

    Ok(Html(templates::search::render(q, &rows).into_string()))
}

/// Tokenize the user query into a safe FTS5 MATCH expression. Each
/// alphanumeric run becomes a prefix match (`token*`); everything else is
/// dropped. This avoids FTS5 syntax errors from `:` `"` `(` etc. in user input.
fn build_fts_query(q: &str) -> String {
    let tokens: Vec<String> = q
        .split(|c: char| !c.is_alphanumeric())
        .filter(|t| !t.is_empty())
        .map(|t| format!("{}*", t))
        .collect();
    if tokens.is_empty() {
        // Force-empty-result query that's still valid FTS5 syntax.
        return "zzzznonexistentzzz".to_string();
    }
    tokens.join(" ")
}
