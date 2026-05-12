use axum::extract::{Query, State};
use axum::response::Html;
use serde::Deserialize;
use tower_sessions::Session;

use crate::auth::MaybeUser;
use crate::categories::restricted_not_in_clause;
use crate::error::AppResult;
use crate::helpers::build_ctx;
use crate::models::ManuscriptListItem;
use crate::state::AppState;
use crate::templates;

#[derive(Default, Deserialize)]
pub struct HomeFilters {
    #[serde(default)]
    pub show_all: u8,
}

pub async fn index(
    State(state): State<AppState>,
    session: Session,
    maybe_user: MaybeUser,
    Query(filters): Query<HomeFilters>,
) -> AppResult<Html<String>> {
    let (filter_sql, author_sql) = if filters.show_all == 0 {
        let c = restricted_not_in_clause();
        let cat = if c.is_empty() { String::new() } else { format!(" AND {c}") };
        let author = " AND submitter_id IN (
            SELECT id FROM users
             WHERE orcid_verified = 1 OR institutional_email = 1
        )";
        (cat, author)
    } else {
        (String::new(), "")
    };
    let sql = format!(
        "SELECT id, arxiv_like_id, doi, title, authors, category,
                conductor_type, conductor_ai_model, conductor_ai_model_public,
                conductor_human, conductor_human_public,
                has_auditor, auditor_name,
                score, comment_count, withdrawn, created_at
         FROM manuscripts
         WHERE 1=1{filter_sql}{author_sql}
         ORDER BY created_at DESC
         LIMIT 50"
    );
    let rows: Vec<ManuscriptListItem> = sqlx::query_as::<_, ManuscriptListItem>(&sql)
        .fetch_all(&state.pool)
        .await?;

    let ctx = build_ctx(&session, maybe_user, "/").await;
    Ok(Html(templates::home::render(&ctx, &rows).into_string()))
}
