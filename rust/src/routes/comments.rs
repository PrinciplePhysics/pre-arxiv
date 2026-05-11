use axum::extract::{Form, Path, State};
use axum::response::{IntoResponse, Redirect, Response};
use serde::Deserialize;
use tower_sessions::Session;

use crate::auth::{verify_csrf, RequireUser};
use crate::error::{AppError, AppResult};
use crate::helpers::set_flash;
use crate::state::AppState;

#[derive(Deserialize)]
pub struct CommentForm {
    pub csrf_token: String,
    pub content: String,
    #[serde(default)]
    pub parent_id: Option<i64>,
}

pub async fn post_comment(
    State(state): State<AppState>,
    session: Session,
    RequireUser(user): RequireUser,
    Path(id): Path<String>,
    Form(form): Form<CommentForm>,
) -> AppResult<Response> {
    if !verify_csrf(&session, &form.csrf_token).await {
        set_flash(&session, "Form expired — please try again.").await;
        return Ok(Redirect::to(&format!("/m/{id}")).into_response());
    }
    let content = form.content.trim();
    if content.is_empty() {
        set_flash(&session, "Comment cannot be empty.").await;
        return Ok(Redirect::to(&format!("/m/{id}")).into_response());
    }
    if content.len() > 8000 {
        set_flash(&session, "Comment too long (max 8000 chars).").await;
        return Ok(Redirect::to(&format!("/m/{id}")).into_response());
    }

    let m: Option<(i64, String)> = sqlx::query_as::<_, (i64, String)>(
        "SELECT id, COALESCE(arxiv_like_id, CAST(id AS TEXT)) FROM manuscripts WHERE arxiv_like_id = ? OR CAST(id AS TEXT) = ? LIMIT 1",
    )
    .bind(&id)
    .bind(&id)
    .fetch_optional(&state.pool)
    .await?;
    let (manuscript_id, slug) = m.ok_or(AppError::NotFound)?;

    let mut tx = state.pool.begin().await?;
    let res = sqlx::query(
        "INSERT INTO comments (manuscript_id, author_id, parent_id, content) VALUES (?, ?, ?, ?)",
    )
    .bind(manuscript_id)
    .bind(user.id)
    .bind(form.parent_id)
    .bind(content)
    .execute(&mut *tx)
    .await?;
    sqlx::query("UPDATE manuscripts SET comment_count = COALESCE(comment_count, 0) + 1 WHERE id = ?")
        .bind(manuscript_id)
        .execute(&mut *tx)
        .await?;
    tx.commit().await?;

    let cid = res.last_insert_rowid();
    Ok(Redirect::to(&format!("/m/{slug}#comment-{cid}")).into_response())
}
