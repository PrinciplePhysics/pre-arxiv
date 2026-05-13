//! Unified line-by-line diff between two ManuscriptVersion snapshots.

use maud::{html, Markup, PreEscaped};
use similar::{ChangeTag, TextDiff};

use super::layout::{layout, time_ago, PageCtx};
use crate::models::{Manuscript, ManuscriptVersion};

fn slug_for(m: &Manuscript) -> String {
    m.arxiv_like_id.clone().unwrap_or_else(|| m.id.to_string())
}

pub fn render(
    ctx: &PageCtx,
    m: &Manuscript,
    left: &ManuscriptVersion,
    right: &ManuscriptVersion,
) -> Markup {
    let slug = slug_for(m);
    let body = html! {
        div.page-header {
            h1 {
                "Diff: "
                code.no-katex { (slug) }
                " "
                span.muted { "v" (left.version_number) " → v" (right.version_number) }
            }
            p.muted {
                "Line-by-line differences between the two snapshots. Insertions are "
                span.diff-added-inline { "green" }
                "; removals are "
                span.diff-removed-inline { "amber" }
                ". Unchanged context is dimmed."
            }
        }

        div.diff-meta {
            div.diff-meta-version {
                div.diff-meta-tag.diff-meta-left  { "v" (left.version_number) }
                @if let Some(t) = left.revised_at { div.muted.small { (time_ago(&t)) } }
                @if let Some(note) = &left.revision_note { div.muted.small.diff-revision-note { "\u{201c}" (note) "\u{201d}" } }
            }
            div.diff-meta-arrow aria-hidden="true" { "→" }
            div.diff-meta-version {
                div.diff-meta-tag.diff-meta-right { "v" (right.version_number) }
                @if let Some(t) = right.revised_at { div.muted.small { (time_ago(&t)) } }
                @if let Some(note) = &right.revision_note { div.muted.small.diff-revision-note { "\u{201c}" (note) "\u{201d}" } }
            }
        }

        (field_diff("Title",     &left.title,                  &right.title,                  false))
        (field_diff("Abstract",  &left.r#abstract,             &right.r#abstract,             true))
        (field_diff("Authors",   &left.authors,                &right.authors,                false))
        (field_diff("Category",  &left.category,               &right.category,               false))
        (field_diff("License",   left.license.as_deref().unwrap_or(""),     right.license.as_deref().unwrap_or(""),     false))
        (field_diff("AI-training", left.ai_training.as_deref().unwrap_or(""), right.ai_training.as_deref().unwrap_or(""), false))
        (field_diff("External URL", left.external_url.as_deref().unwrap_or(""), right.external_url.as_deref().unwrap_or(""), false))
        (field_diff("PDF path",  left.pdf_path.as_deref().unwrap_or(""),    right.pdf_path.as_deref().unwrap_or(""),    false))
        (field_diff("Conductor notes",
                    left.conductor_notes.as_deref().unwrap_or(""),
                    right.conductor_notes.as_deref().unwrap_or(""),
                    true))

        p style="margin-top:32px" {
            a.btn-secondary href={ "/m/" (slug) "/versions" } { "← All versions" }
            " "
            a.btn-secondary href={ "/m/" (slug) } { "Latest" }
        }
    };
    layout(
        &format!(
            "Diff v{}→v{} · {}",
            left.version_number, right.version_number, slug
        ),
        ctx,
        body,
    )
}

fn field_diff(label: &str, left: &str, right: &str, prose: bool) -> Markup {
    let unchanged = left == right;
    html! {
        section.diff-section {
            h2.diff-section-h {
                (label)
                @if unchanged { span.diff-unchanged-pill { "unchanged" } }
            }
            @if unchanged {
                @if !left.is_empty() {
                    pre.diff-context.no-katex { (left) }
                } @else {
                    p.muted.small.no-katex { "(empty in both versions)" }
                }
            } @else {
                pre.diff-block.no-katex { (PreEscaped(render_diff(left, right, prose))) }
            }
        }
    }
}

fn html_escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

fn render_diff(left: &str, right: &str, prose: bool) -> String {
    // For prose (abstracts, conductor notes) we want word-level
    // granularity; for shorter fields, character-level is too noisy
    // and line-level is fine.
    let diff = TextDiff::from_lines(left, right);
    let mut out = String::new();
    for change in diff.iter_all_changes() {
        let (cls, prefix) = match change.tag() {
            ChangeTag::Delete => ("diff-removed", "-"),
            ChangeTag::Insert => ("diff-added", "+"),
            ChangeTag::Equal => ("diff-context", " "),
        };
        out.push_str(&format!(
            "<span class=\"{cls}\">{prefix} {}</span>",
            html_escape(change.value().trim_end_matches('\n'))
        ));
        out.push('\n');
    }
    let _ = prose; // reserved for future char/word-level mode
    out
}
