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

    let m: Option<(i64, String, i64)> = sqlx::query_as::<_, (i64, String, i64)>(
        "SELECT id, COALESCE(arxiv_like_id, CAST(id AS TEXT)), withdrawn FROM manuscripts WHERE arxiv_like_id = ? OR CAST(id AS TEXT) = ? LIMIT 1",
    )
    .bind(&id)
    .bind(&id)
    .fetch_optional(&state.pool)
    .await?;
    let (manuscript_id, slug, withdrawn) = m.ok_or(AppError::NotFound)?;

    // Reject comments on withdrawn manuscripts. The HTML hides the comment
    // form for withdrawn rows, but a hand-crafted POST would otherwise
    // succeed and leave new commentary attached to a tombstoned record.
    if withdrawn != 0 {
        set_flash(&session, "This manuscript has been withdrawn; new comments are disabled.").await;
        return Ok(Redirect::to(&format!("/m/{slug}")).into_response());
    }

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
    let cid = res.last_insert_rowid();

    // Look up the manuscript submitter and (if reply) the parent comment
    // author, so we can fire notifications. notify() short-circuits if
    // recipient == actor.
    let submitter: Option<(i64,)> = sqlx::query_as(
        "SELECT submitter_id FROM manuscripts WHERE id = ?",
    )
    .bind(manuscript_id)
    .fetch_optional(&mut *tx)
    .await?;
    let parent_author: Option<(i64,)> = match form.parent_id {
        Some(pid) => sqlx::query_as("SELECT author_id FROM comments WHERE id = ?")
            .bind(pid)
            .fetch_optional(&mut *tx)
            .await?,
        None => None,
    };
    tx.commit().await?;

    // Notifications fire on a clone of the pool outside the tx so a
    // DB hiccup here can't roll back the comment.
    let snippet: String = content.chars().take(140).collect();
    if let Some((sid,)) = submitter {
        let _ = crate::notifications::notify(
            &state.pool,
            sid,
            Some(user.id),
            crate::notifications::KIND_COMMENT_ON_MY_MANUSCRIPT,
            Some("comment"),
            Some(cid),
            Some(&snippet),
        ).await;
    }
    if let Some((pid_author,)) = parent_author {
        let _ = crate::notifications::notify(
            &state.pool,
            pid_author,
            Some(user.id),
            crate::notifications::KIND_REPLY_TO_MY_COMMENT,
            Some("comment"),
            Some(cid),
            Some(&snippet),
        ).await;
    }

    Ok(Redirect::to(&format!("/m/{slug}#comment-{cid}")).into_response())
}
