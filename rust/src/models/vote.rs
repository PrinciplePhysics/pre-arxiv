use chrono::NaiveDateTime;
use serde::Serialize;
use sqlx::FromRow;

#[derive(Debug, Clone, FromRow, Serialize)]
pub struct Vote {
    pub id: i64,
    pub user_id: i64,
    pub target_type: String,
    pub target_id: i64,
    pub value: i64,
    pub created_at: Option<NaiveDateTime>,
}
