//! Static-content pages (about / guidelines / ToS / privacy / DMCA / policies).
//!
//! Each page's body HTML lives as a sibling .html file under
//! `pages_content/`, embedded at compile time with `include_str!`. The
//! HTML was extracted from the JS app's EJS templates verbatim — same
//! wording, same structure, same CSS classes. To update content, edit
//! the .html file and rebuild.

use maud::{html, Markup, PreEscaped};

use super::layout::{layout, PageCtx};

pub fn render(ctx: &PageCtx, title: &str, body_html: &str) -> Markup {
    let body = html! {
        (PreEscaped(body_html))
    };
    layout(title, ctx, body)
}

pub const ABOUT: &str = include_str!("pages_content/about.html");
pub const GUIDELINES: &str = include_str!("pages_content/guidelines.html");
pub const TOS: &str = include_str!("pages_content/tos.html");
pub const PRIVACY: &str = include_str!("pages_content/privacy.html");
pub const DMCA: &str = include_str!("pages_content/dmca.html");
pub const POLICIES: &str = include_str!("pages_content/policies.html");
