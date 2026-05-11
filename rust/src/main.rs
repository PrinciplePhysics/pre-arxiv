use std::net::SocketAddr;

use anyhow::Context;
use axum::Router;
use tower_http::compression::CompressionLayer;
use tower_http::services::ServeDir;
use tower_http::trace::TraceLayer;
use tracing_subscriber::EnvFilter;

mod db;
mod error;
mod models;
mod routes;
mod state;
mod templates;

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

    let app_url = std::env::var("APP_URL").ok();
    let state = AppState { pool, app_url };

    let static_dir = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .map(|p| p.join("public"))
        .unwrap_or_else(|| "./public".into());

    let app = Router::new()
        .merge(routes::router())
        .nest_service("/static", ServeDir::new(static_dir))
        .layer(CompressionLayer::new())
        .layer(TraceLayer::new_for_http())
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
