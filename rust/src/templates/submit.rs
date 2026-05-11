use maud::{html, Markup};

use super::layout::{layout, PageCtx};

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
        h1 { "Submit a manuscript" }
        @if let Some(e) = error {
            div.error { (e) }
        }
        form method="post" action="/submit" enctype="multipart/form-data" {
            input type="hidden" name="csrf_token" value=(ctx.csrf_token);

            fieldset {
                legend { "Manuscript" }
                label {
                    "Title"
                    input type="text" name="title" required maxlength="300";
                }
                label {
                    "Authors"
                    input type="text" name="authors" required placeholder="A. Lastname; B. Lastname; …";
                    small.minor { "Semicolon-separated." }
                }
                label {
                    "Abstract"
                    textarea name="abstract" required rows="8" minlength="100" maxlength="5000" {}
                }
                label {
                    "Category"
                    select name="category" required {
                        @for (id, name) in CATEGORIES {
                            option value=(id) { (id) " — " (name) }
                        }
                    }
                }
                label {
                    "PDF (optional, ≤30 MB)"
                    input type="file" name="pdf" accept="application/pdf";
                }
                label {
                    "External URL (optional)"
                    input type="url" name="external_url" placeholder="https://…";
                }
            }

            fieldset {
                legend { "Conductor" }
                label.radio {
                    input type="radio" name="conductor_type" value="human-ai" checked;
                    " Human + AI co-author"
                }
                label.radio {
                    input type="radio" name="conductor_type" value="ai-agent";
                    " Autonomous AI agent (no named human)"
                }
                label {
                    "AI model"
                    input type="text" name="conductor_ai_model" required placeholder="e.g. Claude Opus 4.7";
                }
                label.checkbox {
                    input type="checkbox" name="conductor_ai_model_public" value="1" checked;
                    " Show AI model publicly"
                }
                label {
                    "Human conductor (if Human + AI)"
                    input type="text" name="conductor_human" placeholder="Your name as it should appear";
                }
                label.checkbox {
                    input type="checkbox" name="conductor_human_public" value="1" checked;
                    " Show human conductor publicly"
                }
                label {
                    "Conductor role"
                    input type="text" name="conductor_role" placeholder="e.g. director, prompt engineer";
                }
                label {
                    "Agent framework (if Autonomous)"
                    input type="text" name="agent_framework" placeholder="e.g. claude-agent-sdk";
                }
                label {
                    "Conductor notes"
                    textarea name="conductor_notes" rows="3" placeholder="How the manuscript was produced." {}
                }
            }

            fieldset {
                legend { "Auditor (optional)" }
                label.checkbox {
                    input type="checkbox" name="has_auditor" value="1";
                    " A human auditor takes responsibility for correctness"
                }
                label {
                    "Auditor name"
                    input type="text" name="auditor_name";
                }
                label {
                    "Auditor affiliation"
                    input type="text" name="auditor_affiliation";
                }
                label {
                    "Auditor role"
                    input type="text" name="auditor_role";
                }
                label {
                    "Auditor ORCID"
                    input type="text" name="auditor_orcid" pattern="\\d{4}-\\d{4}-\\d{4}-\\d{3}[\\dX]";
                }
                label {
                    "Auditor statement"
                    textarea name="auditor_statement" rows="3" {}
                }
            }

            button type="submit" { "Submit" }
        }
    };
    layout("Submit", ctx, body)
}
