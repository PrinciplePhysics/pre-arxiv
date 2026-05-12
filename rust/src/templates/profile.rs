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

    // The real name (display_name) is the headline; the @username is
    // demoted to a small monospace handle below it. When display_name
    // is empty we fall back to the username as the headline so we
    // never render an anonymous-looking blank.
    let real_name: &str = u.display_name.as_deref().filter(|s| !s.trim().is_empty()).unwrap_or(&u.username);
    let has_display_name = u.display_name.as_deref().map(|s| !s.trim().is_empty()).unwrap_or(false);
    let body = html! {
        header.profile-card {
            div.profile-card-id {
                h1.profile-name { (real_name) }
                @if has_display_name {
                    p.profile-handle { "@" (u.username) }
                }
            }
            p.profile-stats {
                span.profile-stat { strong { (u.karma.unwrap_or(0)) } " karma" }
                span.profile-sep { "·" }
                span.profile-stat { strong { (rows.len()) } " manuscript" @if rows.len() != 1 { "s" } }
                span.profile-sep { "·" }
                span.profile-stat {
                    strong { (stats.follower_count) }
                    " "
                    @if stats.follower_count == 1 { "follower" } @else { "followers" }
                }
                span.profile-sep { "·" }
                span.profile-stat { "following " strong { (stats.following_count) } }
                @if u.is_admin() {
                    span.profile-sep { "·" }
                    span.role-tag { "admin" }
                }
            }
            @if let Some(b) = &u.bio {
                @if !b.trim().is_empty() {
                    p.profile-bio { (b) }
                }
            }
            @if let Some(a) = &u.affiliation {
                @if !a.trim().is_empty() {
                    p.profile-affiliation {
                        span.profile-affiliation-label { "Affiliation" }
                        " · " (a)
                    }
                }
            }
            @if u.is_verified_scholar() {
                div.profile-verified-badges {
                    @if u.is_orcid_verified() {
                        @let orcid = u.orcid.as_deref().unwrap_or("");
                        span.profile-vbadge title="ORCID iD verified — the public ORCID record's name matches this user's display name." {
                            "✓ ORCID"
                            @if !orcid.is_empty() {
                                " "
                                a.no-katex href={ "https://orcid.org/" (orcid) } target="_blank" rel="noopener" {
                                    (orcid)
                                }
                            }
                        }
                    }
                    @if u.is_institutional_email() {
                        span.profile-vbadge title="Verified email on an institutional / R&D-org domain." {
                            "✓ Institutional email"
                        }
                    }
                }
            }
            div.profile-actions {
                @if viewer_is_self {
                    a.btn-secondary href="/me/edit" { "Edit profile" }
                } @else if logged_in {
                    @if stats.viewer_follows {
                        form action={"/u/" (u.username) "/unfollow"} method="post" {
                            input type="hidden" name="csrf_token" value=(ctx.csrf_token);
                            button.btn-secondary type="submit" { "✓ Following — unfollow" }
                        }
                    } @else {
                        form action={"/u/" (u.username) "/follow"} method="post" {
                            input type="hidden" name="csrf_token" value=(ctx.csrf_token);
                            button.btn-primary type="submit" { "+ Follow" }
                        }
                    }
                } @else {
                    a.btn-secondary href={ "/login?next=/u/" (u.username) } { "Sign in to follow" }
                }
            }
        }
        h2.profile-section-h { "Submitted manuscripts" }
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
