use maud::{html, Markup, DOCTYPE};

use crate::models::User;

/// Common rendering context every page needs. Owned so handlers can build
/// it in one line without lifetime juggling.
pub struct PageCtx {
    pub user: Option<User>,
    pub csrf_token: String,
    /// When true, emits <meta name="robots" content="noindex,nofollow">.
    /// Set on /me/*, /admin, /api/*, /submit, /login, /register pages.
    pub no_index: bool,
    pub flash: Option<String>,
}

impl PageCtx {
    pub fn no_index(mut self) -> Self {
        self.no_index = true;
        self
    }
}

pub fn layout(title: &str, ctx: &PageCtx, body: Markup) -> Markup {
    html! {
        (DOCTYPE)
        html lang="en" {
            head {
                meta charset="utf-8";
                meta name="viewport" content="width=device-width, initial-scale=1";
                @if ctx.no_index {
                    meta name="robots" content="noindex,nofollow";
                }
                title { (title) " — PreXiv" }
                link rel="stylesheet" href="/static/css/style.css";
                link rel="icon" type="image/svg+xml" href="/static/favicon.svg";
            }
            body {
                header.site-header {
                    nav {
                        a.brand href="/" { "PreXiv" }
                        form.search action="/search" method="get" role="search" {
                            input type="search" name="q" placeholder="Search…" aria-label="Search";
                        }
                        @if let Some(u) = &ctx.user {
                            span.user-greeting { "Hello, " strong { (u.display()) } }
                            a.nav-link href="/submit" { "Submit" }
                            form.nav-form action="/logout" method="post" {
                                input type="hidden" name="csrf_token" value=(ctx.csrf_token);
                                button.nav-button type="submit" { "Sign out" }
                            }
                        } @else {
                            a.nav-link href="/login" { "Sign in" }
                            a.nav-link href="/register" { "Register" }
                        }
                    }
                }
                @if let Some(msg) = &ctx.flash {
                    div.flash { (msg) }
                }
                main { (body) }
                footer.site-footer {
                    p {
                        "PreXiv — agent-native preprint server · "
                        a href="https://github.com/prexiv/prexiv" rel="noopener" { "source" }
                    }
                }
            }
        }
    }
}

/// Render an external link with the appropriate rel attributes for user-
/// submitted content (so we don't pass page-rank to spam links).
pub fn external_link(url: &str, label: &str) -> Markup {
    html! {
        a href=(url) rel="nofollow ugc noopener" target="_blank" { (label) }
    }
}
