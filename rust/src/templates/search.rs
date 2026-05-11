use maud::{html, Markup};

use crate::models::ManuscriptListItem;

use super::home::manuscript_row;
use super::layout::{layout, PageCtx};

pub fn render(ctx: &PageCtx, query: &str, results: &[ManuscriptListItem]) -> Markup {
    let logged_in = ctx.user.is_some();
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
        @if results.is_empty() {
            div.empty { p { "No matches." } }
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
