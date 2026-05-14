//! /m/{id}/versions list + /m/{id}/v/{n} single historical version.

use maud::{html, Markup, PreEscaped};

use super::layout::{layout, time_ago, PageCtx};
use crate::markdown;
use crate::models::{Manuscript, ManuscriptVersion};

fn slug_for(m: &Manuscript) -> String {
    m.arxiv_like_id.clone().unwrap_or_else(|| m.id.to_string())
}

fn public_slug_for(m: &Manuscript) -> String {
    let slug = slug_for(m);
    slug.strip_prefix("prexiv:").unwrap_or(&slug).to_string()
}

pub fn render_list(ctx: &PageCtx, m: &Manuscript, versions: &[ManuscriptVersion]) -> Markup {
    let slug = slug_for(m);
    let public_slug = public_slug_for(m);
    let body = html! {
        div.page-header {
            h1 {
                "Versions of "
                a href={ "/abs/" (public_slug) } { code.no-katex { (slug) } }
            }
            p.muted {
                (versions.len())
                @if versions.len() == 1 { " version" } @else { " versions" }
                " on file. Each row links to a permalink for that specific snapshot; the page at "
                code.no-katex { "/abs/" (public_slug) }
                " always shows the latest."
            }
        }

        ol.version-list reversed {
            @for v in versions {
                li.version-row {
                    div.version-row-head {
                        span.version-row-tag.no-katex {
                            "v" (v.version_number)
                        }
                        @if v.version_number == m.current_version {
                            span.version-row-current { "current" }
                        }
                        @if let Some(t) = v.revised_at {
                            span.version-row-time { (time_ago(&t)) " \u{2013} " (t.format("%Y-%m-%d")) }
                        }
                        span.version-row-spacer {}
                        @if v.version_number > 1 {
                            a.btn-secondary.btn-small href={ "/m/" (slug) "/diff/" (v.version_number - 1) "/" (v.version_number) }
                              title={ "Diff v" (v.version_number - 1) " → v" (v.version_number) } {
                                "Diff vs v" (v.version_number - 1)
                            }
                        }
                        @if v.version_number == m.current_version {
                            a.btn-secondary.btn-small href={ "/abs/" (public_slug) } { "View latest" }
                        } @else {
                            a.btn-secondary.btn-small href={ "/m/" (slug) "/v/" (v.version_number) } { "View v" (v.version_number) }
                        }
                    }
                    @if let Some(note) = &v.revision_note {
                        p.version-row-note { (note) }
                    } @else if v.version_number == 1 {
                        p.version-row-note.muted { "Original submission." }
                    }
                }
            }
        }

        p style="margin-top:24px" {
            a.btn-secondary href={ "/abs/" (public_slug) } { "\u{2190} Back to manuscript" }
        }
    };
    layout(&format!("Versions of {slug}"), ctx, body)
}

pub fn render_version(ctx: &PageCtx, m: &Manuscript, v: &ManuscriptVersion) -> Markup {
    let slug = slug_for(m);
    let public_slug = public_slug_for(m);
    let body = html! {
        div.historical-banner role="status" {
            div.verify-banner-text {
                strong { "Historical version." }
                " You're viewing "
                strong { "v" (v.version_number) }
                " of "
                code.no-katex { (slug) }
                ", which has since been revised. The latest is "
                strong { "v" (m.current_version) }
                "."
                @if let Some(t) = v.revised_at {
                    " This version was published " (time_ago(&t)) "."
                }
            }
            div.verify-banner-actions {
                a.btn-primary href={ "/abs/" (public_slug) } { "View latest \u{2192}" }
                a.btn-secondary href={ "/m/" (slug) "/versions" } { "All versions" }
            }
        }

        article.manuscript {
            span.bx-eyebrow { "Historical v" (v.version_number) }
            h1.ms-h1 { (PreEscaped(markdown::render_inline(&v.title))) }
            p.ms-authors-line { (v.authors) }
            p.muted.small.mono {
                "doi: " @if let Some(doi) = &m.doi { (doi) } @else { "\u{2014}" }
                " \u{00b7} category: " (v.category)
                @if let Some(note) = &v.revision_note {
                    " \u{00b7} revision note: \u{201c}" (note) "\u{201d}"
                }
            }

            section.ms-section {
                h2.ms-section-h { "Abstract \u{2014} v" (v.version_number) }
                div.ms-abstract.markdown { (PreEscaped(markdown::render(&v.r#abstract))) }
            }

            @if let Some(path) = &v.pdf_path {
                section.ms-section {
                    h2.ms-section-h { "PDF \u{2014} v" (v.version_number) }
                    p {
                        a.btn-secondary href={ "/static/uploads/" (path) } target="_blank" rel="noopener" {
                            "\u{2193} Download v" (v.version_number) " PDF"
                        }
                    }
                }
            }
            @if let Some(url) = &v.external_url {
                section.ms-section {
                    h2.ms-section-h { "External URL \u{2014} v" (v.version_number) }
                    p {
                        a href=(url) rel="nofollow ugc noopener" target="_blank" { (url) }
                    }
                }
            }
            @if let Some(notes) = &v.conductor_notes {
                section.ms-section {
                    h2.ms-section-h { "Conductor notes \u{2014} v" (v.version_number) }
                    div.markdown { (PreEscaped(markdown::render(notes))) }
                }
            }
        }
    };
    layout(&format!("v{} of {}", v.version_number, slug), ctx, body)
}
