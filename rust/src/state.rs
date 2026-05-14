use crate::db::DbPool;

#[derive(Clone)]
pub struct AppState {
    pub pool: DbPool,
    pub app_url: Option<String>,
}
