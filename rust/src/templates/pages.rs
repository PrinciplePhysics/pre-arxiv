//! Static-content pages (about / guidelines / ToS / privacy / DMCA / policies).
//!
//! Each page's body HTML lives as a sibling .html file under
//! `pages_content/`, embedded at compile time with `include_str!`. The
//! HTML was extracted from the JS app's EJS templates verbatim — same
//! wording, same structure, same CSS classes. To update content, edit
//! the .html file and rebuild.

use std::env;

use maud::{html, Markup, PreEscaped};

use super::layout::{layout, PageCtx};

#[derive(Debug)]
struct Contact {
    href: String,
    label: String,
}

fn env_or(var: &str, default: &str) -> String {
    env::var(var)
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| default.to_string())
}

fn contact_from_env(var: &str, default: &str) -> Contact {
    let href = env_or(var, default);
    let label = href
        .strip_prefix("mailto:")
        .unwrap_or(href.as_str())
        .to_string();
    Contact { href, label }
}

fn escape_html(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    for ch in value.chars() {
        match ch {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&#39;"),
            _ => out.push(ch),
        }
    }
    out
}

fn replace_contact(page: String, prefix: &str, contact: &Contact) -> String {
    page.replace(
        &format!("{{{{{prefix}_CONTACT_HREF}}}}"),
        &escape_html(&contact.href),
    )
    .replace(
        &format!("{{{{{prefix}_CONTACT_LABEL}}}}"),
        &escape_html(&contact.label),
    )
}

fn hydrate_legal_placeholders(body_html: &str) -> String {
    let operator = env_or("PREXIV_OPERATOR_NAME", "the PreXiv operator");
    let governing_law = env_or(
        "PREXIV_GOVERNING_LAW",
        "the laws of the jurisdiction where the PreXiv operator is domiciled, excluding conflict-of-law rules",
    );
    let counter_jurisdiction = env_or(
        "PREXIV_DMCA_COUNTER_JURISDICTION",
        "a federal judicial district where your address is located, or if outside the United States, any judicial district where the service provider may be found",
    );

    let mut page = body_html
        .replace("{{OPERATOR_NAME}}", &escape_html(&operator))
        .replace("{{GOVERNING_LAW}}", &escape_html(&governing_law))
        .replace(
            "{{DMCA_COUNTER_JURISDICTION}}",
            &escape_html(&counter_jurisdiction),
        );

    for (prefix, var, default) in [
        ("LEGAL", "PREXIV_LEGAL_CONTACT", "mailto:legal@prexiv.org"),
        (
            "PRIVACY",
            "PREXIV_PRIVACY_CONTACT",
            "mailto:privacy@prexiv.org",
        ),
        ("DMCA", "PREXIV_DMCA_CONTACT", "mailto:dmca@prexiv.org"),
        (
            "APPEALS",
            "PREXIV_APPEALS_CONTACT",
            "mailto:appeals@prexiv.org",
        ),
    ] {
        let contact = contact_from_env(var, default);
        page = replace_contact(page, prefix, &contact);
    }

    page
}

pub fn render(ctx: &PageCtx, title: &str, body_html: &str) -> Markup {
    let body_html = hydrate_legal_placeholders(body_html);
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
pub const LICENSES: &str = include_str!("pages_content/licenses.html");
pub const PERMISSIONS: &str = include_str!("pages_content/permissions.html");
pub const HOW_IT_WORKS: &str = include_str!("pages_content/how_it_works.html");
pub const AGENT_SUPPORT: &str = include_str!("pages_content/agent_support.html");
