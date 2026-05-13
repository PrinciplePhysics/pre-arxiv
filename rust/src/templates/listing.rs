use maud::{html, Markup};

use crate::models::ManuscriptListItem;

use super::home::manuscript_row;
use super::layout::{layout, PageCtx};

pub fn render(
    ctx: &PageCtx,
    heading: &str,
    subheading: &str,
    rows: &[ManuscriptListItem],
    self_path: &str,
    widened: bool,
    show_all: bool,
    show_mode_toggle: bool,
) -> Markup {
    let logged_in = ctx.user.is_some();
    let body = html! {
        div.page-header {
            h1 { (heading) }
            p.muted { (subheading) }
        }
        @if show_mode_toggle {
            (super::home::mode_toggle(self_path, show_all))
        }
        @if show_all && show_mode_toggle {
            (super::home::showing_all_banner(self_path))
        } @else if widened {
            (super::home::verified_widen_banner())
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
    let groups = crate::routes::listings::browse_groups(counts);
    let total: i64 = counts.iter().map(|(_, n)| n).sum();
    let active_categories = counts.iter().filter(|(_, n)| *n > 0).count();
    let active_domains = groups
        .iter()
        .filter(|(_, entries)| entries.iter().any(|e| e.count > 0))
        .count();
    let body = html! {
        div.page-header.browse-header {
            h1 { "Browse by category" }
            p.muted {
                "Find manuscripts by subject. Active categories are shown first within each domain; empty categories stay available on the submit form and API."
            }
        }
        @if total == 0 {
            div.empty { p { "No manuscripts yet." } }
        } @else {
            div.browse-overview aria-label="Corpus summary" {
                div.browse-stat {
                    strong { (total) }
                    span { "manuscripts" }
                }
                div.browse-stat {
                    strong { (active_categories) }
                    span { "active categories" }
                }
                div.browse-stat {
                    strong { (active_domains) }
                    span { "domains represented" }
                }
            }

            div.browse-taxonomy-note {
                span { "Taxonomy: " }
                code { "cs.*" } " / " code { "math.*" } " / " code { "stat.*" } " / " code { "hep-*" }
                " follow arXiv; "
                code { "bio.*" } " follows bioRxiv; "
                code { "med.*" } " follows medRxiv."
            }

            div.browse-domains {
                @for (group_name, entries) in &groups {
                    @let nonzero: Vec<&crate::routes::listings::BrowseEntry>
                        = entries.iter().filter(|e| e.count > 0).collect();
                    @if !nonzero.is_empty() {
                        @let group_total: i64 = nonzero.iter().map(|e| e.count).sum();
                        section.browse-domain {
                            div.browse-domain-head {
                                h2 { (group_name) }
                                span {
                                    (group_total) " "
                                    @if group_total == 1 { "manuscript" } @else { "manuscripts" }
                                }
                            }
                            div.browse-category-grid {
                                @for e in &nonzero {
                                    a.browse-category href={ "/browse/" (e.id) } {
                                        span.browse-cat-id { (e.id) }
                                        span.browse-cat-name { (e.name) }
                                        span.browse-cat-count {
                                            (e.count) " "
                                            @if e.count == 1 { "manuscript" } @else { "manuscripts" }
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }
            p.browse-footnote {
                "Full taxonomy: "
                a href="/submit" { "submit form" }
                " · "
                a href="/api/v1/categories" { "API categories" }
            }
        }
    };
    layout("Browse", ctx, body)
}
