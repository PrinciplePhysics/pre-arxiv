use std::net::SocketAddr;

use anyhow::Context;
use axum::http::{header, HeaderValue};
use axum::Router;
use time::Duration;
use tower_http::compression::CompressionLayer;
use tower_http::services::ServeDir;
use tower_http::set_header::SetResponseHeaderLayer;
use tower_http::trace::TraceLayer;
use tower_sessions::cookie::SameSite;
use tower_sessions::{Expiry, SessionManagerLayer};
use tower_sessions_sqlx_store::SqliteStore;
use tracing_subscriber::EnvFilter;

mod api_auth;
mod auth;
mod categories;
mod db;
mod email;
mod error;
mod helpers;
mod licenses;
mod markdown;
mod email_change;
mod models;
mod passwords;
mod routes;
mod state;
mod templates;
mod verify;
mod versions;

use crate::state::AppState;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::try_from_default_env()
            .unwrap_or_else(|_| EnvFilter::new("info,sqlx=warn,tower_http=debug")))
        .init();

    let data_dir = std::env::var("DATA_DIR").unwrap_or_else(|_| {
        std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .map(|p| p.join("data"))
            .unwrap_or_else(|| "./data".into())
            .to_string_lossy()
            .into_owned()
    });
    let db_path = format!("{}/prearxiv.db", data_dir);

    let pool = db::connect(&db_path)
        .await
        .with_context(|| format!("connecting to sqlite at {db_path}"))?;

    sqlx::migrate!("./migrations")
        .run(&pool)
        .await
        .context("running sqlx migrations")?;

    // Session store — shares the same DB so we don't need a second file.
    let session_store = SqliteStore::new(pool.clone());
    session_store
        .migrate()
        .await
        .context("running tower-sessions migrations")?;

    let secure_cookies = std::env::var("NODE_ENV").as_deref() == Ok("production");
    let session_layer = SessionManagerLayer::new(session_store)
        .with_secure(secure_cookies)
        .with_http_only(true)
        .with_same_site(SameSite::Lax)
        .with_name("prexiv_session")
        .with_expiry(Expiry::OnInactivity(Duration::days(30)));

    let app_url = std::env::var("APP_URL").ok();
    let state = AppState { pool, app_url };

    let static_dir = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .map(|p| p.join("public"))
        .unwrap_or_else(|| "./public".into());

    // UPLOAD_DIR lives outside the source tree on production (so a git
    // reset --hard can't delete user PDFs). We serve it under
    // /static/uploads/ via a second, more-specific nest_service that
    // takes precedence over the broader /static fallback. Without this
    // bridge the PDFs land on disk but 404 in the browser.
    let upload_dir: std::path::PathBuf = std::env::var("UPLOAD_DIR")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|_| static_dir.join("uploads"));

    // Security headers — set on every response.
    //
    //   X-Content-Type-Options: nosniff  → stop the browser from
    //     content-sniffing an uploaded PDF as HTML/JS.
    //   X-Frame-Options: DENY            → defeat clickjacking (no
    //     other site can embed PreXiv in an iframe).
    //   Referrer-Policy: strict-origin-when-cross-origin
    //                                    → don't leak full URLs (which
    //     may include manuscript ids that aren't yet public) to outbound
    //     links.
    //   Permissions-Policy: interest-cohort=()
    //                                    → opt out of FLoC-style tracking
    //     (cheap; harmless to set).
    //   Strict-Transport-Security        → only in production, where the
    //     Tailscale Funnel serves HTTPS. Browsers ignore HSTS sent over
    //     plaintext HTTP, but spec says don't send it — so we gate on
    //     `secure_cookies` (same flag that means "we're behind HTTPS").
    let security_headers = tower::ServiceBuilder::new()
        .layer(SetResponseHeaderLayer::overriding(
            header::X_CONTENT_TYPE_OPTIONS,
            HeaderValue::from_static("nosniff"),
        ))
        .layer(SetResponseHeaderLayer::overriding(
            header::X_FRAME_OPTIONS,
            HeaderValue::from_static("DENY"),
        ))
        .layer(SetResponseHeaderLayer::overriding(
            header::REFERRER_POLICY,
            HeaderValue::from_static("strict-origin-when-cross-origin"),
        ))
        .layer(SetResponseHeaderLayer::if_not_present(
            header::HeaderName::from_static("permissions-policy"),
            HeaderValue::from_static("interest-cohort=()"),
        ));

    let app = Router::new()
        .merge(routes::router())
        // The more-specific upload mount goes FIRST so axum picks it up
        // before the generic /static fallback.
        .nest_service("/static/uploads", ServeDir::new(upload_dir))
        .nest_service("/static", ServeDir::new(static_dir))
        // Unmatched routes — return the styled 404 page.
        .fallback(routes::not_found_fallback)
        .layer(security_headers)
        .layer(SetResponseHeaderLayer::if_not_present(
            header::STRICT_TRANSPORT_SECURITY,
            if secure_cookies {
                HeaderValue::from_static("max-age=31536000; includeSubDomains")
            } else {
                HeaderValue::from_static("max-age=0")
            },
        ))
        .layer(CompressionLayer::new())
        .layer(TraceLayer::new_for_http())
        .layer(session_layer)
        .with_state(state);

    let port: u16 = std::env::var("PORT")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(3001);
    let addr = SocketAddr::from(([0, 0, 0, 0], port));
    let listener = tokio::net::TcpListener::bind(addr).await?;
    tracing::info!("prexiv (rust) listening on http://{addr}");
    axum::serve(listener, app).await?;
    Ok(())
}
