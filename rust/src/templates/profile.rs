use maud::{html, Markup};

use crate::models::{ManuscriptListItem, User};

use super::home::manuscript_row;
use super::layout::{layout, PageCtx};

pub fn render(ctx: &PageCtx, u: &User, rows: &[ManuscriptListItem]) -> Markup {
    let logged_in = ctx.user.is_some();
    let body = html! {
        div.page-header {
            h1 { "@" (u.username) }
            @if let Some(d) = &u.display_name {
                p.muted { (d) }
            }
            p.muted {
                "karma " (u.karma.unwrap_or(0))
                " · " (rows.len()) " manuscripts"
                @if u.is_admin() { " · " span.role-tag { "admin" } }
            }
            @if let Some(b) = &u.bio { p { (b) } }
            @if let Some(a) = &u.affiliation { p.muted { "Affiliation: " (a) } }
        }
        h2 { "Submitted manuscripts" }
        @if rows.is_empty() {
            p.muted { "No manuscripts yet." }
        } @else {
            ol.ms-list {
                @for (i, m) in rows.iter().enumerate() {
                    (manuscript_row(ctx, m, i + 1, logged_in))
                }
            }
        }
    };
    layout(&format!("@{}", u.username), ctx, body)
}
