//! Listing variants: /new, /top, /audited, /browse, /browse/{cat}.
//! All share the same template; only the SQL and the page heading differ.

use axum::extract::{Path, State};
use axum::response::Html;
use tower_sessions::Session;

use crate::auth::MaybeUser;
use crate::error::AppResult;
use crate::helpers::build_ctx;
use crate::models::ManuscriptListItem;
use crate::state::AppState;
use crate::templates;

const SLIM_COLS: &str = r#"id, arxiv_like_id, doi, title, authors, category,
    conductor_type, conductor_ai_model, conductor_ai_model_public,
    conductor_human, conductor_human_public,
    score, comment_count, withdrawn, created_at"#;

async fn fetch(pool: &sqlx::SqlitePool, sql: &str) -> Result<Vec<ManuscriptListItem>, sqlx::Error> {
    sqlx::query_as::<_, ManuscriptListItem>(sql).fetch_all(pool).await
}

pub async fn new_listing(
    State(state): State<AppState>,
    session: Session,
    maybe_user: MaybeUser,
) -> AppResult<Html<String>> {
    let sql = format!(
        "SELECT {SLIM_COLS} FROM manuscripts ORDER BY created_at DESC LIMIT 50"
    );
    let rows = fetch(&state.pool, &sql).await?;
    let ctx = build_ctx(&session, maybe_user, "/new").await;
    Ok(Html(templates::listing::render(&ctx, "Newest", "Most recent manuscripts.", &rows, "/new").into_string()))
}

pub async fn top_listing(
    State(state): State<AppState>,
    session: Session,
    maybe_user: MaybeUser,
) -> AppResult<Html<String>> {
    let sql = format!(
        "SELECT {SLIM_COLS} FROM manuscripts WHERE withdrawn = 0 ORDER BY score DESC, created_at DESC LIMIT 50"
    );
    let rows = fetch(&state.pool, &sql).await?;
    let ctx = build_ctx(&session, maybe_user, "/top").await;
    Ok(Html(templates::listing::render(&ctx, "Top", "Highest-scoring manuscripts.", &rows, "/top").into_string()))
}

pub async fn audited_listing(
    State(state): State<AppState>,
    session: Session,
    maybe_user: MaybeUser,
) -> AppResult<Html<String>> {
    let sql = format!(
        "SELECT {SLIM_COLS} FROM manuscripts WHERE has_auditor = 1 AND withdrawn = 0 ORDER BY created_at DESC LIMIT 50"
    );
    let rows = fetch(&state.pool, &sql).await?;
    let ctx = build_ctx(&session, maybe_user, "/audited").await;
    Ok(Html(templates::listing::render(
        &ctx,
        "Audited",
        "Only manuscripts with a named human auditor who has signed a correctness statement.",
        &rows,
        "/audited",
    )
    .into_string()))
}

pub async fn browse_index(
    State(state): State<AppState>,
    session: Session,
    maybe_user: MaybeUser,
) -> AppResult<Html<String>> {
    let counts: Vec<(String, i64)> = sqlx::query_as::<_, (String, i64)>(
        "SELECT category, COUNT(*) FROM manuscripts WHERE withdrawn = 0 GROUP BY category ORDER BY category"
    )
    .fetch_all(&state.pool)
    .await?;
    let ctx = build_ctx(&session, maybe_user, "/browse").await;
    Ok(Html(templates::listing::render_browse(&ctx, &counts).into_string()))
}

pub async fn browse_category(
    State(state): State<AppState>,
    session: Session,
    maybe_user: MaybeUser,
    Path(cat): Path<String>,
) -> AppResult<Html<String>> {
    let sql = format!(
        "SELECT {SLIM_COLS} FROM manuscripts WHERE category = ? ORDER BY created_at DESC LIMIT 50"
    );
    let rows: Vec<ManuscriptListItem> = sqlx::query_as::<_, ManuscriptListItem>(&sql)
        .bind(&cat)
        .fetch_all(&state.pool)
        .await?;
    let ctx = build_ctx(&session, maybe_user, "/browse").await;
    let heading = format!("Browse · {cat}");
    let sub = format!("All manuscripts categorized as {cat}, newest first.");
    Ok(Html(templates::listing::render(&ctx, &heading, &sub, &rows, &format!("/browse/{cat}")).into_string()))
}
