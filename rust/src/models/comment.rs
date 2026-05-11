use chrono::NaiveDateTime;
use serde::Serialize;
use sqlx::FromRow;

#[derive(Debug, Clone, FromRow, Serialize)]
pub struct Comment {
    pub id: i64,
    pub manuscript_id: i64,
    pub author_id: i64,
    pub parent_id: Option<i64>,
    pub content: String,
    pub score: Option<i64>,
    pub created_at: Option<NaiveDateTime>,
}

/// Comment joined with author username for rendering.
#[derive(Debug, Clone, FromRow, Serialize)]
pub struct CommentWithAuthor {
    pub id: i64,
    pub manuscript_id: i64,
    pub author_id: i64,
    pub author_username: String,
    pub parent_id: Option<i64>,
    pub content: String,
    pub score: Option<i64>,
    pub created_at: Option<NaiveDateTime>,
}
