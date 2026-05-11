use maud::{html, Markup};

use crate::models::ManuscriptListItem;

use super::home::manuscript_row;
use super::layout::{layout, PageCtx};

pub fn render(
    ctx: &PageCtx,
    heading: &str,
    subheading: &str,
    rows: &[ManuscriptListItem],
    _self_path: &str,
) -> Markup {
    let logged_in = ctx.user.is_some();
    let body = html! {
        div.page-header {
            h1 { (heading) }
            p.muted { (subheading) }
        }
        @if rows.is_empty() {
            div.empty { p { "No manuscripts here yet." } }
        } @else {
            ol.ms-list {
                @for (i, m) in rows.iter().enumerate() {
                    (manuscript_row(ctx, m, i + 1, logged_in))
                }
            }
        }
    };
    layout(heading, ctx, body)
}

pub fn render_browse(ctx: &PageCtx, counts: &[(String, i64)]) -> Markup {
    let body = html! {
        div.page-header {
            h1 { "Browse by category" }
            p.muted { "Pick a category to see all its manuscripts." }
        }
        @if counts.is_empty() {
            div.empty { p { "No categories yet." } }
        } @else {
            ul.category-index {
                @for (cat, n) in counts {
                    li {
                        a.ms-cat-pill href={ "/browse/" (cat) } { (cat) }
                        " "
                        span.muted { "(" (n) ")" }
                    }
                }
            }
        }
    };
    layout("Browse", ctx, body)
}
