use maud::{html, Markup};

use crate::models::ManuscriptListItem;

use super::layout::{layout, PageCtx};

pub fn render(ctx: &PageCtx, query: &str, results: &[ManuscriptListItem]) -> Markup {
    let body = html! {
        h1 { "Search results" }
        p.query { "for " strong { (query) } " — " (results.len()) " result" @if results.len() != 1 { "s" } }
        @if results.is_empty() {
            p.empty { "No matches." }
        } @else {
            ul.manuscript-list {
                @for m in results {
                    li.manuscript-row {
                        a.manuscript-title href={ "/m/" (m.arxiv_like_id.as_deref().unwrap_or("")) } {
                            (m.title)
                        }
                        div.manuscript-meta {
                            span.authors { (m.authors) }
                            " · "
                            span.category { (m.category) }
                            " · "
                            span.conductor { (m.conductor_label()) }
                        }
                    }
                }
            }
        }
    };
    layout(&format!("Search: {query}"), ctx, body)
}
