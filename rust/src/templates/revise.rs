//! /m/{id}/revise — revision form (v{N+1}).

use maud::{Markup, PreEscaped, html};

use super::layout::{PageCtx, layout};
use crate::categories::{self as cats};
use crate::models::Manuscript;

const UPLOAD_ICON_SVG: &str = r##"<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.7" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="M12 3.5v11"/><path d="m7.5 8 4.5-4.5L16.5 8"/><path d="M4.5 14.5v3.25A2.25 2.25 0 0 0 6.75 20h10.5a2.25 2.25 0 0 0 2.25-2.25V14.5"/></svg>"##;

pub fn render(ctx: &PageCtx, m: &Manuscript, error: Option<&str>) -> Markup {
    let slug = m.arxiv_like_id.clone().unwrap_or_else(|| m.id.to_string());
    let next_version = m.current_version + 1;
    let ai_model_public = m.conductor_ai_model_public != 0;
    let human_public = m.conductor_human_public != 0;
    let has_human_conductor = m.conductor_type == "human-ai" && m.conductor_human.is_some();
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
                        @for g in cats::GROUPS {
                            optgroup label=(g) {
                                @for c in cats::in_group(g) {
                                    option value=(c.id) selected[m.category == c.id] {
                                        (c.id) " \u{2014} " (c.name)
                                    }
                                }
                            }
                        }
                    }
                    span.hint.no-katex {
                        "Grouped by subject area, matching the submit form. Pick the most specific category that still honestly fits the revised manuscript."
                    }
                }

                div.revision-artifacts {
                    div.revision-artifact-heading {
                        span.label-text { "Manuscript files" }
                        span.muted.small {
                            @if m.pdf_path.is_some() || m.source_path.is_some() {
                                "Current stored artifact: "
                                @if m.source_path.is_some() { strong { "LaTeX source + compiled PDF" } }
                                @else { strong { "PDF" } }
                            } @else {
                                "No stored PDF/source artifact."
                            }
                        }
                    }

                    div.revision-upload-grid {
                        div.field {
                            p.label-text { "Replacement LaTeX source " span.muted { "(optional)" } }
                            div.upload-dropzone data-bound-name="revision-source-name" {
                                input #source_upload.upload-input type="file" name="source"
                                      accept=".tex,.zip,.tar.gz,.tgz,application/x-tex,application/zip,application/gzip,application/x-gzip";
                                label.upload-target for="source_upload" {
                                    span.upload-icon aria-hidden="true" { (PreEscaped(UPLOAD_ICON_SVG)) }
                                    span.upload-copy {
                                        strong.upload-prompt { "Choose source, or drop it here" }
                                        span.upload-filename #revision-source-name data-empty="No replacement source selected" {
                                            "No replacement source selected"
                                        }
                                    }
                                    span.upload-button { "Browse" }
                                }
                            }
                            span.hint.no-katex {
                                "Upload "
                                code { ".tex" } ", " code { ".zip" } ", or " code { ".tar.gz" }
                                ". Required when changing a public conductor/model field to private, because PreXiv must regenerate the blacked-out source and PDF."
                            }
                        }

                        div.field {
                            p.label-text { "Replacement PDF " span.muted { "(optional)" } }
                            div.upload-dropzone data-bound-name="revision-pdf-name" {
                                input #pdf_upload.upload-input type="file" name="pdf" accept="application/pdf";
                                label.upload-target for="pdf_upload" {
                                    span.upload-icon aria-hidden="true" { (PreEscaped(UPLOAD_ICON_SVG)) }
                                    span.upload-copy {
                                        strong.upload-prompt { "Choose PDF, or drop it here" }
                                        span.upload-filename #revision-pdf-name data-empty="No replacement PDF selected" {
                                            "No replacement PDF selected"
                                        }
                                    }
                                    span.upload-button { "Browse" }
                                }
                            }
                            span.hint.no-katex {
                                "PDF only, up to 30 MB. Direct PDF replacement is available only when public conductor/model fields stay public; private fields need source-based redaction."
                            }
                        }
                    }

                    div.revision-url-row {
                        label.revision-url-field {
                            span.label-text { "External URL " span.muted { "(optional)" } }
                            input type="url" name="external_url" maxlength="500" value=(m.external_url.clone().unwrap_or_default())
                                  placeholder="https://\u{2026}";
                            span.hint.no-katex { "Canonical hosted copy elsewhere (arXiv, GitHub, journal site). Readers see this link alongside any stored PDF/source." }
                        }
                    }
                    @if m.pdf_path.is_some() || m.source_path.is_some() {
                        div.revision-artifact-actions {
                            div.revision-artifact-action-copy {
                                strong { "Stored PreXiv artifact" }
                                span.muted.small { " Leave this alone to keep the current PDF/source downloads." }
                            }
                            label.checkbox.revision-remove-artifacts {
                                input type="checkbox" name="remove_pdf" value="1";
                                span {
                                    "Remove stored PDF/source"
                                    span.muted.small { " and rely on External URL only" }
                                }
                            }
                        }
                    }
                }

                label {
                    span.label-text { "Conductor notes " span.muted { "(optional, public)" } }
                    textarea name="conductor_notes" maxlength="3000" rows="4" { (m.conductor_notes.clone().unwrap_or_default()) }
                    span.hint.no-katex { "Free-form note about how this version was produced, the AI's role, or anything readers should know about provenance. Markdown supported." }
                }
            }

            section.form-section {
                h2 { "3 — Disclosure" }
                p.muted.small {
                    "These controls change what public readers and API clients see for this version onward. Submitters and admins can still see the stored conductor values."
                }
                div.disclosure-options {
                    label.checkbox-inline.disclosure-choice {
                        input type="hidden" name="conductor_ai_model_public" value="0";
                        input type="checkbox" name="conductor_ai_model_public" value="1" checked[ai_model_public];
                        span {
                            strong { "Show AI model(s) publicly" }
                            " — if unchecked, readers see " em { "(undisclosed)" } "."
                        }
                    }
                    @if has_human_conductor {
                        label.checkbox-inline.disclosure-choice {
                            input type="hidden" name="conductor_human_public" value="0";
                            input type="checkbox" name="conductor_human_public" value="1" checked[human_public];
                            span {
                                strong { "Show human conductor publicly" }
                                " — if unchecked, readers see " em { "(undisclosed)" } "."
                            }
                        }
                    } @else {
                        input type="hidden" name="conductor_human_public" value=(if human_public { "1" } else { "0" });
                    }
                }
                div.disclosure-redaction-note.no-katex {
                    strong { "Privacy rule:" }
                    " if you turn a previously public name/model private while stored artifacts remain on PreXiv, upload replacement LaTeX source. PreXiv will black out the source before compiling and will serve only the blacked-out source/PDF."
                }
            }

            section.form-section {
                h2 { "4 — Licensing" }
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
                "Conductor identity, audit status, and the manuscript id are immutable across versions; disclosure flags can be changed here. To change the underlying conductor or auditor, withdraw this and submit anew. To view earlier versions after revising, visit "
                a href={ "/m/" (slug) "/versions" } { "/m/" (slug) "/versions" }
                "."
            }
        }
        script { (PreEscaped(r#"
(function(){
  document.querySelectorAll('.upload-dropzone').forEach(function(zone){
    var inp = zone.querySelector('.upload-input');
    var out = document.getElementById(zone.dataset.boundName);
    if(!inp || !out) return;
    var empty = out.dataset.empty || 'No file selected';
    inp.addEventListener('change', function(){
      var f = inp.files && inp.files[0];
      if(f){
        out.textContent = f.name + ' · ' + (f.size/1024/1024).toFixed(2) + ' MB';
        out.classList.add('has-file');
      } else {
        out.textContent = empty;
        out.classList.remove('has-file');
      }
    });
  });
})();
"#)) }
    };
    layout(&format!("Revise {slug}"), ctx, body)
}
