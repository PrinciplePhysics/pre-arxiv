use maud::{html, Markup, PreEscaped};

use super::layout::{layout, PageCtx};
use crate::licenses::{AI_TRAINING_OPTIONS, LICENSES};

/// Upload-tray glyph. A simple stroked tray with an up-arrow on top —
/// reads as "upload" at any size, doesn't depend on an icon font, and
/// inherits `currentColor` so it follows the brand palette. Sized to 22px
/// via the CSS rule on `.upload-icon svg`.
const UPLOAD_ICON_SVG: &str = r##"<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.7" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="M12 3.5v11"/><path d="m7.5 8 4.5-4.5L16.5 8"/><path d="M4.5 14.5v3.25A2.25 2.25 0 0 0 6.75 20h10.5a2.25 2.25 0 0 0 2.25-2.25V14.5"/></svg>"##;

/// Top-tier flagships, current as of 2026-05-11. Surfaced as a
/// <datalist> (typeahead suggestions) rather than a hard <select> — model
/// names go stale fast and agents need to record precise version strings.
/// Newest within each lab first; superseded flagships kept because
/// historical submissions reference them.
const AI_MODELS: &[&str] = &[
    // Anthropic — Opus 4.7 GA'd April 16, 2026.
    "Claude Opus 4.7",
    "Claude Sonnet 4.6",
    "Claude Haiku 4.5",
    "Claude Opus 4.6",
    // OpenAI — GPT-5.5 family launched April/May 2026.
    "GPT-5.5 Pro",
    "GPT-5.5 Thinking",
    "GPT-5.5 Instant",
    "GPT-5",
    "o3",
    // Google — Gemini 3.1 Pro preview Feb 19, 2026.
    "Gemini 3.1 Pro",
    "Gemini 3 Pro",
    "Gemini 3 Flash",
    "Gemini 3.1 Flash-Lite",
    // xAI — Grok 4.20 Beta currently exposes 2M-token context.
    "Grok 4.20",
    "Grok 4",
    // DeepSeek — V4-Pro released April 24, 2026 (MIT-licensed).
    "DeepSeek V4-Pro",
    "DeepSeek V3.1",
    "DeepSeek R1",
    // Open-weights frontier
    "Llama 4 Maverick",
    "Llama 4 Scout",
    "Qwen 3.5",
    "GLM-5",
    "Mistral Large 3",
    // Multi-model provenance
    "Multiple (see notes)",
];

const ROLES: &[&str] = &[
    "undergraduate",
    "graduate-student",
    "postdoc",
    "industry-researcher",
    "professor",
    "professional-expert",
    "independent-researcher",
    "hobbyist",
];

use crate::categories::{self as cats};

pub fn render(ctx: &PageCtx, error: Option<&str>) -> Markup {
    let unverified = ctx
        .user
        .as_ref()
        .map(|u| !u.is_verified_or_admin())
        .unwrap_or(false);
    let email = ctx.user.as_ref().map(|u| u.email.as_str()).unwrap_or("");
    let body = html! {
        div.page-header {
            h1 { "Submit a manuscript" }
            p.muted {
                "A manuscript on PreXiv is a piece of work with substantial AI assistance or autonomous agent production. The "
                strong { "conductor" }
                " is the human who guided the AI (or, in autonomous mode, the authorized agent that produced it); the "
                strong { "auditor" }
                " (optional) is a human expert who has verified its correctness."
            }
        }

        @if unverified {
            (crate::templates::me_edit::verify_banner(&ctx.csrf_token, email, ctx.pending_verify_token.as_deref()))
        }

        @if let Some(e) = error {
            div.form-errors {
                strong { "Please fix the following:" }
                ul { li { (e) } }
            }
        }

        form.submit-form method="post" action="/submit" enctype="multipart/form-data" {
            input type="hidden" name="csrf_token" value=(ctx.csrf_token);

            section.form-section {
                h2 { "1 — The manuscript" }

                label {
                    span.label-text { "Title " span.req { "*" } }
                    input type="text" name="title" required maxlength="300"
                          placeholder="A descriptive title — Markdown + inline LaTeX OK";
                    span.hint.no-katex {
                        "Markdown ("
                        code { "*italic*" } ", "
                        code { "**bold**" } ", "
                        code { "`code`" } ") and inline LaTeX ("
                        code { "$E=mc^2$" }
                        ") render in listings and on the manuscript page."
                    }
                }

                label {
                    span.label-text { "Authors line " span.req { "*" } }
                    input type="text" name="authors" required maxlength="500"
                          placeholder="e.g., Jane Doe; Example Lab";
                    span.hint.no-katex { "Separate names with semicolons. Use humans or organizations here; disclose AI tools in the AI model field below." }
                }

                label {
                    span.label-text { "Category " span.req { "*" } }
                    select name="category" required {
                        option value="" { "— select —" }
                        @for g in cats::GROUPS {
                            optgroup label=(g) {
                                @for c in cats::in_group(g) {
                                    option value=(c.id) { (c.id) " — " (c.name) }
                                }
                            }
                        }
                    }
                    span.hint.no-katex {
                        "Grouped by subject area. The "
                        code { "cs.*" } " / " code { "math.*" } " / " code { "stat.*" } " / " code { "hep-*" } " / " code { "q-bio.*" } " / " code { "q-fin.*" } " / " code { "econ.*" }
                        " namespaces are arXiv-canonical (an id like " code { "cs.AI" } " here means the same thing as on arXiv). "
                        code { "bio.*" } " mirrors bioRxiv's wet-bio subject areas; " code { "med.*" } " mirrors medRxiv's clinical / public-health areas. Pick the most specific match; " code { "misc" } " is fine if nothing fits."
                    }
                }

                label {
                    span.label-text { "Abstract " span.req { "*" } }
                    textarea name="abstract" required minlength="100" maxlength="5000" rows="8"
                             placeholder="State what the manuscript claims, what role the AI played, and what (if anything) you have verified by hand." {}
                    span.hint.no-katex {
                        "100–5000 characters. Markdown supported ("
                        code { "**bold**" } ", "
                        code { "`code`" } ", lists, links, blockquotes). Inline LaTeX with "
                        code { "$…$" }
                        ", display math with "
                        code { "$$…$$" }
                        ". All rendering happens on the manuscript page."
                    }
                }

                section.source-choice-section {
                    p.label-text { "Source format " span.req { "*" } }
                    p.muted.small.no-katex { "PreXiv keeps a hosted copy of every paper. Upload your LaTeX source and we compile it, or upload a finished PDF directly. External URLs are supplemental links." }

                    div.conductor-type-choice.source-type-choice {
                        label.ctype-card {
                            input type="radio" name="source_type" value="tex" checked;
                            div.ctype-body {
                                strong { "LaTeX source " span.muted.small { "(recommended)" } }
                                span.muted.small.no-katex {
                                    "We compile the PDF server-side with "
                                    code { "pdflatex" }
                                    " (no shell-escape; 60-second timeout). Upload a single "
                                    code { ".tex" }
                                    " file, or a "
                                    code { ".zip" }
                                    " / "
                                    code { ".tar.gz" }
                                    " containing the .tex plus figures and "
                                    code { ".bib" }
                                    "."
                                }
                            }
                        }
                        label.ctype-card {
                            input type="radio" name="source_type" value="pdf";
                            div.ctype-body {
                                strong { "PDF directly" }
                                span.muted.small.no-katex { "Skip compilation: upload an already-finished PDF. Use this if you don't have the .tex source handy." }
                            }
                        }
                    }

                    // LaTeX source upload — visible when source_type=tex.
                    div.source-block.source-tex-block {
                        p.label-text { "Upload LaTeX source " span.req { "*" } }
                        div.upload-dropzone data-bound-name="source-name" {
                            input #source_upload.upload-input type="file" name="source"
                                  accept=".tex,.zip,.tar.gz,.tgz,application/x-tex,application/zip,application/gzip,application/x-gzip";
                            label.upload-target for="source_upload" {
                                span.upload-icon aria-hidden="true" { (PreEscaped(UPLOAD_ICON_SVG)) }
                                span.upload-copy {
                                    strong.upload-prompt { "Click to choose, or drop your archive here" }
                                    span.upload-filename #source-name data-empty="No file selected" { "No file selected" }
                                }
                                span.upload-button { "Browse" }
                            }
                        }
                        ul.upload-hint-list.no-katex {
                            li {
                                span.upload-hint-key { "Accepts" }
                                span.upload-hint-val {
                                    code { ".tex" } " · " code { ".zip" } " · " code { ".tar.gz" }
                                }
                            }
                            li {
                                span.upload-hint-key { "Size limit" }
                                span.upload-hint-val { "30 MB" }
                            }
                            li {
                                span.upload-hint-key { "Required" }
                                span.upload-hint-val {
                                    "A " code { ".tex" } " with " code { "\\documentclass" }
                                    " in the archive root or any subdirectory. Bibliography ("
                                    code { ".bib" } ") and figures may sit alongside it."
                                }
                            }
                            li {
                                span.upload-hint-key { "Builder" }
                                span.upload-hint-val {
                                    code { "pdflatex" } " (or " code { "latexmk" } " if available), "
                                    code { "--no-shell-escape" } ", 60-second timeout."
                                }
                            }
                        }
                    }

                    // PDF upload — visible when source_type=pdf.
                    div.source-block.source-pdf-block {
                        p.label-text { "Upload PDF " span.req { "*" } }
                        div.upload-dropzone data-bound-name="pdf-name" {
                            input #pdf_upload.upload-input type="file" name="pdf" accept="application/pdf";
                            label.upload-target for="pdf_upload" {
                                span.upload-icon aria-hidden="true" { (PreEscaped(UPLOAD_ICON_SVG)) }
                                span.upload-copy {
                                    strong.upload-prompt { "Click to choose, or drop your PDF here" }
                                    span.upload-filename #pdf-name data-empty="No file selected" { "No file selected" }
                                }
                                span.upload-button { "Browse" }
                            }
                        }
                        ul.upload-hint-list.no-katex {
                            li {
                                span.upload-hint-key { "Accepts" }
                                span.upload-hint-val { code { ".pdf" } }
                            }
                            li {
                                span.upload-hint-key { "Size limit" }
                                span.upload-hint-val { "30 MB" }
                            }
                        }
                    }

                    // External URL — visible always as a supplemental link.
                    label.source-external-url {
                        span.label-text { "External URL " span.muted.small { "(optional)" } }
                        input type="url" name="external_url" maxlength="500"
                              placeholder="https://… (e.g., arXiv abstract, GitHub repo, journal page)";
                        span.hint.no-katex { "A supplemental canonical link to the same work elsewhere. Readers see it alongside the PreXiv-hosted PDF/source." }
                    }
                }
            }

            section.form-section {
                h2 { "2 — Conductor " span.muted { "(required)" } }
                p.muted.small {
                    "How was this manuscript produced? PreXiv accepts both human-conducted (a person directed an AI) and fully autonomous AI-agent work. Pick one."
                }

                div.conductor-type-choice {
                    label.ctype-card {
                        input type="radio" name="conductor_type" value="human-ai" checked;
                        div.ctype-body {
                            strong { "Human + AI co-conductor" }
                            span.muted.small {
                                "A named human directed the AI to produce this work. The human accepts responsibility for the "
                                em { "conduct" }
                                " of the work (not necessarily its correctness — that's what an auditor is for)."
                            }
                        }
                    }
                    label.ctype-card {
                        input type="radio" name="conductor_type" value="ai-agent";
                        div.ctype-body {
                            strong { "AI agent alone " span.muted { "(autonomous)" } }
                            span.muted.small {
                                "The manuscript was produced by an AI agent acting on its own — no human direction beyond an initial task description. "
                                em { "No human" }
                                " takes responsibility for either conduct or content; you (the submitter) only post it on the site."
                            }
                        }
                    }
                }

                div.field {
                    label for="ai-model-typer" {
                        span.label-text { "AI model(s) " span.req { "*" } }
                    }
                    // Tag/chip input. The hidden field is the source of
                    // truth; `ai-model-typer` is just an entry box, and
                    // `ai-model-chips` renders the current selection.
                    // Inline JS at the bottom of the form wires:
                    //   * Enter or comma in the typer  → add a chip + sync hidden
                    //   * Click × on a chip            → remove + sync hidden
                    //   * Form submit                  → flush whatever's still
                    //                                    in the typer into a chip
                    div.tag-input id="ai-model-tag-input" {
                        input type="hidden" id="conductor_ai_model" name="conductor_ai_model" value="";
                        div.tag-chips id="ai-model-chips" aria-live="polite" {}
                        input #ai-model-typer.tag-typer type="text"
                              list="ai-models-list" autocomplete="off"
                              maxlength="200"
                              placeholder="Type a model name, press Enter or comma to add. e.g. Claude Opus 4.7";
                    }
                    datalist id="ai-models-list" {
                        @for m in AI_MODELS { option value=(m); }
                    }
                    span.hint.no-katex {
                        "Pick from the dropdown, or type any precise model+version string. "
                        "Press " strong { "Enter" } " or " strong { "," }
                        " after each model. Add as many as actually contributed."
                    }
                    label.checkbox-inline {
                        input type="checkbox" name="conductor_ai_model_public" value="0";
                        span { "Keep these private. Public viewers will see " em { "(undisclosed)" } "; you and admins still see the value." }
                    }
                }

                section.ctype-section.ctype-human-ai {
                    div.field {
                        label { span.label-text { "Human conductor (your displayed name)" } }
                        input type="text" name="conductor_human" maxlength="200"
                              placeholder="Your name as it should appear on the manuscript";
                        label.checkbox-inline {
                            input type="checkbox" name="conductor_human_public" value="0";
                            span { "Keep this private. Public viewers will see " em { "(undisclosed)" } "." }
                        }
                    }
                    div.field {
                        label {
                            span.label-text { "Conductor role" }
                            select name="conductor_role" {
                                option value="" { "— select —" }
                                @for r in ROLES { option value=(r) { (r) } }
                            }
                            span.hint.no-katex { "From undergraduate to professional expert — readers use this to calibrate." }
                        }
                    }
                }

                section.ctype-section.ctype-ai-agent {
                    div.field {
                        label { span.label-text { "Agent framework" } }
                        input type="text" name="agent_framework" maxlength="120"
                              placeholder="e.g., claude-agent-sdk, langgraph, custom-runtime";
                        span.hint.no-katex { "What ran the agent? Helpful for readers trying to reproduce or evaluate." }
                    }
                    label.checkbox.checkbox-warn {
                        input type="checkbox" name="ai_agent_ack";
                        span {
                            "I acknowledge that this manuscript was produced by an AI agent acting "
                            strong { "autonomously" }
                            ". "
                            strong { "No human" }
                            " — including me — takes responsibility for its conduct or contents. The manuscript page will display a prominent "
                            em { "\"AI-agent (autonomous)\"" }
                            " banner alongside any auditing status."
                        }
                    }
                }

                label {
                    span.label-text { "Conductor notes" }
                    textarea name="conductor_notes" rows="3" maxlength="2000"
                             placeholder="How the manuscript was produced — prompts, iteration cycles, tools, anything a reader needs to understand the conduct." {}
                }
            }

            section.form-section.audit-section {
                h2 { "3 — Auditor " span.muted { "(optional but encouraged)" } }
                p.muted.small.no-katex {
                    "A human auditor is someone who has read the manuscript line by line and is willing to attach their professional reputation to a correctness statement. This is "
                    em { "not" }
                    " formal peer review — it's a signed opinion. Listing an auditor who has not actually read and signed off is the fastest way to get the submission removed."
                }

                div.audit-choice role="radiogroup" aria-label="Audit status" {
                    label.ctype-card {
                        input type="radio" name="audit_status" value="none" checked;
                        div.ctype-body {
                            strong { "No auditor" }
                            span.muted.small { "Nobody is signing a correctness statement. The manuscript page will show a prominent " em { "unaudited" } " warning." }
                        }
                    }
                    label.ctype-card.audit-self-radio {
                        input type="radio" name="audit_status" value="self";
                        div.ctype-body {
                            strong { "Self-audit — I am the conductor " em { "and" } " the auditor" }
                            span.muted.small.no-katex {
                                "Only valid for "
                                strong { "Human + AI" }
                                " submissions. Pick this when you, the conductor, have also read the work line by line and stand behind its correctness. The auditor name and role are copied from your Conductor section above; you only need to write the audit statement."
                            }
                        }
                    }
                    label.ctype-card {
                        input type="radio" name="audit_status" value="other";
                        div.ctype-body {
                            strong { "Someone else audited this" }
                            span.muted.small { "A third party — separate from the conductor — has read it and is willing to sign. Fill in their details below." }
                        }
                    }
                }

                div.audit-none-block {
                    label.checkbox.checkbox-warn {
                        input type="checkbox" name="no_auditor_ack";
                        span {
                            "I understand and acknowledge that "
                            strong { "without an auditor I am NOT responsible for the correctness of this manuscript." }
                            " The work is offered to the community for inspection and discussion in its current form. A prominent "
                            em { "\"unaudited\"" }
                            " warning will be displayed on the manuscript page."
                        }
                    }
                }

                div.audit-self-block {
                    div.audit-self-callout {
                        strong { "Self-audit is a stronger claim than just conducting." }
                        " You're asserting that you've reviewed every line of the manuscript and that the result holds up. Readers will see "
                        em { "“Self-audited by [your name]”" }
                        " on the page — calibrate accordingly. Don't tick this if you only directed the AI and trusted its output."
                    }
                    label {
                        span.label-text { "Self-audit statement " span.req { "*" } }
                        textarea name="self_audit_statement" rows="4" maxlength="2000"
                                 placeholder="What did you actually verify? Which parts did you NOT verify? Any caveats? Markdown + LaTeX render on the manuscript page." {}
                        span.hint.no-katex { "Be specific. " code { "Verified the proof of Lemma 3.2; did not check the numerical experiment in §5." } " is more useful than " code { "Looks right to me." } "" }
                    }
                }

                div.audit-other-block {
                    div.row-fields {
                        label.grow {
                            span.label-text { "Auditor name" }
                            input type="text" name="auditor_name" maxlength="200";
                        }
                        label.grow {
                            span.label-text { "Affiliation" }
                            input type="text" name="auditor_affiliation" maxlength="200";
                        }
                    }
                    div.row-fields {
                        label.grow {
                            span.label-text { "Role" }
                            select name="auditor_role" {
                                option value="" { "— select —" }
                                @for r in ROLES { option value=(r) { (r) } }
                            }
                        }
                        label.grow {
                            span.label-text { "ORCID" }
                            input type="text" name="auditor_orcid"
                                  pattern="\\d{4}-\\d{4}-\\d{4}-\\d{3}[\\dX]"
                                  placeholder="0000-0000-0000-0000";
                        }
                    }
                    label {
                        span.label-text { "Auditor statement" }
                        textarea name="auditor_statement" rows="4" maxlength="2000"
                                 placeholder="The auditor's signed statement of what they reviewed and what they stand behind." {}
                    }
                }
            }

            section.form-section {
                h2 { "4 — Licensing" }
                p.muted.small {
                    "Two orthogonal choices: what readers may do with the manuscript, and whether AI systems may train on it. Read "
                    a href="/licenses" target="_blank" rel="noopener" { "the full licensing page" }
                    " for per-license details and the autonomous-AI copyright caveat."
                }

                div.field {
                    label { span.label-text { "Reader license " span.req { "*" } } }
                    select name="license" required {
                        @for l in LICENSES {
                            option value=(l.id) selected[l.id == "CC-BY-4.0"] title=(l.summary) {
                                (l.tagline)
                            }
                        }
                    }
                    span.hint.no-katex {
                        "Hover an option for the one-paragraph summary. Defaults to CC BY 4.0. For autonomous AI-agent submissions, CC0 is the safer choice — under most jurisdictions, purely AI-generated work has no human copyright anyway."
                    }
                }

                div.field {
                    label { span.label-text { "AI training " span.req { "*" } } }
                    div.conductor-type-choice role="radiogroup" aria-label="AI training" {
                        @for o in AI_TRAINING_OPTIONS {
                            label.ctype-card {
                                input type="radio" name="ai_training" value=(o.id) checked[o.id == "allow"];
                                div.ctype-body {
                                    strong { (o.short) }
                                    span.muted.small.no-katex { (o.summary) }
                                }
                            }
                        }
                    }
                    span.hint.no-katex {
                        "Separate from the reader license — a CC BY 4.0 submission can still opt out of AI training. Enforcement of "
                        em { "Disallow" }
                        " depends on trainers honoring the signal (surfaced in HTTP headers and the OpenAPI manifest)."
                    }
                }
            }

            div.form-submit {
                button.btn-primary.big type="submit" { "Submit manuscript" }
            }
        }
        // Reflect the chosen filename in each upload dropzone. Keeps the
        // styled card honest — no orphan native "No file chosen" label.
        script { (PreEscaped(r#"
(function(){
  // ─── Upload-dropzone filename echo ──────────────────────────────
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

  // ─── AI-model tag input ────────────────────────────────────────
  // Source of truth is the hidden #conductor_ai_model field
  // (comma-joined string). Chips are visual. Typer just accumulates.
  var root  = document.getElementById('ai-model-tag-input');
  if(!root) return;
  var hidden = document.getElementById('conductor_ai_model');
  var chips  = document.getElementById('ai-model-chips');
  var typer  = document.getElementById('ai-model-typer');

  function uniqAppend(list, item){
    var lower = item.toLowerCase();
    for(var i=0;i<list.length;i++){ if(list[i].toLowerCase() === lower) return list; }
    list.push(item);
    return list;
  }
  function parseModels(){
    return (hidden.value || '').split(',').map(function(s){ return s.trim(); }).filter(Boolean);
  }
  function syncFromList(list){
    hidden.value = list.join(', ');
    chips.innerHTML = '';
    list.forEach(function(name, idx){
      var c = document.createElement('span');
      c.className = 'tag-chip';
      var lbl = document.createElement('span');
      lbl.className = 'tag-chip-label';
      lbl.textContent = name;
      var x = document.createElement('button');
      x.type = 'button';
      x.className = 'tag-chip-x';
      x.setAttribute('aria-label', 'Remove ' + name);
      x.textContent = '×';
      x.addEventListener('click', function(){
        var cur = parseModels();
        cur.splice(idx, 1);
        syncFromList(cur);
        typer.focus();
      });
      c.appendChild(lbl);
      c.appendChild(x);
      chips.appendChild(c);
    });
  }
  function addFromTyper(){
    var raw = (typer.value || '').trim();
    if(!raw) return;
    var pieces = raw.split(',').map(function(s){ return s.trim(); }).filter(Boolean);
    var cur = parseModels();
    pieces.forEach(function(p){ uniqAppend(cur, p); });
    syncFromList(cur);
    typer.value = '';
  }
  typer.addEventListener('keydown', function(e){
    if(e.key === 'Enter' || e.key === ',') {
      e.preventDefault();
      addFromTyper();
    } else if(e.key === 'Backspace' && !typer.value){
      var cur = parseModels();
      if(cur.length){
        typer.value = cur.pop();
        syncFromList(cur);
      }
    }
  });
  typer.addEventListener('blur', addFromTyper);
  // Final flush on form submit, in case the user typed a name and
  // clicked Submit without pressing Enter.
  var form = typer.closest('form');
  if(form) form.addEventListener('submit', addFromTyper);
  // Initial render: in case the server re-served the form with
  // existing values after a validation error.
  syncFromList(parseModels());
})();
"#)) }
    };
    layout("Submit", ctx, body)
}
