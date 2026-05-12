//! RSS + Atom feeds and the sitemap.
//!
//!   GET /feed.rss             RSS 2.0, 30 most recent manuscripts.
//!   GET /feed/{cat}.rss       RSS 2.0, 30 most recent in `cat`.
//!   GET /sitemap.xml          Plain sitemap with main pages, all
//!                             manuscripts, all categories, all profiles.
//!
//! XML is written by hand (no rss/atom crate) to keep deps light and
//! tightly control escaping. We html-escape `<`, `>`, `&`, `"`, `'` in
//! every string interpolation.

use axum::extract::{Path, State};
use axum::http::header;
use axum::response::IntoResponse;
use chrono::{DateTime, NaiveDateTime, Utc};

use crate::error::AppResult;
use crate::state::AppState;

const SITEMAP_XSL: &str = include_str!("../../../public/static/sitemap.xsl");
const FEED_XSL: &str    = include_str!("../../../public/static/feed.xsl");

/// XSL stylesheet for sitemap.xml. Served at `/sitemap.xsl` with an
/// explicit `text/xsl` content-type so browsers apply it despite our
/// global `X-Content-Type-Options: nosniff` header.
pub async fn sitemap_xsl() -> impl IntoResponse {
    (
        [(header::CONTENT_TYPE, "text/xsl; charset=utf-8")],
        SITEMAP_XSL,
    )
}

/// XSL stylesheet for feed.rss (and per-category feeds), served at
/// `/feed.xsl` with `text/xsl`.
pub async fn feed_xsl() -> impl IntoResponse {
    (
        [(header::CONTENT_TYPE, "text/xsl; charset=utf-8")],
        FEED_XSL,
    )
}

const PAGE_FEED: usize = 30;

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

fn rfc2822(ts: &NaiveDateTime) -> String {
    DateTime::<Utc>::from_naive_utc_and_offset(*ts, Utc)
        .format("%a, %d %b %Y %H:%M:%S +0000")
        .to_string()
}

fn iso8601(ts: &NaiveDateTime) -> String {
    DateTime::<Utc>::from_naive_utc_and_offset(*ts, Utc)
        .format("%Y-%m-%dT%H:%M:%SZ")
        .to_string()
}

// ── helpers ──────────────────────────────────────────────────────────

struct FeedItem {
    slug: String,
    title: String,
    abstract_: String,
    authors: String,
    category: String,
    created_at: Option<NaiveDateTime>,
}

async fn fetch_feed_items(
    pool: &sqlx::SqlitePool,
    category: Option<&str>,
) -> AppResult<Vec<FeedItem>> {
    let sql = match category {
        Some(_) => "SELECT arxiv_like_id, title, abstract, authors, category, created_at
                    FROM manuscripts
                    WHERE category = ? AND withdrawn = 0
                    ORDER BY created_at DESC LIMIT ?".to_string(),
        None    => "SELECT arxiv_like_id, title, abstract, authors, category, created_at
                    FROM manuscripts
                    WHERE withdrawn = 0
                    ORDER BY created_at DESC LIMIT ?".to_string(),
    };
    let q = sqlx::query_as::<_, (Option<String>, String, String, String, String, Option<NaiveDateTime>)>(&sql);
    let rows = match category {
        Some(c) => q.bind(c).bind(PAGE_FEED as i64).fetch_all(pool).await?,
        None    => q.bind(PAGE_FEED as i64).fetch_all(pool).await?,
    };
    Ok(rows.into_iter().map(|(slug, title, abstract_, authors, category, created_at)| FeedItem {
        slug: slug.unwrap_or_default(),
        title,
        abstract_,
        authors,
        category,
        created_at,
    }).collect())
}

// ── /feed.rss + /feed/{cat}.rss ──────────────────────────────────────

pub async fn rss_all(State(state): State<AppState>) -> AppResult<impl IntoResponse> {
    let items = fetch_feed_items(&state.pool, None).await?;
    Ok(rss_response(&state, None, &items))
}

pub async fn rss_category(
    State(state): State<AppState>,
    Path(cat): Path<String>,
) -> AppResult<impl IntoResponse> {
    // Tolerate a trailing .rss for backward compatibility with any
    // bookmarks people may have made before /rss/{cat} replaced
    // /feed/{cat}.rss (axum doesn't allow params mixed with literal
    // suffixes in one segment).
    let cat = cat.trim_end_matches(".rss").to_string();
    let items = fetch_feed_items(&state.pool, Some(&cat)).await?;
    Ok(rss_response(&state, Some(&cat), &items))
}

fn rss_response(state: &AppState, category: Option<&str>, items: &[FeedItem]) -> impl IntoResponse {
    let base = base_url(state);
    let (channel_title, channel_url, channel_desc) = match category {
        Some(c) => (
            format!("PreXiv — {c}"),
            format!("{base}/browse/{c}"),
            format!("Newest manuscripts in category {c} on PreXiv."),
        ),
        None => (
            "PreXiv — all manuscripts".to_string(),
            format!("{base}/new"),
            "Newest manuscripts on PreXiv (all categories).".to_string(),
        ),
    };
    let now_rfc = chrono::Utc::now().format("%a, %d %b %Y %H:%M:%S +0000").to_string();
    let self_link = match category {
        Some(c) => format!("{base}/rss/{c}"),
        None    => format!("{base}/feed.rss"),
    };

    let mut xml = String::with_capacity(8192);
    xml.push_str(r#"<?xml version="1.0" encoding="UTF-8"?>"#);
    // Browser-friendly rendering: a stylesheet that turns the raw RSS
    // into a readable HTML page. Feed readers and harvesters ignore the
    // PI and parse the XML directly.
    xml.push_str("\n<?xml-stylesheet type=\"text/xsl\" href=\"/feed.xsl\"?>\n");
    xml.push_str("<rss version=\"2.0\" xmlns:atom=\"http://www.w3.org/2005/Atom\" xmlns:dc=\"http://purl.org/dc/elements/1.1/\">\n");
    xml.push_str("<channel>\n");
    xml.push_str(&format!("<title>{}</title>\n", xml_escape(&channel_title)));
    xml.push_str(&format!("<link>{}</link>\n",   xml_escape(&channel_url)));
    xml.push_str(&format!("<description>{}</description>\n", xml_escape(&channel_desc)));
    xml.push_str("<language>en-us</language>\n");
    xml.push_str(&format!("<lastBuildDate>{now_rfc}</lastBuildDate>\n"));
    xml.push_str(&format!("<atom:link href=\"{}\" rel=\"self\" type=\"application/rss+xml\"/>\n", xml_escape(&self_link)));
    xml.push_str("<generator>PreXiv</generator>\n");

    for it in items {
        let item_url = format!("{base}/m/{}", it.slug);
        let pubdate = it.created_at.map(|t| rfc2822(&t)).unwrap_or_else(|| now_rfc.clone());
        let desc = crate::markdown::render(&it.abstract_);
        xml.push_str("<item>\n");
        xml.push_str(&format!("<title>{}</title>\n", xml_escape(&it.title)));
        xml.push_str(&format!("<link>{}</link>\n", xml_escape(&item_url)));
        xml.push_str(&format!("<guid isPermaLink=\"true\">{}</guid>\n", xml_escape(&item_url)));
        xml.push_str(&format!("<pubDate>{pubdate}</pubDate>\n"));
        xml.push_str(&format!("<category>{}</category>\n", xml_escape(&it.category)));
        xml.push_str(&format!("<dc:creator>{}</dc:creator>\n", xml_escape(&it.authors)));
        xml.push_str(&format!("<description>{}</description>\n", xml_escape(&desc)));
        xml.push_str("</item>\n");
    }
    xml.push_str("</channel>\n</rss>\n");

    (
        [(header::CONTENT_TYPE, "application/rss+xml; charset=utf-8")],
        xml,
    )
}

// ── /sitemap.xml ─────────────────────────────────────────────────────

pub async fn sitemap(State(state): State<AppState>) -> AppResult<impl IntoResponse> {
    let base = base_url(&state).trim_end_matches('/').to_string();
    let mut xml = String::with_capacity(16384);
    xml.push_str(r#"<?xml version="1.0" encoding="UTF-8"?>"#);
    xml.push_str("\n<?xml-stylesheet type=\"text/xsl\" href=\"/sitemap.xsl\"?>\n");
    xml.push_str("<urlset xmlns=\"http://www.sitemaps.org/schemas/sitemap/0.9\">\n");

    // Static pages.
    for (path, prio) in &[
        ("/",            "1.0"),
        ("/new",         "0.9"),
        ("/top",         "0.9"),
        ("/audited",     "0.9"),
        ("/browse",      "0.8"),
        ("/about",       "0.5"),
        ("/guidelines",  "0.4"),
        ("/licenses",    "0.4"),
        ("/policies",    "0.4"),
        ("/tos",         "0.3"),
        ("/privacy",     "0.3"),
        ("/dmca",        "0.3"),
    ] {
        xml.push_str("<url>");
        xml.push_str(&format!("<loc>{}{}</loc>", xml_escape(&base), xml_escape(path)));
        xml.push_str(&format!("<priority>{prio}</priority>"));
        xml.push_str("</url>\n");
    }

    // Categories.
    for cat in crate::categories::CATEGORIES.iter() {
        xml.push_str("<url>");
        xml.push_str(&format!("<loc>{}/browse/{}</loc>", xml_escape(&base), xml_escape(cat.id)));
        xml.push_str("<priority>0.6</priority>");
        xml.push_str("</url>\n");
    }

    // Manuscripts.
    let ms: Vec<(Option<String>, Option<NaiveDateTime>)> = sqlx::query_as(
        "SELECT arxiv_like_id, COALESCE(updated_at, created_at) AS lastmod
         FROM manuscripts WHERE withdrawn = 0 ORDER BY id DESC LIMIT 50000",
    )
    .fetch_all(&state.pool)
    .await?;
    for (slug_opt, lm) in &ms {
        let slug = match slug_opt { Some(s) => s.as_str(), None => continue };
        xml.push_str("<url>");
        xml.push_str(&format!("<loc>{}/m/{}</loc>", xml_escape(&base), xml_escape(slug)));
        if let Some(t) = lm {
            xml.push_str(&format!("<lastmod>{}</lastmod>", iso8601(t)));
        }
        xml.push_str("<priority>0.7</priority>");
        xml.push_str("</url>\n");
    }

    // User profiles.
    let users: Vec<(String,)> = sqlx::query_as(
        "SELECT username FROM users WHERE EXISTS (SELECT 1 FROM manuscripts WHERE submitter_id = users.id AND withdrawn = 0) ORDER BY id LIMIT 10000",
    )
    .fetch_all(&state.pool)
    .await?;
    for (u,) in &users {
        xml.push_str("<url>");
        xml.push_str(&format!("<loc>{}/u/{}</loc>", xml_escape(&base), xml_escape(u)));
        xml.push_str("<priority>0.4</priority>");
        xml.push_str("</url>\n");
    }

    xml.push_str("</urlset>\n");
    Ok((
        [(header::CONTENT_TYPE, "application/xml; charset=utf-8")],
        xml,
    ))
}
