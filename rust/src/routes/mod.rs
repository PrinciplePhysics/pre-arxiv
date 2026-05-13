use axum::extract::DefaultBodyLimit;
use axum::http::{StatusCode, Uri};
use axum::response::{Html, IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};

use crate::error::render_error_page;
use crate::state::AppState;

const UPLOAD_BODY_LIMIT: usize = 35 * 1024 * 1024;

/// Axum fallback for unmatched routes. HTML 404 for browser paths,
/// JSON 404 for `/api/*` so machine clients don't have to parse HTML
/// when they hit a bad path. The HTML branch matches AppError::NotFound.
pub async fn not_found_fallback(uri: Uri) -> Response {
    if uri.path().starts_with("/api/") {
        return (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({ "error": "not found" })),
        )
            .into_response();
    }
    (
        StatusCode::NOT_FOUND,
        Html(render_error_page(
            404,
            "Page not found",
            "The URL you followed doesn't match any route on PreXiv. It may have been renamed, or never existed.",
        )),
    )
        .into_response()
}

pub mod admin;
pub mod api;
pub mod auth;
pub mod cite;
pub mod comments;
pub mod feed;
pub mod feeds;
pub mod flags;
pub mod follow;
pub mod forgot;
pub mod home;
pub mod listings;
pub mod manuscript;
pub mod manuscript_versions;
pub mod revise;
pub mod versions_diff;
pub mod me;
pub mod me_edit;
pub mod me_email;
pub mod me_account;
pub mod me_password;
pub mod me_tokens;
pub mod notifications;
pub mod oai;
pub mod pages;
pub mod profile;
pub mod search;
pub mod static_routes;
pub mod submit;
pub mod two_factor;
pub mod verify;
pub mod votes;
pub mod withdraw;

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
        // /m/{id}/comment POST is in write_post_router() (rate-limited)
        .route("/m/{id}/cite", get(cite::cite))
        .route("/m/{id}/cite.bib", get(cite::bib))
        .route("/m/{id}/cite.ris", get(cite::ris))
        .route("/m/{id}/withdraw", post(withdraw::withdraw))
        .route(
            "/m/{id}/revise",
            get(revise::show)
                .post(revise::submit)
                .layer(DefaultBodyLimit::max(UPLOAD_BODY_LIMIT)),
        )
        .route("/m/{id}/versions", get(manuscript_versions::list_versions))
        .route("/m/{id}/v/{n}", get(manuscript_versions::show_version))
        .route("/m/{id}/diff/{a}/{b}", get(versions_diff::show))

        // Profile + follow
        .route("/u/{username}", get(profile::show))
        .route("/u/{username}/follow", post(follow::follow))
        .route("/u/{username}/unfollow", post(follow::unfollow))

        // Search
        .route("/search", get(search::search))

        // Auth — POSTs live in auth_post_router() (rate-limited)
        .route("/login", get(auth::show_login))
        .route("/login/2fa", get(two_factor::show_login_2fa))
        .route("/register", get(auth::show_register))
        .route("/logout", post(auth::do_logout))

        // Submit GET only (POST is in write_post_router())
        .route("/submit", get(submit::show_submit))

        // /me/tokens (real impl) and other /me/* (stubs for now)
        .route("/me/tokens", get(me_tokens::show).post(me_tokens::create))
        .route("/me/tokens/{id}/revoke", post(me_tokens::revoke))
        .route("/me/edit", get(me_edit::show).post(me_edit::submit))
        .route("/me/verify-orcid", post(me_edit::verify_orcid))
        .route("/me/password", get(me_password::show).post(me_password::submit))
        .route("/me/notifications", get(notifications::show))
        .route("/me/notifications/{id}/read", post(notifications::mark_read))
        .route("/me/notifications/mark-all-read", post(notifications::mark_all_read))
        .route("/me/2fa", get(two_factor::show).post(two_factor::start_enroll))
        .route("/me/2fa/confirm", post(two_factor::confirm))
        .route("/me/2fa/disable", post(two_factor::disable))
        .route("/me/delete-account", get(me_account::show_delete).post(me_account::submit_delete))
        .route("/me/export", get(me_account::export))
        .route("/me/email", get(me_email::show).post(me_email::submit))
        .route("/me/email/cancel", post(me_email::cancel))
        .route("/me/resend-verification", post(verify::resend))
        .route("/verify/{token}", get(verify::show))
        .route("/confirm-email-change/{token}", get(me_email::confirm))

        // Forgot / reset password — GETs only; POSTs in auth_post_router().
        .route("/forgot-password", get(forgot::show_forgot))
        .route("/forgot-password/sent", get(forgot::show_sent))
        .route("/reset-password/{token}", get(forgot::show_reset))

        .route("/feed", get(feed::show))

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
        .route("/licenses", get(pages::licenses))

        // Crawler policy + indexer surface
        .route("/robots.txt", get(static_routes::robots_txt))
        .route("/sitemap.xml", get(feeds::sitemap))
        .route("/sitemap.xsl", get(feeds::sitemap_xsl))

        // OAI-PMH metadata-harvest endpoint (Dublin Core).
        .route("/oai", get(oai::oai))
}

/// POST endpoints subject to the strict auth-attempt rate limit
/// (5 attempts / min per IP). Layered with auth_layer in main.rs.
pub fn auth_post_router() -> Router<AppState> {
    Router::new()
        .route("/login", post(auth::do_login))
        .route("/login/2fa", post(two_factor::submit_login_2fa))
        .route("/register", post(auth::do_register))
        .route("/forgot-password", post(forgot::submit_forgot))
        .route("/reset-password/{token}", post(forgot::submit_reset))
}

/// POST endpoints subject to the standard write-throttle rate limit
/// (30 req / min per IP). Submission, voting, commenting. Layered
/// with write_layer in main.rs.
pub fn write_post_router() -> Router<AppState> {
    Router::new()
        .route(
            "/submit",
            post(submit::do_submit).layer(DefaultBodyLimit::max(UPLOAD_BODY_LIMIT)),
        )
        .route("/vote", post(votes::vote))
        .route("/m/{id}/comment", post(comments::post_comment))
        .route("/c/{id}/delete", post(comments::delete_comment))
        .route("/m/{id}/flag", post(flags::flag_manuscript))
        .route("/c/{id}/flag", post(flags::flag_comment))
}
