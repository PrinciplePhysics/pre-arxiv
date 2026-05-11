use maud::{html, Markup};

use crate::licenses::{AI_TRAINING_OPTIONS, LICENSES};
use super::layout::{layout, PageCtx};

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

    // Multi-model authorship
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

const CATEGORIES: &[(&str, &str)] = &[
    ("cs.AI", "Artificial Intelligence"),
    ("cs.LG", "Machine Learning"),
    ("cs.CL", "Computation & Language"),
    ("cs.CV", "Computer Vision"),
    ("cs.SE", "Software Engineering"),
    ("math.AG", "Algebraic Geometry"),
    ("math.NT", "Number Theory"),
    ("math.PR", "Probability"),
    ("math.OC", "Optimization & Control"),
    ("physics.gen-ph", "General Physics"),
    ("hep-th", "High Energy Physics — Theory"),
    ("hep-ph", "High Energy Physics — Phenomenology"),
    ("nucl-th", "Nuclear Theory"),
    ("cond-mat", "Condensed Matter"),
    ("astro-ph", "Astrophysics"),
    ("q-bio", "Quantitative Biology"),
    ("q-fin", "Quantitative Finance"),
    ("stat.ML", "Statistics — Machine Learning"),
    ("econ", "Economics"),
    ("misc", "Miscellaneous"),
];

pub fn render(ctx: &PageCtx, error: Option<&str>) -> Markup {
    let body = html! {
        div.page-header {
            h1 { "Submit a manuscript" }
            p.muted {
                "A manuscript on PreXiv is a piece of work in which an AI was a substantial co-author. The "
                strong { "conductor" }
                " is the human who guided the AI to write it (or, in autonomous mode, the AI that produced it alone); the "
                strong { "auditor" }
                " (optional) is a human expert who has verified its correctness."
            }
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
                          placeholder="e.g., Jane Doe; Claude Opus 4.7";
                    span.hint.no-katex { "Separate authors with semicolons. List the AI as a co-author by its model name." }
                }

                label {
                    span.label-text { "Category " span.req { "*" } }
                    select name="category" required {
                        option value="" { "— select —" }
                        @for (id, name) in CATEGORIES {
                            option value=(id) { (id) " — " (name) }
                        }
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

                div.row-fields {
                    div.grow.field {
                        label for="pdf_upload" { span.label-text { "Upload PDF" } }
                        input id="pdf_upload" type="file" name="pdf" accept="application/pdf";
                        span.hint.no-katex { "Optional if you provide an external URL. Max 30 MB." }
                    }
                    label.grow {
                        span.label-text { "External URL" }
                        input type="url" name="external_url" maxlength="500"
                              placeholder="https://… (e.g., GitHub repo or hosted PDF)";
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
                    label for="conductor_ai_model" { span.label-text { "AI model " span.req { "*" } } }
                    input id="conductor_ai_model" type="text" name="conductor_ai_model" required maxlength="200"
                          list="ai-models-list" autocomplete="off"
                          placeholder="Type or pick — Claude Opus 4.7, GPT-5, Gemini 3 Pro, …";
                    datalist id="ai-models-list" {
                        @for m in AI_MODELS { option value=(m); }
                    }
                    span.hint.no-katex { "Pick from the dropdown for the current flagships, or type any precise model+version string." }
                    label.checkbox-inline {
                        input type="checkbox" name="conductor_ai_model_public" value="0";
                        span { "Keep this private. Public viewers will see " em { "(undisclosed)" } "; you and admins still see the value." }
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
    };
    layout("Submit", ctx, body)
}
