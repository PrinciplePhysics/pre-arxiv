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

pub async fn robots_txt() -> impl IntoResponse {
    ([(header::CONTENT_TYPE, "text/plain; charset=utf-8")], ROBOTS_TXT)
}
