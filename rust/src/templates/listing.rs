use maud::{html, Markup};

use crate::models::ManuscriptListItem;

use super::home::manuscript_row;
use super::layout::{layout, PageCtx};

#[allow(clippy::too_many_arguments)]
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
    let audited = rows.iter().filter(|m| m.has_auditor != 0).count();
    let autonomous = rows
        .iter()
        .filter(|m| m.conductor_type == "ai-agent")
        .count();
    let third_party = rows
        .iter()
        .filter(|m| {
            matches!(
                m.audit_status(),
                crate::models::manuscript::AuditStatus::ThirdParty
            )
        })
        .count();
    let body = html! {
        div.page-header {
            h1 { (heading) }
            p.muted { (subheading) }
        }
        @if show_mode_toggle {
            (super::home::mode_toggle(self_path, show_all))
        }
        div.browse-overview aria-label="Listing summary" {
            div.browse-stat {
                strong { (rows.len()) }
                span { "shown" }
            }
            div.browse-stat {
                strong { (audited) }
                span { "audited" }
            }
            div.browse-stat {
                strong { (third_party) }
                span { "third-party audits" }
            }
            div.browse-stat {
                strong { (autonomous) }
                span { "autonomous agents" }
            }
        }
        @if show_mode_toggle && !show_all {
            p.muted.small {
                "Default archive listings emphasize account-verified submitters and standard subject categories. "
                a href="/audited" { "Audited-only view" }
                " is available for reader triage."
            }
        }
        @if show_all && show_mode_toggle {
            (super::home::showing_all_banner(self_path))
        } @else if widened {
            (super::home::verified_widen_banner())
        }
        @if rows.is_empty() {
            div.empty {
                p { "No manuscripts match this listing yet." }
                p.muted.small {
                    "Try "
                    a href="/new?show_all=1" { "showing the full firehose" }
                    ", browsing by subject, or checking back after new submissions are posted."
                }
            }
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

pub fn render_browse(ctx: &PageCtx, counts: &[crate::routes::listings::BrowseCount]) -> Markup {
    let groups = crate::routes::listings::browse_groups(counts);
    let total: i64 = counts.iter().map(|c| c.total).sum();
    let new_this_week: i64 = counts.iter().map(|c| c.new_this_week).sum();
    let active_categories = counts.iter().filter(|c| c.total > 0).count();
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
                div.browse-stat {
                    strong { (new_this_week) }
                    span { "new this week" }
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
                                        span.muted.small {
                                            (category_description(e.id))
                                        }
                                        span.browse-cat-count {
                                            (e.count) " "
                                            @if e.count == 1 { "manuscript" } @else { "manuscripts" }
                                            @if e.new_this_week > 0 {
                                                " · " (e.new_this_week) " new this week"
                                            }
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

fn category_description(id: &str) -> &'static str {
    if id.starts_with("cs.") {
        "Computing, algorithms, systems, and AI-adjacent work."
    } else if id.starts_with("math.") {
        "Mathematical arguments, structures, proofs, and models."
    } else if id.starts_with("stat.") {
        "Statistical methods, inference, learning theory, and applications."
    } else if id.starts_with("physics.")
        || id.starts_with("astro-ph")
        || id.starts_with("hep-")
        || id == "gr-qc"
        || id == "quant-ph"
    {
        "Physical sciences, theory, experiment, and simulation."
    } else if id.starts_with("q-bio.") || id.starts_with("bio.") {
        "Biological systems, wet-lab adjacent reports, and quantitative biology."
    } else if id.starts_with("med.") {
        "Clinical, public-health, and biomedical manuscripts."
    } else if id.starts_with("econ.") || id.starts_with("q-fin.") {
        "Economics, markets, finance, and decision systems."
    } else {
        "Cross-disciplinary or legacy category."
    }
}
