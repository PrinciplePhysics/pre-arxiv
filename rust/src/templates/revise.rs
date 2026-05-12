//! /m/{id}/revise — revision form (v{N+1}).

use maud::{html, Markup};

use super::layout::{layout, PageCtx};
use crate::models::Manuscript;

pub fn render(ctx: &PageCtx, m: &Manuscript, error: Option<&str>) -> Markup {
    let slug = m.arxiv_like_id.clone().unwrap_or_else(|| m.id.to_string());
    let next_version = m.current_version + 1;
    let body = html! {
        div.page-header {
            h1 {
                "Revise "
                code.no-katex { (slug) }
                " "
                span.muted { "→ v" (next_version) }
            }
            p.muted {
                "Replace the displayed values, write a one-line revision note, and submit. The current version (v"
                (m.current_version)
                ") stays in the archive and remains viewable; readers visiting "
                code.no-katex { (slug) }
                " will see v"
                (next_version)
                " by default."
            }
        }

        @if let Some(e) = error {
            div.form-errors {
                strong { "Please fix:" }
                ul { li { (e) } }
            }
        }

        form.submit-form method="post" action={ "/m/" (slug) "/revise" } enctype="multipart/form-data" {
            input type="hidden" name="csrf_token" value=(ctx.csrf_token);

            section.form-section {
                h2 { "1 — The revision" }

                label {
                    span.label-text { "Revision note " span.req { "*" } }
                    input type="text" name="revision_note" required maxlength="500"
                          placeholder="e.g. Fixed sign error in (3.2); rewrote proof of Theorem 2.1; added Section 4 on stability.";
                    span.hint { "One short line; will appear on the version history. Mandatory \u{2014} keeps the archive legible." }
                }
            }

            section.form-section {
                h2 { "2 — Manuscript" }

                label {
                    span.label-text { "Title " span.req { "*" } }
                    input type="text" name="title" required maxlength="500" value=(m.title);
                    span.hint.no-katex { "Plain text or markdown with inline math ($\u{2026}$)." }
                }

                label {
                    span.label-text { "Abstract " span.req { "*" } }
                    textarea name="abstract" required minlength="100" maxlength="5000" rows="10" { (m.r#abstract) }
                    span.hint.no-katex { "100\u{2013}5000 characters. Markdown + LaTeX math supported." }
                }

                label {
                    span.label-text { "Authors " span.req { "*" } }
                    input type="text" name="authors" required maxlength="500" value=(m.authors);
                    span.hint { "Comma-separated. The change to authorship across versions should be reflected in the revision note." }
                }

                label {
                    span.label-text { "Category " span.req { "*" } }
                    select name="category" required {
                        @for cat in crate::categories::CATEGORIES {
                            option value=(cat.id) selected[m.category == cat.id] {
                                (cat.id) " \u{2014} " (cat.name)
                            }
                        }
                    }
                }

                div.row-fields {
                    div.grow.field {
                        label for="pdf_upload" { span.label-text { "Upload new PDF " span.muted { "(optional)" } } }
                        input id="pdf_upload" type="file" name="pdf" accept="application/pdf";
                        span.hint.no-katex {
                            @if let Some(p) = &m.pdf_path {
                                "Current: " code.no-katex { (p) } ". Leave empty to keep the existing PDF. Or check \"Remove\" below to delete the PDF entirely (you'd then rely on the external URL)."
                            } @else {
                                "No PDF currently attached. Upload one if you'd like to add it. PDF only, up to 30 MB."
                            }
                        }
                        @if m.pdf_path.is_some() {
                            label.checkbox style="margin-top:6px" {
                                input type="checkbox" name="remove_pdf" value="1";
                                " Remove the current PDF (don't replace, just remove it)"
                            }
                        }
                    }
                    label.grow {
                        span.label-text { "External URL " span.muted { "(optional)" } }
                        input type="url" name="external_url" maxlength="500" value=(m.external_url.clone().unwrap_or_default())
                              placeholder="https://\u{2026}";
                        span.hint.no-katex { "For hosted-elsewhere copies (arXiv, GitHub, journal site)." }
                    }
                }

                label {
                    span.label-text { "Conductor notes " span.muted { "(optional, public)" } }
                    textarea name="conductor_notes" maxlength="3000" rows="4" { (m.conductor_notes.clone().unwrap_or_default()) }
                    span.hint.no-katex { "Free-form note about how this version was produced, the AI's role, or anything readers should know about provenance. Markdown supported." }
                }
            }

            section.form-section {
                h2 { "3 — Licensing" }
                p.muted.small { "Defaults to the existing values. Change only if the licensing terms have actually changed across this version." }
                div.row-fields {
                    label.grow {
                        span.label-text { "Reader license" }
                        select name="license" {
                            @for l in crate::licenses::LICENSES {
                                option value=(l.id) selected[m.license.as_deref() == Some(l.id)] {
                                    (l.tagline)
                                }
                            }
                        }
                    }
                    label.grow {
                        span.label-text { "AI-training opt-in" }
                        select name="ai_training" {
                            @for ai in crate::licenses::AI_TRAINING_OPTIONS {
                                option value=(ai.id) selected[m.ai_training.as_deref() == Some(ai.id)] {
                                    (ai.short) " \u{2014} " (ai.id)
                                }
                            }
                        }
                    }
                }
            }

            div.form-submit {
                button.btn-primary.big type="submit" {
                    "Publish v" (next_version)
                }
                " "
                a.btn-secondary href={ "/m/" (slug) } { "Cancel" }
            }

            p.muted.small style="margin-top:18px" {
                "Conductor identity, audit status, and the manuscript id are immutable across versions. To change those you'd withdraw this and submit anew. To view earlier versions after revising, visit "
                a href={ "/m/" (slug) "/versions" } { "/m/" (slug) "/versions" }
                "."
            }
        }
    };
    layout(&format!("Revise {slug}"), ctx, body)
}
