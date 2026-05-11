use axum::extract::State;
use axum::response::Html;
use tower_sessions::Session;

use crate::auth::MaybeUser;
use crate::error::AppResult;
use crate::helpers::build_ctx;
use crate::models::ManuscriptListItem;
use crate::state::AppState;
use crate::templates;

pub async fn index(
    State(state): State<AppState>,
    session: Session,
    maybe_user: MaybeUser,
) -> AppResult<Html<String>> {
    let rows: Vec<ManuscriptListItem> = sqlx::query_as::<_, ManuscriptListItem>(
        r#"
        SELECT id, arxiv_like_id, doi, title, authors, category,
               conductor_type, conductor_ai_model, conductor_ai_model_public,
               conductor_human, conductor_human_public,
               score, comment_count, withdrawn, created_at
        FROM manuscripts
        ORDER BY created_at DESC
        LIMIT 50
        "#,
    )
    .fetch_all(&state.pool)
    .await?;

    let ctx = build_ctx(&session, maybe_user, "/").await;
    Ok(Html(templates::home::render(&ctx, &rows).into_string()))
}
