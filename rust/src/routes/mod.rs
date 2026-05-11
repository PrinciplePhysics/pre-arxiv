use axum::routing::{get, post};
use axum::Router;

use crate::state::AppState;

pub mod auth;
pub mod comments;
pub mod home;
pub mod manuscript;
pub mod search;
pub mod static_routes;
pub mod submit;
pub mod votes;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/", get(home::index))
        .route("/m/{id}", get(manuscript::view))
        .route("/m/{id}/comment", post(comments::post_comment))
        .route("/search", get(search::search))
        .route("/login", get(auth::show_login).post(auth::do_login))
        .route("/register", get(auth::show_register).post(auth::do_register))
        .route("/logout", post(auth::do_logout))
        .route("/submit", get(submit::show_submit).post(submit::do_submit))
        .route("/vote", post(votes::vote))
        .route("/robots.txt", get(static_routes::robots_txt))
}
