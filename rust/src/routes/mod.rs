use axum::routing::{get, post};
use axum::Router;

use crate::state::AppState;

pub mod admin;
pub mod api;
pub mod auth;
pub mod cite;
pub mod comments;
pub mod home;
pub mod listings;
pub mod manuscript;
pub mod me;
pub mod me_tokens;
pub mod pages;
pub mod profile;
pub mod search;
pub mod static_routes;
pub mod submit;
pub mod votes;

pub fn router() -> Router<AppState> {
    Router::new()
        // Home + listings
        .route("/", get(home::index))
        .route("/new", get(listings::new_listing))
        .route("/top", get(listings::top_listing))
        .route("/audited", get(listings::audited_listing))
        .route("/browse", get(listings::browse_index))
        .route("/browse/{cat}", get(listings::browse_category))

        // Manuscript
        .route("/m/{id}", get(manuscript::view))
        .route("/m/{id}/comment", post(comments::post_comment))
        .route("/m/{id}/cite", get(cite::cite))

        // Profile
        .route("/u/{username}", get(profile::show))

        // Search
        .route("/search", get(search::search))

        // Auth
        .route("/login", get(auth::show_login).post(auth::do_login))
        .route("/register", get(auth::show_register).post(auth::do_register))
        .route("/logout", post(auth::do_logout))

        // Submit + write actions
        .route("/submit", get(submit::show_submit).post(submit::do_submit))
        .route("/vote", post(votes::vote))

        // /me/tokens (real impl) and other /me/* (stubs for now)
        .route("/me/tokens", get(me_tokens::show).post(me_tokens::create))
        .route("/me/tokens/{id}/revoke", post(me_tokens::revoke))
        .route("/me/edit", get(me::me_edit))
        .route("/feed", get(me::feed))

        // Admin
        .route("/admin", get(admin::queue))
        .route("/admin/flag/{id}/resolve", post(admin::resolve))
        .route("/admin/audit", get(admin::audit))

        // Agent-native JSON API
        .nest("/api/v1", api::router())

        // Static content pages
        .route("/about", get(pages::about))
        .route("/guidelines", get(pages::guidelines))
        .route("/tos", get(pages::tos))
        .route("/privacy", get(pages::privacy))
        .route("/dmca", get(pages::dmca))
        .route("/policies", get(pages::policies))

        // Crawler policy
        .route("/robots.txt", get(static_routes::robots_txt))
}
