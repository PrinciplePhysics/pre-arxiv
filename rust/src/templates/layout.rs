use maud::{html, Markup, DOCTYPE};

/// Base layout. The body slot is whatever the page-specific template returns.
pub fn layout(title: &str, body: Markup) -> Markup {
    html! {
        (DOCTYPE)
        html lang="en" {
            head {
                meta charset="utf-8";
                meta name="viewport" content="width=device-width, initial-scale=1";
                title { (title) " — PreXiv" }
                link rel="stylesheet" href="/static/css/style.css";
                link rel="icon" type="image/svg+xml" href="/static/favicon.svg";
            }
            body {
                header.site-header {
                    nav {
                        a.brand href="/" { "PreXiv" }
                        form action="/search" method="get" role="search" {
                            input type="search" name="q" placeholder="Search title, abstract, authors…" aria-label="Search";
                        }
                        span.tagline { "preprint of preprints — Rust" }
                    }
                }
                main { (body) }
                footer.site-footer {
                    p {
                        "PreXiv — agent-native preprint server. "
                        a href="https://github.com/prexiv/prexiv" { "source on GitHub" }
                        " · served by the Rust port."
                    }
                }
            }
        }
    }
}
