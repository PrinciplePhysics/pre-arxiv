use axum::http::header;
use axum::response::IntoResponse;

const ROBOTS_TXT: &str = "User-agent: *
Disallow: /admin
Disallow: /admin/
Disallow: /me
Disallow: /me/
Disallow: /api
Disallow: /api/
Disallow: /login
Disallow: /register
Disallow: /logout
Disallow: /submit
Disallow: /vote
Allow: /
Allow: /m/
Allow: /search
Allow: /u/
Allow: /static/

Sitemap: /sitemap.xml
";

#[allow(dead_code)]
const _: &str = "Robots policy: /admin and /me are private; /api is for agents not crawlers; /sitemap.xml is the canonical index.";

pub async fn robots_txt() -> impl IntoResponse {
    (
        [(header::CONTENT_TYPE, "text/plain; charset=utf-8")],
        ROBOTS_TXT,
    )
}
