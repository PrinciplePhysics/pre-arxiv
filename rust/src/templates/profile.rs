use maud::{html, Markup, PreEscaped};

use crate::models::{ManuscriptListItem, User};

use super::home::manuscript_row;
use super::layout::{layout, PageCtx};
use crate::routes::profile::ProfileStats;

/// ORCID iD official mark — green circle with "iD" lettering plus the
/// signature green-dot tittle on the lowercase i. Public-domain
/// vector from the ORCID brand pack; we inline rather than load
/// from a CDN so the badge renders even when the user is offline or
/// pub.orcid.org is unreachable. `aria-hidden` so screen readers
/// rely on the parent anchor's aria-label instead.
const ORCID_BADGE_SVG: &str = r##"<svg viewBox="0 0 256 256" xmlns="http://www.w3.org/2000/svg" aria-hidden="true"><circle cx="128" cy="128" r="128" fill="#A6CE39"/><g fill="#fff"><path d="M86.3 186.2H70.9V79.1h15.4v107.1z"/><path d="M108.9 79.1h41.6c39.6 0 57 28.3 57 53.6 0 27.5-21.5 53.6-56.8 53.6h-41.8V79.1zm15.4 93.3h24.5c34.9 0 42.9-26.5 42.9-39.7 0-21.5-13.7-39.7-43.7-39.7h-23.7v79.4z"/><circle cx="78.6" cy="56.8" r="10.1"/></g></svg>"##;

/// Institutional-email badge — slate-blue circle, white classical
/// columns (a pediment on five fluted columns). Reads as "this person
/// is at an institution." Drawn fresh rather than borrowing an icon
/// font so nothing extra has to load.
const INST_BADGE_SVG: &str = r##"<svg viewBox="0 0 256 256" xmlns="http://www.w3.org/2000/svg" aria-hidden="true"><circle cx="128" cy="128" r="128" fill="#3a6f9c"/><g fill="#fff"><path d="M128 60 L62 96 L194 96 Z"/><rect x="58" y="100" width="140" height="9"/><rect x="68" y="114" width="12" height="56"/><rect x="92" y="114" width="12" height="56"/><rect x="116" y="114" width="12" height="56"/><rect x="140" y="114" width="12" height="56"/><rect x="164" y="114" width="12" height="56"/><rect x="58" y="174" width="140" height="11"/><rect x="50" y="188" width="156" height="6"/></g></svg>"##;

/// GitHub account-control badge. We avoid external brand assets and
/// render a compact "GH" mark so the profile page has no remote
/// dependencies and no icon-font dependency.
const GITHUB_BADGE_SVG: &str = r##"<svg viewBox="0 0 256 256" xmlns="http://www.w3.org/2000/svg" aria-hidden="true"><circle cx="128" cy="128" r="128" fill="#24292f"/><text x="128" y="149" text-anchor="middle" font-family="Arial, Helvetica, sans-serif" font-size="86" font-weight="700" fill="#fff" letter-spacing="-6">GH</text></svg>"##;

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
    let real_name: &str = u
        .display_name
        .as_deref()
        .filter(|s| !s.trim().is_empty())
        .unwrap_or(&u.username);
    let has_display_name = u
        .display_name
        .as_deref()
        .map(|s| !s.trim().is_empty())
        .unwrap_or(false);
    let body = html! {
        header.profile-card {
            div.profile-card-id {
                div.profile-name-row {
                    h1.profile-name { (real_name) }
                    @if u.is_orcid_oauth_verified() || u.is_verified_scholar() || u.is_github_oauth_verified() {
                        span.profile-name-badges aria-label="Profile trust signals" {
                            @if u.is_orcid_oauth_verified() {
                                @let orcid = u.orcid.as_deref().unwrap_or("");
                                @let orcid_url = format!("https://orcid.org/{orcid}");
                                @let orcid_title = format!("ORCID authenticated · {orcid}");
                                @let orcid_aria = format!("ORCID authenticated: {orcid}");
                                a.profile-name-badge.is-orcid
                                  href=(orcid_url)
                                  target="_blank" rel="noopener me"
                                  title=(orcid_title)
                                  aria-label=(orcid_aria) {
                                    (PreEscaped(ORCID_BADGE_SVG))
                                }
                            }
                            @if u.is_github_oauth_verified() {
                                @let gh_login = u.github_login.as_deref().unwrap_or("");
                                @let gh_title = if gh_login.is_empty() {
                                    "GitHub account verified".to_string()
                                } else {
                                    format!("GitHub account verified · @{gh_login}")
                                };
                                @let gh_aria = if gh_login.is_empty() {
                                    "GitHub account verified".to_string()
                                } else {
                                    format!("GitHub account verified: @{gh_login}")
                                };
                                @if gh_login.is_empty() {
                                    span.profile-name-badge.is-github
                                      title=(gh_title)
                                      aria-label=(gh_aria) {
                                        (PreEscaped(GITHUB_BADGE_SVG))
                                    }
                                } @else {
                                    @let gh_url = format!("https://github.com/{gh_login}");
                                    a.profile-name-badge.is-github
                                      href=(gh_url)
                                      target="_blank" rel="noopener me"
                                      title=(gh_title)
                                      aria-label=(gh_aria) {
                                        (PreEscaped(GITHUB_BADGE_SVG))
                                    }
                                }
                            }
                            @if u.is_verified_scholar() {
                                span.profile-name-badge.is-inst
                                  title="Verified ownership of an institutional / R&D-org email domain"
                                  aria-label="Verified institutional email" {
                                    (PreEscaped(INST_BADGE_SVG))
                                }
                            }
                        }
                    }
                }
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
