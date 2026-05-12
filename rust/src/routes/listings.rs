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
    has_auditor, auditor_name,
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

/// Helper exposed for the template — keeps the grouping logic out of maud.
pub fn browse_groups(counts: &[(String, i64)]) -> Vec<(&'static str, Vec<BrowseEntry>)> {
    use crate::categories;
    use std::collections::HashMap;

    // Build count map for O(1) lookup. Categories that aren't in our
    // canonical taxonomy (legacy data) bucket into "Other".
    let count_map: HashMap<&str, i64> =
        counts.iter().map(|(k, v)| (k.as_str(), *v)).collect();

    let mut groups: Vec<(&'static str, Vec<BrowseEntry>)> = Vec::new();
    for &g in categories::GROUPS {
        let mut entries: Vec<BrowseEntry> = categories::in_group(g)
            .map(|c| BrowseEntry {
                id: c.id,
                name: c.name,
                count: *count_map.get(c.id).unwrap_or(&0),
            })
            .collect();
        // Sort by count desc, then by id asc (stable).
        entries.sort_by(|a, b| b.count.cmp(&a.count).then(a.id.cmp(b.id)));
        groups.push((g, entries));
    }
    // Append any DB categories not in our taxonomy as a synthetic group.
    let canonical: std::collections::HashSet<&str> =
        categories::CATEGORIES.iter().map(|c| c.id).collect();
    let legacy: Vec<BrowseEntry> = counts
        .iter()
        .filter(|(k, _)| !canonical.contains(k.as_str()))
        .map(|(k, n)| BrowseEntry { id: leak(k.clone()), name: "(uncategorised in current taxonomy)", count: *n })
        .collect();
    if !legacy.is_empty() {
        groups.push(("Legacy ids", legacy));
    }
    groups
}

#[derive(Debug)]
pub struct BrowseEntry {
    pub id: &'static str,
    pub name: &'static str,
    pub count: i64,
}

fn leak(s: String) -> &'static str {
    // Tiny static-leak shim so legacy category ids from the DB can flow
    // through a Vec<BrowseEntry { id: &'static str }>. Only called once
    // per browse-index render for at-most-a-handful of legacy ids.
    Box::leak(s.into_boxed_str())
}

pub async fn browse_category(
    State(state): State<AppState>,
    session: Session,
    maybe_user: MaybeUser,
    Path(cat): Path<String>,
) -> AppResult<Html<String>> {
    // Primary OR cross-listed in `cat`. Cross-list match looks for
    // " <cat> " inside the whitespace-padded secondary_categories
    // string so we don't false-match `cs.L` against `cs.LG`.
    let pattern = format!("% {} %", cat);
    let sql = format!(
        "SELECT {SLIM_COLS} FROM manuscripts
         WHERE category = ?
            OR (' ' || COALESCE(secondary_categories, '') || ' ') LIKE ?
         ORDER BY created_at DESC LIMIT 50"
    );
    let rows: Vec<ManuscriptListItem> = sqlx::query_as::<_, ManuscriptListItem>(&sql)
        .bind(&cat)
        .bind(&pattern)
        .fetch_all(&state.pool)
        .await?;
    let ctx = build_ctx(&session, maybe_user, "/browse").await;
    let heading = format!("Browse · {cat}");
    let sub = format!("All manuscripts categorized as {cat}, newest first.");
    Ok(Html(templates::listing::render(&ctx, &heading, &sub, &rows, &format!("/browse/{cat}")).into_string()))
}
