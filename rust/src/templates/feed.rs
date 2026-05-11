use maud::{html, Markup};

use crate::models::ManuscriptListItem;

use super::home::manuscript_row;
use super::layout::{layout, PageCtx};

pub fn render(
    ctx: &PageCtx,
    rows: &[ManuscriptListItem],
    page: i64,
    per: i64,
    follow_count: i64,
) -> Markup {
    let logged_in = ctx.user.is_some();
    let body = html! {
        div.page-header {
            h1 { "Your feed" }
            p.muted {
                "Manuscripts from people you follow, newest first."
                @if follow_count > 0 {
                    " You follow " strong { (follow_count) } " "
                    @if follow_count == 1 { "user" } @else { "users" } "."
                }
            }
        }

        @if rows.is_empty() {
            div.empty {
                p { "Your feed is empty." }
                p.muted {
                    "Follow other PreXiv users from their "
                    code { "/u/<username>" }
                    " page and their submissions will appear here."
                }
                p {
                    a.btn-primary href="/" { "Browse the ranked feed →" }
                    " "
                    a.btn-secondary href="/new" { "Or just see newest" }
                }
            }
        } @else {
            ol.ms-list {
                @for (i, m) in rows.iter().enumerate() {
                    (manuscript_row(ctx, m, ((page - 1) * per + i as i64 + 1) as usize, logged_in))
                }
            }
            nav.pagination aria-label="Pagination" {
                @if page > 1 {
                    a href={ "?page=" (page - 1) } { "← previous" }
                }
                " "
                span.muted { "page " (page) }
                " "
                @if (rows.len() as i64) == per {
                    a href={ "?page=" (page + 1) } { "next →" }
                }
            }
        }
    };
    layout("Your feed", ctx, body)
}
