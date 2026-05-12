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
    widened: bool,
) -> Markup {
    let logged_in = ctx.user.is_some();
    let body = html! {
        div.page-header {
            h1 { (heading) }
            p.muted { (subheading) }
        }
        @if widened {
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
    let body = html! {
        div.page-header {
            h1 { "Browse by category" }
            p.muted {
                "PreXiv's category taxonomy is layered: the "
                code { "cs.*" } " / " code { "math.*" } " / " code { "stat.*" } " / " code { "hep-*" } " / " code { "q-bio.*" } " / " code { "q-fin.*" } " / " code { "econ.*" }
                " ids are arXiv-canonical (semantic-identical to arXiv's). "
                code { "bio.*" }
                " mirrors bioRxiv's wet-biology subject areas. "
                code { "med.*" }
                " mirrors medRxiv's clinical and public-health areas. "
                (total) " manuscripts across the corpus right now."
            }
        }
        @if total == 0 {
            div.empty { p { "No manuscripts yet." } }
        } @else {
            @for (group_name, entries) in &groups {
                @let nonzero: Vec<&crate::routes::listings::BrowseEntry>
                    = entries.iter().filter(|e| e.count > 0).collect();
                @if !nonzero.is_empty() {
                    section.ms-section {
                        h2.ms-section-h { (group_name) }
                        ul.category-index style="margin:6px 0 0;padding:0;list-style:none;display:flex;flex-wrap:wrap;gap:8px 16px" {
                            @for e in &nonzero {
                                li {
                                    a.ms-cat-pill href={ "/browse/" (e.id) } { (e.id) }
                                    " "
                                    span.muted { "(" (e.count) ")" }
                                }
                            }
                        }
                    }
                }
            }
            p.muted.small style="margin-top:24px" {
                "Empty categories — those with zero current manuscripts — are hidden on this page. The full taxonomy (~85 categories across "
                (groups.len())
                " domains) is available on the submit form at "
                a href="/submit" { "/submit" }
                " and via "
                a href="/api/v1/categories" { "/api/v1/categories" }
                "."
            }
        }
    };
    layout("Browse", ctx, body)
}
