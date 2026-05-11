use maud::{html, Markup};

use crate::models::ManuscriptListItem;

use super::layout::{layout, PageCtx};

pub fn render(ctx: &PageCtx, manuscripts: &[ManuscriptListItem]) -> Markup {
    let body = html! {
        h1 { "Recent manuscripts" }
        @if manuscripts.is_empty() {
            p.empty { "No manuscripts yet." }
        } @else {
            ul.manuscript-list {
                @for m in manuscripts {
                    li.manuscript-row.withdrawn[m.is_withdrawn()] {
                        a.manuscript-title href={ "/m/" (m.arxiv_like_id.as_deref().unwrap_or("")) } {
                            (m.title)
                        }
                        @if m.is_withdrawn() {
                            span.badge.withdrawn { "withdrawn" }
                        }
                        div.manuscript-meta {
                            span.authors { (m.authors) }
                            " · "
                            span.category { (m.category) }
                            " · "
                            span.conductor { (m.conductor_label()) }
                            " · "
                            span.score {
                                (m.score.unwrap_or(0)) " pts · "
                                (m.comment_count.unwrap_or(0)) " comments"
                            }
                        }
                    }
                }
            }
        }
    };
    layout("Home", ctx, body)
}
