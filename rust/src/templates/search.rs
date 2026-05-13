use maud::{html, Markup};

use crate::models::ManuscriptListItem;

use super::home::manuscript_row;
use super::layout::{layout, PageCtx};

pub fn render(ctx: &PageCtx, query: &str, results: &[ManuscriptListItem]) -> Markup {
    let logged_in = ctx.user.is_some();
    let audited = results.iter().filter(|m| m.has_auditor != 0).count();
    let autonomous = results
        .iter()
        .filter(|m| m.conductor_type == "ai-agent")
        .count();
    let body = html! {
        div.page-header {
            h1 { "Search results" }
            p.muted {
                "for "
                strong { (query) }
                " — "
                (results.len())
                " result"
                @if results.len() != 1 { "s" }
            }
        }
        @if !results.is_empty() {
            div.browse-overview aria-label="Search summary" {
                div.browse-stat {
                    strong { (results.len()) }
                    span { "matches" }
                }
                div.browse-stat {
                    strong { (audited) }
                    span { "audited" }
                }
                div.browse-stat {
                    strong { (autonomous) }
                    span { "autonomous agents" }
                }
            }
        }
        @if results.is_empty() {
            div.empty {
                p { "No manuscripts matched this search." }
                p.muted.small {
                    "Try a shorter title phrase, author name, DOI, PreXiv id, or subject category such as "
                    code { "cs.LG" }
                    "."
                }
                p.muted.small {
                    a href="/browse" { "Browse categories" }
                    " or "
                    a href="/new?show_all=1" { "view the full firehose" }
                    "."
                }
            }
        } @else {
            ol.ms-list {
                @for (i, m) in results.iter().enumerate() {
                    (manuscript_row(ctx, m, i + 1, logged_in))
                }
            }
        }
    };
    layout(&format!("Search: {query}"), ctx, body)
}
