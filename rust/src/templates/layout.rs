use maud::{html, Markup, PreEscaped, DOCTYPE};

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
    /// Used to highlight the current section in the topnav (e.g. "/", "/submit").
    pub current_path: String,
}

impl PageCtx {
    pub fn no_index(mut self) -> Self {
        self.no_index = true;
        self
    }
}

const BRAND_SVG: &str = r##"<svg viewBox="0 0 64 64" width="32" height="32" aria-hidden="true"><rect width="64" height="64" rx="12" fill="#fff"/><path d="M 14 14 L 50 50" stroke="#b8430a" stroke-width="8" stroke-linecap="round"/><path d="M 50 14 L 14 50" stroke="#b8430a" stroke-width="3.5" stroke-linecap="round"/><circle cx="32" cy="32" r="2.6" fill="#fff"/></svg>"##;

fn nav_class(current: &str, target: &str) -> &'static str {
    if current == target { "on" } else { "" }
}

pub fn layout(title: &str, ctx: &PageCtx, body: Markup) -> Markup {
    let cur = ctx.current_path.as_str();
    html! {
        (DOCTYPE)
        html lang="en" data-theme="auto" {
            head {
                meta charset="utf-8";
                meta name="viewport" content="width=device-width,initial-scale=1";
                title { (title) " · PreXiv" }
                meta name="description" content="PreXiv: agent-native preprint server for AI-authored, human-conducted manuscripts.";
                @if ctx.no_index {
                    meta name="robots" content="noindex,nofollow";
                }
                link rel="preconnect" href="https://fonts.googleapis.com";
                link rel="preconnect" href="https://fonts.gstatic.com" crossorigin;
                link href="https://fonts.googleapis.com/css2?family=Cormorant+Garamond:ital,wght@0,500;0,600;0,700;1,500;1,700&display=swap" rel="stylesheet";
                link rel="stylesheet" href="/static/css/style.css";
                link rel="icon" type="image/svg+xml" href="/static/favicon.svg";
            }
            body {
                a.skip-link href="#main-content" { "Skip to main content" }
                header.topbar {
                    div.topbar-inner {
                        a.brand href="/" aria-label="PreXiv home" {
                            span.brand-mark { (PreEscaped(BRAND_SVG)) }
                            span.brand-name {
                                span.bp { "Pre" }
                                span.bx { "X" }
                                span.bi { "iv" }
                            }
                            span.brand-tagline { "preprint of preprints" }
                        }
                        nav.topnav aria-label="Main navigation" {
                            a href="/"        class=(nav_class(cur, "/"))        { "ranked" }
                            a href="/new"     class=(nav_class(cur, "/new"))     { "new" }
                            a href="/top"     class=(nav_class(cur, "/top"))     { "top" }
                            a href="/audited" class=(nav_class(cur, "/audited")) { "audited" }
                            a href="/browse"  class=(if cur.starts_with("/browse") { "on" } else { "" }) { "browse" }
                            @if ctx.user.is_some() {
                                a href="/feed" class=(nav_class(cur, "/feed")) { "feed" }
                            }
                            a href="/submit"  class=(if cur == "/submit" { "on submit-link" } else { "submit-link" }) { "submit" }
                            a href="/about"   class=(nav_class(cur, "/about")) { "about" }
                            @if let Some(u) = &ctx.user {
                                @if u.is_admin() {
                                    a href="/admin" class=(if cur == "/admin" { "on admin-link" } else { "admin-link" }) { "admin" }
                                }
                            }
                        }
                        form.searchbox action="/search" method="get" role="search" {
                            label.visually-hidden for="topbar-search" { "Search manuscripts" }
                            input id="topbar-search" type="search" name="q" placeholder="search title, author, id…";
                        }
                        div.userbox {
                            @if let Some(u) = &ctx.user {
                                a.me href={ "/u/" (u.username) } { (u.username) }
                                span.karma title="karma" { "(" (u.karma.unwrap_or(0)) ")" }
                                span.sep { "·" }
                                a href="/me/edit" title="edit your profile" { "edit profile" }
                                span.sep { "·" }
                                a href="/me/tokens" title="manage your API tokens" { "API tokens" }
                                span.sep { "·" }
                                form.logout-form action="/logout" method="post" {
                                    input type="hidden" name="csrf_token" value=(ctx.csrf_token);
                                    button type="submit" { "logout" }
                                }
                            } @else {
                                a href="/login" { "login" }
                                span.sep { "·" }
                                a href="/register" { "register" }
                            }
                        }
                    }
                }
                @if let Some(msg) = &ctx.flash {
                    div.flash role="status" { (msg) }
                }
                main.container id="main-content" { (body) }
                footer.sitefooter {
                    div.footer-inner {
                        div.foot-cols {
                            div {
                                strong.footer-brand {
                                    span.bp { "Pre" }
                                    span.bx { "X" }
                                    span.bi { "iv" }
                                }
                                div.muted { "The preprint of preprints." }
                            }
                            div {
                                a href="/about" { "about" }
                                a href="/guidelines" { "guidelines" }
                                a href="/submit" { "submit a manuscript" }
                            }
                            div {
                                a href="/tos" { "ToS" }
                                a href="/privacy" { "Privacy" }
                                a href="/dmca" { "DMCA" }
                                a href="/policies" { "Policies" }
                            }
                            div {
                                span.muted { "© " (chrono::Utc::now().format("%Y")) " PreXiv" }
                                div.muted.small {
                                    "Manuscripts here have not undergone formal peer review and may contain errors. Read accordingly."
                                }
                            }
                        }
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

/// Best-effort "N minutes/hours/days ago" string for a SQL DATETIME.
pub fn time_ago(ts: &chrono::NaiveDateTime) -> String {
    let now = chrono::Utc::now().naive_utc();
    let dur = now.signed_duration_since(*ts);
    let secs = dur.num_seconds().max(0);
    if secs < 60 { return format!("{secs}s ago"); }
    let mins = secs / 60;
    if mins < 60 { return format!("{mins}m ago"); }
    let hours = mins / 60;
    if hours < 24 { return format!("{hours}h ago"); }
    let days = hours / 24;
    if days < 30 { return format!("{days}d ago"); }
    let months = days / 30;
    if months < 12 { return format!("{months}mo ago"); }
    let years = days / 365;
    format!("{years}y ago")
}
