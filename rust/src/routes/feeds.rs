//! /sitemap.xml — site index for search engines and harvesters.
//!
//! Browser-friendly rendering: the XML carries a `<?xml-stylesheet?>` PI
//! pointing at /sitemap.xsl, so a human visiting the URL gets a styled
//! HTML table; bots ignore the PI and parse the XML.

use axum::extract::State;
use axum::http::header;
use axum::response::IntoResponse;
use chrono::{DateTime, NaiveDateTime, Utc};

use crate::error::AppResult;
use crate::state::AppState;

const SITEMAP_XSL: &str = include_str!("../../../public/static/sitemap.xsl");

fn xml_escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 8);
    for c in s.chars() {
        match c {
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '&' => out.push_str("&amp;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&apos;"),
            _ => out.push(c),
        }
    }
    out
}

fn base_url(state: &AppState) -> &str {
    state.app_url.as_deref().unwrap_or("http://localhost:3001")
}

fn iso8601(ts: &NaiveDateTime) -> String {
    DateTime::<Utc>::from_naive_utc_and_offset(*ts, Utc)
        .format("%Y-%m-%dT%H:%M:%SZ")
        .to_string()
}

/// XSL stylesheet for sitemap.xml. Served as `application/xslt+xml`,
/// the canonical content-type that every browser+nosniff accepts.
pub async fn sitemap_xsl() -> impl IntoResponse {
    (
        [(header::CONTENT_TYPE, "application/xslt+xml; charset=utf-8")],
        SITEMAP_XSL,
    )
}

pub async fn sitemap(State(state): State<AppState>) -> AppResult<impl IntoResponse> {
    let base = base_url(&state).trim_end_matches('/').to_string();
    let mut xml = String::with_capacity(16384);
    xml.push_str(r#"<?xml version="1.0" encoding="UTF-8"?>"#);
    xml.push_str("\n<?xml-stylesheet type=\"text/xsl\" href=\"/sitemap.xsl\"?>\n");
    xml.push_str("<urlset xmlns=\"http://www.sitemaps.org/schemas/sitemap/0.9\">\n");

    // Static pages.
    for (path, prio) in &[
        ("/", "1.0"),
        ("/new", "0.9"),
        ("/top", "0.9"),
        ("/audited", "0.9"),
        ("/browse", "0.8"),
        ("/about", "0.5"),
        ("/guidelines", "0.4"),
        ("/licenses", "0.4"),
        ("/policies", "0.4"),
        ("/tos", "0.3"),
        ("/privacy", "0.3"),
        ("/dmca", "0.3"),
    ] {
        xml.push_str("<url>");
        xml.push_str(&format!(
            "<loc>{}{}</loc>",
            xml_escape(&base),
            xml_escape(path)
        ));
        xml.push_str(&format!("<priority>{prio}</priority>"));
        xml.push_str("</url>\n");
    }

    // Categories.
    for cat in crate::categories::CATEGORIES.iter() {
        xml.push_str("<url>");
        xml.push_str(&format!(
            "<loc>{}/browse/{}</loc>",
            xml_escape(&base),
            xml_escape(cat.id)
        ));
        xml.push_str("<priority>0.6</priority>");
        xml.push_str("</url>\n");
    }

    // Manuscripts.
    let ms: Vec<(Option<String>, Option<NaiveDateTime>)> = sqlx::query_as(crate::db::pg(
        "SELECT arxiv_like_id, COALESCE(updated_at, created_at) AS lastmod
         FROM manuscripts WHERE withdrawn = 0 ORDER BY id DESC LIMIT 50000",
    ))
    .fetch_all(&state.pool)
    .await?;
    for (slug_opt, lm) in &ms {
        let slug = match slug_opt {
            Some(s) => s.as_str(),
            None => continue,
        };
        let public_slug = slug.strip_prefix("prexiv:").unwrap_or(slug);
        xml.push_str("<url>");
        xml.push_str(&format!(
            "<loc>{}/abs/{}</loc>",
            xml_escape(&base),
            xml_escape(public_slug)
        ));
        if let Some(t) = lm {
            xml.push_str(&format!("<lastmod>{}</lastmod>", iso8601(t)));
        }
        xml.push_str("<priority>0.7</priority>");
        xml.push_str("</url>\n");
    }

    // User profiles.
    let users: Vec<(String,)> = sqlx::query_as(
        crate::db::pg("SELECT username FROM users WHERE EXISTS (SELECT 1 FROM manuscripts WHERE submitter_id = users.id AND withdrawn = 0) ORDER BY id LIMIT 10000"),
    )
    .fetch_all(&state.pool)
    .await?;
    for (u,) in &users {
        xml.push_str("<url>");
        xml.push_str(&format!(
            "<loc>{}/u/{}</loc>",
            xml_escape(&base),
            xml_escape(u)
        ));
        xml.push_str("<priority>0.4</priority>");
        xml.push_str("</url>\n");
    }

    xml.push_str("</urlset>\n");
    Ok((
        [(header::CONTENT_TYPE, "application/xml; charset=utf-8")],
        xml,
    ))
}
