//! The PreXiv license model — three orthogonal axes (platform / reader /
//! AI-training). See /licenses for the full prose; the design rationale
//! lives in commits and in pages_content/licenses.html.

#[derive(Debug, Clone, Copy)]
#[allow(dead_code)]
pub struct License {
    pub id: &'static str,
    pub short: &'static str,
    /// One-line label for the <select> dropdown (must fit in ~70 chars).
    pub tagline: &'static str,
    pub name: &'static str,
    pub summary: &'static str,
    pub url: &'static str,
    pub allows_commercial: bool,
    pub allows_derivatives: bool,
    pub allows_redistribution: bool,
}

pub const LICENSES: &[License] = &[
    License {
        id: "CC0-1.0",
        short: "CC0",
        tagline: "CC0 1.0 — public domain (no rights reserved)",
        name: "CC0 1.0 — Public Domain Dedication",
        summary: "Submitter relinquishes all rights. Anyone may copy, modify, distribute, and use the work, even commercially, without asking permission. Recommended for autonomous AI-agent submissions, which under US/UK doctrine may have no human-authored copyright in the first place.",
        url: "https://creativecommons.org/publicdomain/zero/1.0/",
        allows_commercial: true, allows_derivatives: true, allows_redistribution: true,
    },
    License {
        id: "CC-BY-4.0",
        short: "CC BY 4.0",
        tagline: "CC BY 4.0 — attribute & reuse (open default)",
        name: "Creative Commons Attribution 4.0 International",
        summary: "Anyone may share and adapt the work for any purpose, including commercial, provided they give credit (cite the manuscript) and indicate if changes were made. Recommended default for Human + AI submissions; matches arXiv's modern open default.",
        url: "https://creativecommons.org/licenses/by/4.0/",
        allows_commercial: true, allows_derivatives: true, allows_redistribution: true,
    },
    License {
        id: "CC-BY-SA-4.0",
        short: "CC BY-SA 4.0",
        tagline: "CC BY-SA 4.0 — attribute & ShareAlike (copyleft)",
        name: "Creative Commons Attribution-ShareAlike 4.0 International",
        summary: "Like CC BY 4.0 but with copyleft: derivative works must be distributed under the same license. Useful when you want adaptations to stay open.",
        url: "https://creativecommons.org/licenses/by-sa/4.0/",
        allows_commercial: true, allows_derivatives: true, allows_redistribution: true,
    },
    License {
        id: "CC-BY-NC-4.0",
        short: "CC BY-NC 4.0",
        tagline: "CC BY-NC 4.0 — attribute & noncommercial",
        name: "Creative Commons Attribution-NonCommercial 4.0 International",
        summary: "Anyone may share and adapt the work with attribution, but NOT for commercial purposes. Useful when the result has potential industrial value the submitter wants to keep.",
        url: "https://creativecommons.org/licenses/by-nc/4.0/",
        allows_commercial: false, allows_derivatives: true, allows_redistribution: true,
    },
    License {
        id: "CC-BY-NC-SA-4.0",
        short: "CC BY-NC-SA 4.0",
        tagline: "CC BY-NC-SA 4.0 — noncommercial + copyleft",
        name: "Creative Commons Attribution-NonCommercial-ShareAlike 4.0 International",
        summary: "Noncommercial reuse with attribution, and derivatives must use the same license. The 'stay open and stay academic' combination.",
        url: "https://creativecommons.org/licenses/by-nc-sa/4.0/",
        allows_commercial: false, allows_derivatives: true, allows_redistribution: true,
    },
    License {
        id: "PREXIV-STANDARD-1.0",
        short: "PreXiv Standard",
        tagline: "PreXiv Standard — read & cite, no redistribution or training",
        name: "PreXiv Standard License 1.0",
        summary: "Readers may read, study, and cite the work and discuss it on PreXiv. They may NOT redistribute it outside PreXiv, create derivative works, or use it as ML training data. For community-feedback submissions where the submitter is not yet ready to commit to broader open-content terms.",
        url: "/licenses#prexiv-standard",
        allows_commercial: false, allows_derivatives: false, allows_redistribution: false,
    },
];

pub fn lookup(id: &str) -> Option<&'static License> {
    LICENSES.iter().find(|l| l.id == id)
}

#[derive(Debug, Clone, Copy)]
pub struct AiTrainingOption {
    pub id: &'static str,
    pub short: &'static str,
    pub summary: &'static str,
}

pub const AI_TRAINING_OPTIONS: &[AiTrainingOption] = &[
    AiTrainingOption {
        id: "allow",
        short: "Allow",
        summary: "AI training is permitted on this manuscript. Default — matches the open-research ecosystem where most preprints already flow into training corpora.",
    },
    AiTrainingOption {
        id: "allow-with-attribution",
        short: "Allow with attribution",
        summary: "Training is permitted, but the submitter requests that trained models attribute this work (PreXiv id and DOI) when generating substantively similar content. Non-binding — current models can't reliably honor this — but signals intent.",
    },
    AiTrainingOption {
        id: "disallow",
        short: "Disallow",
        summary: "The submitter requests that this manuscript NOT be used as training data for AI models. Signaled in the manuscript page, the OpenAPI manifest, and via PreXiv's robots.txt (`X-Robots-Tag: noai` headers). Enforcement depends on the model trainer's good-faith reading of these signals.",
    },
];

pub fn ai_training_lookup(id: &str) -> Option<&'static AiTrainingOption> {
    AI_TRAINING_OPTIONS.iter().find(|o| o.id == id)
}
