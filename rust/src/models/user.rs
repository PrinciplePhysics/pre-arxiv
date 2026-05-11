use chrono::NaiveDateTime;
use serde::Serialize;
use sqlx::FromRow;

#[derive(Debug, Clone, FromRow, Serialize)]
pub struct User {
    pub id: i64,
    pub username: String,
    pub email: String,
    pub display_name: Option<String>,
    pub affiliation: Option<String>,
    pub bio: Option<String>,
    pub karma: Option<i64>,
    pub is_admin: i64,
    pub email_verified: i64,
    pub orcid: Option<String>,
    pub created_at: Option<NaiveDateTime>,
}

impl User {
    pub fn is_admin(&self) -> bool {
        self.is_admin != 0
    }
    pub fn is_verified(&self) -> bool {
        self.email_verified != 0
    }
    pub fn display(&self) -> &str {
        self.display_name.as_deref().unwrap_or(&self.username)
    }
}
