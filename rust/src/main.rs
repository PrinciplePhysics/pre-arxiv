use std::net::SocketAddr;

use std::sync::Arc;

use anyhow::Context;
use axum::http::{header, HeaderValue};
use axum::Router;
use time::Duration;
use tower_governor::governor::GovernorConfigBuilder;
use tower_governor::GovernorLayer;
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
mod compile;
mod crockford;
mod crypto;
mod db;
mod email;
mod error;
mod helpers;
mod licenses;
mod markdown;
mod email_change;
mod models;
mod notifications;
mod orcid;
mod passwords;
mod routes;
mod state;
mod templates;
mod totp;
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

    // S-7: load the email-at-rest master key, then backfill any legacy
    // rows still missing an encrypted email. Both steps are idempotent —
    // re-running on every startup is fine (and is the recovery path if
    // someone restores from an older backup).
    crypto::init().context("initialising PREXIV_DATA_KEY (S-7)")?;
    let backfilled = backfill_user_emails(&pool)
        .await
        .context("backfilling user email_enc / email_hash")?;
    if backfilled > 0 {
        tracing::info!("S-7 backfill: encrypted {backfilled} legacy email rows");
    }
    let inst_set = backfill_institutional_email(&pool)
        .await
        .context("backfilling users.institutional_email")?;
    if inst_set > 0 {
        tracing::info!(
            "verified-scholar backfill: tagged {inst_set} users with institutional_email=1"
        );
    }

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

    // Per-IP rate limiting via tower_governor. Default key extractor
    // uses the source IP (taking it from the socket; behind Tailscale
    // Funnel this is the inbound Funnel-relay IP, which still gives us
    // a usable token-bucket because a single client's requests share
    // the same source). Two buckets:
    //
    //   * `auth_governor` — applied to /login, /register, /forgot-password
    //     and the /login/2fa second-step. 5 attempts per minute with a
    //     burst of 5. Defends against credential-stuffing.
    //
    //   * `write_governor` — applied to /submit, /vote, comment posts,
    //     and the API write paths. 30 requests per minute, burst 30.
    //     Defends against vote-brigading and submission spam.
    let auth_governor = Arc::new(
        GovernorConfigBuilder::default()
            .per_second(12) // 5/min average
            .burst_size(5)
            .finish()
            .expect("auth GovernorConfig"),
    );
    let write_governor = Arc::new(
        GovernorConfigBuilder::default()
            .per_second(2)  // 30/min average
            .burst_size(30)
            .finish()
            .expect("write GovernorConfig"),
    );
    let auth_layer  = GovernorLayer::new(auth_governor);
    let write_layer = GovernorLayer::new(write_governor);

    let app = Router::new()
        .merge(routes::router())
        .merge(routes::auth_post_router().layer(auth_layer))
        .merge(routes::write_post_router().layer(write_layer))
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
    // ConnectInfo<SocketAddr> is required by tower_governor's default
    // PeerIpKeyExtractor — without it, every rate-limited request
    // 500s. `into_make_service_with_connect_info::<SocketAddr>` is
    // the standard fix.
    axum::serve(
        listener,
        app.into_make_service_with_connect_info::<SocketAddr>(),
    )
    .await?;
    Ok(())
}

/// S-7 startup pass: encrypt the plaintext `email` column for any
/// user row whose `email_hash` is still NULL. Returns the number of
/// rows updated. Idempotent — running it twice does nothing the
/// second time. Skips rows with an empty `email` (which is what an
/// operator-level "hard zero the plaintext" pass produces; we don't
/// want to re-encrypt empty strings on top of a legitimate ciphertext).
/// One-time pass that flips `institutional_email = 1` for any row
/// whose plaintext email (still populated during the S-7 rollout) is
/// on the institutional-domain allowlist AND whose email has been
/// verified. Idempotent — the `WHERE institutional_email = 0` guard
/// means re-running this on every startup costs one cheap scan.
async fn backfill_institutional_email(pool: &sqlx::SqlitePool) -> anyhow::Result<usize> {
    let rows: Vec<(i64, Option<String>, i64)> = sqlx::query_as(
        "SELECT id, email, email_verified FROM users
          WHERE institutional_email = 0 AND email IS NOT NULL AND email <> ''",
    )
    .fetch_all(pool)
    .await?;
    let mut n = 0usize;
    for (id, email_opt, verified) in rows {
        if verified == 0 {
            continue;
        }
        let email = email_opt.unwrap_or_default();
        if !crate::email::is_institutional(&email) {
            continue;
        }
        sqlx::query("UPDATE users SET institutional_email = 1 WHERE id = ?")
            .bind(id)
            .execute(pool)
            .await?;
        n += 1;
    }
    Ok(n)
}

async fn backfill_user_emails(pool: &sqlx::SqlitePool) -> anyhow::Result<usize> {
    let rows: Vec<(i64, Option<String>)> = sqlx::query_as(
        "SELECT id, email FROM users WHERE email_hash IS NULL",
    )
    .fetch_all(pool)
    .await?;
    let mut n = 0usize;
    for (id, email_opt) in rows {
        let email = email_opt.unwrap_or_default();
        if email.trim().is_empty() {
            continue;
        }
        let (hash, enc) = crypto::seal_email(&email)?;
        let hash_vec = hash.to_vec();
        sqlx::query(
            "UPDATE users SET email_hash = ?, email_enc = ? WHERE id = ?",
        )
        .bind(&hash_vec)
        .bind(&enc)
        .bind(id)
        .execute(pool)
        .await?;
        n += 1;
    }
    Ok(n)
}
