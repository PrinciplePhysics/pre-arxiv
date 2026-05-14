use axum::extract::{Query, State};
use axum::response::Html;
use serde::Deserialize;
use tower_sessions::Session;

use crate::auth::MaybeUser;
use crate::categories::restricted_not_in_clause;
use crate::error::AppResult;
use crate::helpers::build_ctx_with_state;
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
    let want_filter = filters.show_all == 0;
    let cat_sql = if want_filter {
        let c = restricted_not_in_clause();
        if c.is_empty() {
            String::new()
        } else {
            format!(" AND {c}")
        }
    } else {
        String::new()
    };
    let author_sql = if want_filter {
        " AND submitter_id IN (
            SELECT id FROM users
             WHERE orcid_oauth_verified = 1
                OR (email_verified = 1 AND institutional_email = 1)
        )"
    } else {
        ""
    };
    let cols = r#"id, arxiv_like_id, doi, title, authors, category,
                conductor_type, conductor_ai_model, conductor_ai_model_public,
                conductor_human, conductor_human_public,
                has_auditor, auditor_name,
                score, comment_count, withdrawn, created_at"#;

    // "Ranked" — Hacker-News-style hot list. Rank value is
    //
    //   (score + 1) / (age_hours + 2)^2
    //
    //   * `score + 1` keeps newly-submitted, zero-vote work visible
    //     (instead of zeroing the numerator).
    //   * `age_hours + 2` is the gravity offset so the denominator
    //     doesn't approach 0 right after submission.
    //   * Gravity 2 — we expand the square inline as
    //     `(age_hours + 2) * (age_hours + 2)` so we don't need the
    //     database-specific exponent function. A power-of-2 decay is slightly more
    //     aggressive than HN's 1.8; for preprints — where freshness
    //     matters less than for news — this is still gentler than
    //     a strict chronological sort and reads well in practice.
    //
    // PostgreSQL's `EXTRACT(EPOCH FROM interval)` returns seconds; we
    // divide by 3600 to get hours. Withdrawn submissions are excluded;
    // the verified-scholar / restricted-category filters above still apply.
    let rank_order = r#"ORDER BY
        (CAST(COALESCE(score, 0) AS REAL) + 1.0)
        /
        ( ((EXTRACT(EPOCH FROM (CURRENT_TIMESTAMP - created_at)) / 3600.0) + 2.0)
        * ((EXTRACT(EPOCH FROM (CURRENT_TIMESTAMP - created_at)) / 3600.0) + 2.0) )
        DESC,
        created_at DESC"#;

    let sql = format!(
        "SELECT {cols} FROM manuscripts WHERE withdrawn = 0{cat_sql}{author_sql}
         {rank_order} LIMIT 50"
    );
    let mut rows: Vec<ManuscriptListItem> = sqlx::query_as::<_, ManuscriptListItem>(&sql)
        .fetch_all(&state.pool)
        .await?;

    // Cold-start fallback: if the verified-scholar filter produced no
    // rows, transparently widen to the unfiltered query and render a
    // banner explaining what happened. Only kicks in when the filter
    // was actually applied AND when it dropped everything — never
    // overrides an explicit `?show_all=1`.
    let widened = rows.is_empty() && want_filter;
    if widened {
        let fallback_sql =
            format!("SELECT {cols} FROM manuscripts WHERE withdrawn = 0 {rank_order} LIMIT 50");
        rows = sqlx::query_as::<_, ManuscriptListItem>(&fallback_sql)
            .fetch_all(&state.pool)
            .await?;
    }

    let ctx = build_ctx_with_state(&state, &session, maybe_user, "/").await;
    Ok(Html(
        templates::home::render(&ctx, &rows, widened, !want_filter).into_string(),
    ))
}
