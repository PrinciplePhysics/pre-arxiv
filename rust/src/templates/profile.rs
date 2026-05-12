use maud::{html, Markup};

use crate::models::{ManuscriptListItem, User};

use super::home::manuscript_row;
use super::layout::{layout, PageCtx};
use crate::routes::profile::ProfileStats;

pub fn render(
    ctx: &PageCtx,
    u: &User,
    rows: &[ManuscriptListItem],
    stats: &ProfileStats,
) -> Markup {
    let logged_in = ctx.user.is_some();
    let viewer_is_self = ctx.user.as_ref().map(|v| v.id == u.id).unwrap_or(false);

    let body = html! {
        div.page-header {
            h1 { "@" (u.username) }
            @if let Some(d) = &u.display_name {
                p.muted { (d) }
            }
            p.muted {
                "karma " (u.karma.unwrap_or(0))
                " · " (rows.len()) " manuscripts"
                " · " (stats.follower_count) " "
                @if stats.follower_count == 1 { "follower" } @else { "followers" }
                " · following " (stats.following_count)
                @if u.is_admin() { " · " span.role-tag { "admin" } }
            }
            @if let Some(b) = &u.bio { p { (b) } }
            @if let Some(a) = &u.affiliation { p.muted { "Affiliation: " (a) } }

            div style="display:flex;gap:8px;margin-top:8px" {
                @if viewer_is_self {
                    a.btn-secondary href="/me/edit" { "Edit profile" }
                } @else if logged_in {
                    @if stats.viewer_follows {
                        form action={"/u/" (u.username) "/unfollow"} method="post" style="display:inline" {
                            input type="hidden" name="csrf_token" value=(ctx.csrf_token);
                            button.btn-secondary type="submit" { "✓ Following — unfollow" }
                        }
                    } @else {
                        form action={"/u/" (u.username) "/follow"} method="post" style="display:inline" {
                            input type="hidden" name="csrf_token" value=(ctx.csrf_token);
                            button.btn-primary type="submit" { "+ Follow" }
                        }
                    }
                } @else {
                    a.btn-secondary href={ "/login?next=/u/" (u.username) } { "Sign in to follow" }
                }
            }
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
