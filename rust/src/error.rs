//! Application-level error type + IntoResponse renderer.
//!
//! On 404 and 500 we emit a small self-contained HTML page (not via the
//! full `layout()` because AppError doesn't carry session context).
//! The page uses the same `.error-page` styles as the rest of the site,
//! so it doesn't look orphaned.

use axum::http::StatusCode;
use axum::response::{Html, IntoResponse, Response};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum AppError {
    #[error("not found")]
    NotFound,

    #[error(transparent)]
    Sqlx(#[from] sqlx::Error),

    #[error(transparent)]
    Other(#[from] anyhow::Error),
}

pub type AppResult<T> = Result<T, AppError>;

impl IntoResponse for AppError {
    fn into_response(self) -> Response {
        match self {
            AppError::NotFound => (
                StatusCode::NOT_FOUND,
                Html(render_error_page(
                    404,
                    "Page not found",
                    "The page you're looking for doesn't exist on PreXiv. It may have been renamed, withdrawn, or never existed.",
                )),
            )
                .into_response(),
            AppError::Sqlx(sqlx::Error::RowNotFound) => (
                StatusCode::NOT_FOUND,
                Html(render_error_page(
                    404,
                    "Page not found",
                    "The record you're looking for doesn't exist on PreXiv.",
                )),
            )
                .into_response(),
            AppError::Sqlx(e) => {
                tracing::error!(error = %e, "sqlx error");
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    Html(render_error_page(
                        500,
                        "Something went wrong",
                        "We hit an internal error processing your request. The details were logged server-side; try again in a moment.",
                    )),
                )
                    .into_response()
            }
            AppError::Other(e) => {
                tracing::error!(error = %e, "internal error");
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    Html(render_error_page(
                        500,
                        "Something went wrong",
                        "We hit an internal error processing your request. The details were logged server-side; try again in a moment.",
                    )),
                )
                    .into_response()
            }
        }
    }
}

/// Render the standalone HTML error page.
///
/// Self-contained: it doesn't need PageCtx (errors are produced without
/// session info), so it includes the topbar inline as static HTML. The
/// styles come from /static/css/{style,prexiv-rust}.css just like every
/// other page.
pub fn render_error_page(status: u16, headline: &str, message: &str) -> String {
    let h = html_escape(headline);
    let m = html_escape(message);
    format!(
        r##"<!DOCTYPE html>
<html lang="en" data-theme="auto">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width,initial-scale=1">
<title>{h} · PreXiv</title>
<meta name="robots" content="noindex,nofollow">
<link rel="stylesheet" href="/static/vendor/fonts/cormorant/cormorant-garamond.css?v=20260516a">
<link rel="stylesheet" href="/static/css/style.css?v=20260516a">
<link rel="stylesheet" href="/static/css/prexiv-rust.css?v=20260516a">
<link rel="icon" type="image/svg+xml" href="/static/favicon.svg">
</head>
<body>
<header class="topbar"><div class="topbar-inner">
<a class="brand" href="/" aria-label="PreXiv home">
<span class="brand-mark"><svg viewBox="0 0 64 64" width="32" height="32" aria-hidden="true"><rect width="64" height="64" rx="12" fill="#fff"/><path d="M 14 14 L 50 50" stroke="#b8430a" stroke-width="8" stroke-linecap="round"/><path d="M 50 14 L 14 50" stroke="#b8430a" stroke-width="3.5" stroke-linecap="round"/><circle cx="32" cy="32" r="2.6" fill="#fff"/></svg></span>
<span class="brand-name"><span class="bp">Pre</span><span class="bx">X</span><span class="bi">iv</span></span>
</a>
</div></header>
<main class="container">
<div class="error-page">
<span class="error-page-code">{status}</span>
<h1 class="error-page-h">{h}</h1>
<p class="muted">{m}</p>
<div class="error-page-actions">
<a class="btn-primary" href="/">← Back to homepage</a>
<a class="btn-secondary" href="/browse">Browse categories</a>
<a class="btn-secondary" href="/search">Search manuscripts</a>
</div>
</div>
</main>
</body>
</html>"##
    )
}

fn html_escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}
