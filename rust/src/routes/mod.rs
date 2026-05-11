use axum::routing::get;
use axum::Router;

use crate::state::AppState;

pub mod home;
pub mod manuscript;
pub mod search;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/", get(home::index))
        .route("/m/{id}", get(manuscript::view))
        .route("/search", get(search::search))
}
