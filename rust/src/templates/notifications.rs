//! /me/notifications listing.

use maud::{html, Markup};

use super::layout::{layout, time_ago, PageCtx};
use crate::notifications::{
    NotificationRow, KIND_COMMENT_ON_MY_MANUSCRIPT, KIND_FOLLOWED, KIND_REPLY_TO_MY_COMMENT,
};

pub fn render(ctx: &PageCtx, rows: &[NotificationRow]) -> Markup {
    let unread = rows.iter().filter(|r| r.read_at.is_none()).count();
    let body = html! {
        div.page-header {
            h1 { "Notifications" }
            p.muted {
                (unread)
                @if unread == 1 { " unread, " } @else { " unread, " }
                (rows.len())
                " total. Unread first."
            }
        }

        @if unread > 0 {
            form method="post" action="/me/notifications/mark-all-read" style="margin-bottom:16px" {
                input type="hidden" name="csrf_token" value=(ctx.csrf_token);
                button.btn-secondary.btn-small type="submit" { "Mark all read" }
            }
        }

        @if rows.is_empty() {
            div.empty {
                p { "Nothing here yet." }
                p.muted { "When someone comments on your manuscripts, replies to your comments, or follows you, it'll show up here." }
            }
        } @else {
            ul.notification-list {
                @for n in rows {
                    li.notification-row.notification-unread[n.read_at.is_none()] {
                        div.notification-body {
                            (notification_text(n))
                            @if let Some(ts) = &n.created_at {
                                " " span.notification-time { (time_ago(ts)) }
                            }
                        }
                        @if n.read_at.is_none() {
                            form method="post" action={ "/me/notifications/" (n.id) "/read" } {
                                input type="hidden" name="csrf_token" value=(ctx.csrf_token);
                                button.btn-secondary.btn-small type="submit" title="Mark as read" { "✓" }
                            }
                        }
                    }
                }
            }
        }
    };
    layout("Notifications", ctx, body)
}

fn notification_text(n: &NotificationRow) -> Markup {
    let actor = n.actor_display.as_deref()
        .or(n.actor_username.as_deref())
        .unwrap_or("Someone");
    let actor_link = match &n.actor_username {
        Some(u) => Some(format!("/u/{u}")),
        None => None,
    };
    let target_slug = n.target_slug.as_deref().unwrap_or("");
    let target_title = n.target_title.as_deref().unwrap_or("a manuscript");

    let target_url = match n.target_type.as_deref() {
        Some("manuscript") if !target_slug.is_empty() => Some(format!("/m/{target_slug}")),
        Some("comment")    if !target_slug.is_empty() && n.target_id.is_some() =>
            Some(format!("/m/{target_slug}#comment-{}", n.target_id.unwrap())),
        Some("user") => actor_link.clone(),
        _ => None,
    };

    html! {
        @match n.kind.as_str() {
            k if k == KIND_COMMENT_ON_MY_MANUSCRIPT => {
                @if let Some(url) = &actor_link { a href=(url) { strong { (actor) } } }
                @else { strong { (actor) } }
                " commented on your manuscript "
                @if let Some(url) = &target_url {
                    a href=(url) { em { (target_title) } }
                } @else { em { (target_title) } }
            }
            k if k == KIND_REPLY_TO_MY_COMMENT => {
                @if let Some(url) = &actor_link { a href=(url) { strong { (actor) } } }
                @else { strong { (actor) } }
                " replied to your comment on "
                @if let Some(url) = &target_url {
                    a href=(url) { em { (target_title) } }
                } @else { em { (target_title) } }
            }
            k if k == KIND_FOLLOWED => {
                @if let Some(url) = &actor_link { a href=(url) { strong { (actor) } } }
                @else { strong { (actor) } }
                " started following you"
            }
            _ => {
                strong { (actor) }
                " · "
                code { (n.kind) }
            }
        }
        @if let Some(snippet) = &n.detail {
            div.notification-detail { "“" (snippet) "”" }
        }
    }
}
