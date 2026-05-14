//! Canonical category taxonomy for PreXiv submissions.
//!
//! Derived from three sources:
//!
//!   • arXiv (https://arxiv.org/category_taxonomy) — CS, Math, Stats,
//!     Physics, plus the arXiv-style `q-bio.*` / `q-fin.*` / `econ.*`
//!     namespaces. Keeps the original dotted ids verbatim (cs.AI,
//!     math.NT, etc.) so a submission can in principle be cross-listed
//!     to arXiv without an id mapping.
//!
//!   • bioRxiv (https://www.biorxiv.org/about/FAQ) — the wet-bio subject
//!     areas (Cell, Developmental, Ecology, Genetics, Neuroscience, …).
//!     Placed under the `bio.*` namespace to disambiguate from arXiv's
//!     quantitative-biology `q-bio.*`.
//!
//!   • medRxiv (https://www.medrxiv.org/) — the clinical / public-health
//!     subject areas (Cardiovascular, Epidemiology, Oncology, …).
//!     Placed under the `med.*` namespace.
//!
//! Curated rather than exhaustive: arXiv has ~150 categories, medRxiv
//! ~50, bioRxiv ~30. Here we keep the ones AI-assisted manuscripts
//! actually land in often, plus the major rare ones, for a total of
//! ~85. The submit form renders these in <optgroup>s so picking is
//! still fast.

#[derive(Debug, Clone, Copy)]
pub struct Category {
    pub id: &'static str,
    pub name: &'static str,
    pub group: &'static str,
}

/// Categories that historically attract cranks / "I overturn Einstein"
/// submissions across preprint servers — physics.gen-ph, the *.GN
/// "general" buckets in econ / q-fin, etc. Submissions to these are
/// **not surfaced** in the default ranked listings (`/`, `/new`,
/// `/top`, `/audited`); they're still reachable via `/browse`,
/// `/search`, direct ID, and OAI-PMH. The listing routes can
/// override the filter with `?show_all=1`.
///
/// We mark them by id rather than by group so a careful submission to
/// e.g. `gr-qc` (which is legitimate) still surfaces, while the same
/// person's `physics.gen-ph` submission doesn't.
pub const RESTRICTED_CATEGORIES: &[&str] = &[
    "physics.gen-ph", // "General Physics" — TOE / anti-relativity attractor
    "econ.GN",        // "General Economics" — anti-mainstream-econ attractor
    "q-fin.GN",       // "General Finance" — get-rich-quick model attractor
];

pub fn is_restricted(category_id: &str) -> bool {
    RESTRICTED_CATEGORIES.contains(&category_id)
}

/// SQL fragment for `category NOT IN ('id1','id2',…)`. Built once at
/// startup-equivalent (it's a `const`-derived `String`-via-`format!`)
/// rather than per-query; callers concat it into their WHERE clause.
/// Returns an empty string if the list is empty, which is a no-op
/// when ANDed in.
pub fn restricted_not_in_clause() -> String {
    if RESTRICTED_CATEGORIES.is_empty() {
        return String::new();
    }
    let mut s = String::from("category NOT IN (");
    for (i, c) in RESTRICTED_CATEGORIES.iter().enumerate() {
        if i > 0 {
            s.push(',');
        }
        // These are compile-time constants from our own source; not
        // user input. Safe to interpolate. The single quotes are
        // single-character literals which can't contain a quote.
        s.push('\'');
        s.push_str(c);
        s.push('\'');
    }
    s.push(')');
    s
}

/// Display order for the groups; the form renders <optgroup>s in this order.
/// Physics first, then Mathematics, then the rest — per user preference for
/// the /submit Category dropdown.
pub const GROUPS: &[&str] = &[
    "Physics",
    "Mathematics",
    "Computer Science",
    "Statistics",
    "Quantitative Biology",
    "Biology (bioRxiv-style)",
    "Medicine (medRxiv-style)",
    "Economics & Finance",
    "Other",
];

pub const CATEGORIES: &[Category] = &[
    // ─── Computer Science (arXiv cs.*) ──────────────────────────────────
    Category {
        id: "cs.AI",
        name: "Artificial Intelligence",
        group: "Computer Science",
    },
    Category {
        id: "cs.LG",
        name: "Machine Learning",
        group: "Computer Science",
    },
    Category {
        id: "cs.CL",
        name: "Computation & Language (NLP)",
        group: "Computer Science",
    },
    Category {
        id: "cs.CV",
        name: "Computer Vision",
        group: "Computer Science",
    },
    Category {
        id: "cs.NE",
        name: "Neural & Evolutionary Computing",
        group: "Computer Science",
    },
    Category {
        id: "cs.MA",
        name: "Multi-Agent Systems",
        group: "Computer Science",
    },
    Category {
        id: "cs.IR",
        name: "Information Retrieval",
        group: "Computer Science",
    },
    Category {
        id: "cs.CR",
        name: "Cryptography & Security",
        group: "Computer Science",
    },
    Category {
        id: "cs.DB",
        name: "Databases",
        group: "Computer Science",
    },
    Category {
        id: "cs.DC",
        name: "Distributed & Parallel Computing",
        group: "Computer Science",
    },
    Category {
        id: "cs.DS",
        name: "Data Structures & Algorithms",
        group: "Computer Science",
    },
    Category {
        id: "cs.SE",
        name: "Software Engineering",
        group: "Computer Science",
    },
    Category {
        id: "cs.PL",
        name: "Programming Languages",
        group: "Computer Science",
    },
    Category {
        id: "cs.HC",
        name: "Human-Computer Interaction",
        group: "Computer Science",
    },
    Category {
        id: "cs.RO",
        name: "Robotics",
        group: "Computer Science",
    },
    Category {
        id: "cs.SY",
        name: "Systems & Control",
        group: "Computer Science",
    },
    Category {
        id: "cs.IT",
        name: "Information Theory",
        group: "Computer Science",
    },
    Category {
        id: "cs.LO",
        name: "Logic in Computer Science",
        group: "Computer Science",
    },
    Category {
        id: "cs.CC",
        name: "Computational Complexity",
        group: "Computer Science",
    },
    Category {
        id: "cs.GR",
        name: "Graphics",
        group: "Computer Science",
    },
    Category {
        id: "cs.SD",
        name: "Sound",
        group: "Computer Science",
    },
    Category {
        id: "cs.GT",
        name: "Computer Science & Game Theory",
        group: "Computer Science",
    },
    // ─── Mathematics (arXiv math.*) ─────────────────────────────────────
    Category {
        id: "math.AG",
        name: "Algebraic Geometry",
        group: "Mathematics",
    },
    Category {
        id: "math.AT",
        name: "Algebraic Topology",
        group: "Mathematics",
    },
    Category {
        id: "math.AP",
        name: "Analysis of PDEs",
        group: "Mathematics",
    },
    Category {
        id: "math.CA",
        name: "Classical Analysis & ODEs",
        group: "Mathematics",
    },
    Category {
        id: "math.CO",
        name: "Combinatorics",
        group: "Mathematics",
    },
    Category {
        id: "math.AC",
        name: "Commutative Algebra",
        group: "Mathematics",
    },
    Category {
        id: "math.CV",
        name: "Complex Variables",
        group: "Mathematics",
    },
    Category {
        id: "math.DG",
        name: "Differential Geometry",
        group: "Mathematics",
    },
    Category {
        id: "math.DS",
        name: "Dynamical Systems",
        group: "Mathematics",
    },
    Category {
        id: "math.FA",
        name: "Functional Analysis",
        group: "Mathematics",
    },
    Category {
        id: "math.GT",
        name: "Geometric Topology",
        group: "Mathematics",
    },
    Category {
        id: "math.GR",
        name: "Group Theory",
        group: "Mathematics",
    },
    Category {
        id: "math.LO",
        name: "Logic",
        group: "Mathematics",
    },
    Category {
        id: "math.MP",
        name: "Mathematical Physics",
        group: "Mathematics",
    },
    Category {
        id: "math.NT",
        name: "Number Theory",
        group: "Mathematics",
    },
    Category {
        id: "math.NA",
        name: "Numerical Analysis",
        group: "Mathematics",
    },
    Category {
        id: "math.OC",
        name: "Optimization & Control",
        group: "Mathematics",
    },
    Category {
        id: "math.PR",
        name: "Probability",
        group: "Mathematics",
    },
    Category {
        id: "math.RT",
        name: "Representation Theory",
        group: "Mathematics",
    },
    Category {
        id: "math.ST",
        name: "Statistics Theory",
        group: "Mathematics",
    },
    // ─── Statistics (arXiv stat.*) ──────────────────────────────────────
    Category {
        id: "stat.ML",
        name: "Machine Learning (statistical)",
        group: "Statistics",
    },
    Category {
        id: "stat.ME",
        name: "Methodology",
        group: "Statistics",
    },
    Category {
        id: "stat.AP",
        name: "Applications",
        group: "Statistics",
    },
    Category {
        id: "stat.CO",
        name: "Computation",
        group: "Statistics",
    },
    Category {
        id: "stat.TH",
        name: "Theory",
        group: "Statistics",
    },
    // ─── Physics (arXiv) ────────────────────────────────────────────────
    Category {
        id: "astro-ph",
        name: "Astrophysics",
        group: "Physics",
    },
    Category {
        id: "cond-mat",
        name: "Condensed Matter",
        group: "Physics",
    },
    Category {
        id: "gr-qc",
        name: "General Relativity & Quantum Cosmology",
        group: "Physics",
    },
    Category {
        id: "hep-ex",
        name: "High Energy Physics — Experiment",
        group: "Physics",
    },
    Category {
        id: "hep-lat",
        name: "High Energy Physics — Lattice",
        group: "Physics",
    },
    Category {
        id: "hep-ph",
        name: "High Energy Physics — Phenomenology",
        group: "Physics",
    },
    Category {
        id: "hep-th",
        name: "High Energy Physics — Theory",
        group: "Physics",
    },
    Category {
        id: "nucl-ex",
        name: "Nuclear — Experiment",
        group: "Physics",
    },
    Category {
        id: "nucl-th",
        name: "Nuclear — Theory",
        group: "Physics",
    },
    Category {
        id: "quant-ph",
        name: "Quantum Physics",
        group: "Physics",
    },
    Category {
        id: "physics.gen-ph",
        name: "General Physics",
        group: "Physics",
    },
    Category {
        id: "physics.bio-ph",
        name: "Biological Physics",
        group: "Physics",
    },
    Category {
        id: "physics.chem-ph",
        name: "Chemical Physics",
        group: "Physics",
    },
    // ─── Quantitative Biology (arXiv q-bio.*) ───────────────────────────
    Category {
        id: "q-bio.BM",
        name: "Biomolecules",
        group: "Quantitative Biology",
    },
    Category {
        id: "q-bio.GN",
        name: "Genomics (computational)",
        group: "Quantitative Biology",
    },
    Category {
        id: "q-bio.MN",
        name: "Molecular Networks",
        group: "Quantitative Biology",
    },
    Category {
        id: "q-bio.NC",
        name: "Neurons & Cognition",
        group: "Quantitative Biology",
    },
    Category {
        id: "q-bio.PE",
        name: "Populations & Evolution",
        group: "Quantitative Biology",
    },
    Category {
        id: "q-bio.QM",
        name: "Quantitative Methods",
        group: "Quantitative Biology",
    },
    // ─── Biology — bioRxiv subject areas (bio.*) ────────────────────────
    Category {
        id: "bio.animal-behavior",
        name: "Animal Behavior & Cognition",
        group: "Biology (bioRxiv-style)",
    },
    Category {
        id: "bio.biochem",
        name: "Biochemistry",
        group: "Biology (bioRxiv-style)",
    },
    Category {
        id: "bio.bioengineering",
        name: "Bioengineering",
        group: "Biology (bioRxiv-style)",
    },
    Category {
        id: "bio.bioinformatics",
        name: "Bioinformatics",
        group: "Biology (bioRxiv-style)",
    },
    Category {
        id: "bio.biophysics",
        name: "Biophysics",
        group: "Biology (bioRxiv-style)",
    },
    Category {
        id: "bio.cancer",
        name: "Cancer Biology",
        group: "Biology (bioRxiv-style)",
    },
    Category {
        id: "bio.cell",
        name: "Cell Biology",
        group: "Biology (bioRxiv-style)",
    },
    Category {
        id: "bio.developmental",
        name: "Developmental Biology",
        group: "Biology (bioRxiv-style)",
    },
    Category {
        id: "bio.ecology",
        name: "Ecology",
        group: "Biology (bioRxiv-style)",
    },
    Category {
        id: "bio.evolutionary",
        name: "Evolutionary Biology",
        group: "Biology (bioRxiv-style)",
    },
    Category {
        id: "bio.genetics",
        name: "Genetics",
        group: "Biology (bioRxiv-style)",
    },
    Category {
        id: "bio.genomics",
        name: "Genomics",
        group: "Biology (bioRxiv-style)",
    },
    Category {
        id: "bio.immunology",
        name: "Immunology",
        group: "Biology (bioRxiv-style)",
    },
    Category {
        id: "bio.microbiology",
        name: "Microbiology",
        group: "Biology (bioRxiv-style)",
    },
    Category {
        id: "bio.molecular",
        name: "Molecular Biology",
        group: "Biology (bioRxiv-style)",
    },
    Category {
        id: "bio.neuroscience",
        name: "Neuroscience",
        group: "Biology (bioRxiv-style)",
    },
    Category {
        id: "bio.pharma",
        name: "Pharmacology & Toxicology",
        group: "Biology (bioRxiv-style)",
    },
    Category {
        id: "bio.physiology",
        name: "Physiology",
        group: "Biology (bioRxiv-style)",
    },
    Category {
        id: "bio.plant",
        name: "Plant Biology",
        group: "Biology (bioRxiv-style)",
    },
    Category {
        id: "bio.synthetic",
        name: "Synthetic Biology",
        group: "Biology (bioRxiv-style)",
    },
    Category {
        id: "bio.systems",
        name: "Systems Biology",
        group: "Biology (bioRxiv-style)",
    },
    // ─── Medicine — medRxiv subject areas (med.*) ───────────────────────
    Category {
        id: "med.allergy-immunology",
        name: "Allergy & Immunology",
        group: "Medicine (medRxiv-style)",
    },
    Category {
        id: "med.cardiovascular",
        name: "Cardiovascular Medicine",
        group: "Medicine (medRxiv-style)",
    },
    Category {
        id: "med.dermatology",
        name: "Dermatology",
        group: "Medicine (medRxiv-style)",
    },
    Category {
        id: "med.emergency",
        name: "Emergency Medicine",
        group: "Medicine (medRxiv-style)",
    },
    Category {
        id: "med.endocrinology",
        name: "Endocrinology",
        group: "Medicine (medRxiv-style)",
    },
    Category {
        id: "med.epidemiology",
        name: "Epidemiology",
        group: "Medicine (medRxiv-style)",
    },
    Category {
        id: "med.gastroenterology",
        name: "Gastroenterology",
        group: "Medicine (medRxiv-style)",
    },
    Category {
        id: "med.genomic-medicine",
        name: "Genetic & Genomic Medicine",
        group: "Medicine (medRxiv-style)",
    },
    Category {
        id: "med.health-informatics",
        name: "Health Informatics",
        group: "Medicine (medRxiv-style)",
    },
    Category {
        id: "med.health-policy",
        name: "Health Policy",
        group: "Medicine (medRxiv-style)",
    },
    Category {
        id: "med.hematology",
        name: "Hematology",
        group: "Medicine (medRxiv-style)",
    },
    Category {
        id: "med.infectious",
        name: "Infectious Diseases (incl. HIV)",
        group: "Medicine (medRxiv-style)",
    },
    Category {
        id: "med.medical-education",
        name: "Medical Education",
        group: "Medicine (medRxiv-style)",
    },
    Category {
        id: "med.nephrology",
        name: "Nephrology",
        group: "Medicine (medRxiv-style)",
    },
    Category {
        id: "med.neurology",
        name: "Neurology",
        group: "Medicine (medRxiv-style)",
    },
    Category {
        id: "med.obgyn",
        name: "Obstetrics & Gynecology",
        group: "Medicine (medRxiv-style)",
    },
    Category {
        id: "med.oncology",
        name: "Oncology",
        group: "Medicine (medRxiv-style)",
    },
    Category {
        id: "med.ophthalmology",
        name: "Ophthalmology",
        group: "Medicine (medRxiv-style)",
    },
    Category {
        id: "med.orthopedics",
        name: "Orthopedics",
        group: "Medicine (medRxiv-style)",
    },
    Category {
        id: "med.pathology",
        name: "Pathology",
        group: "Medicine (medRxiv-style)",
    },
    Category {
        id: "med.pediatrics",
        name: "Pediatrics",
        group: "Medicine (medRxiv-style)",
    },
    Category {
        id: "med.pharmacology",
        name: "Pharmacology & Therapeutics",
        group: "Medicine (medRxiv-style)",
    },
    Category {
        id: "med.psychiatry",
        name: "Psychiatry & Clinical Psychology",
        group: "Medicine (medRxiv-style)",
    },
    Category {
        id: "med.public-health",
        name: "Public & Global Health",
        group: "Medicine (medRxiv-style)",
    },
    Category {
        id: "med.radiology",
        name: "Radiology & Imaging",
        group: "Medicine (medRxiv-style)",
    },
    Category {
        id: "med.respiratory",
        name: "Respiratory Medicine",
        group: "Medicine (medRxiv-style)",
    },
    Category {
        id: "med.surgery",
        name: "Surgery",
        group: "Medicine (medRxiv-style)",
    },
    // ─── Economics & Finance (arXiv econ.* / q-fin.*) ──────────────────
    Category {
        id: "econ.EM",
        name: "Econometrics",
        group: "Economics & Finance",
    },
    Category {
        id: "econ.GN",
        name: "General Economics",
        group: "Economics & Finance",
    },
    Category {
        id: "econ.TH",
        name: "Economic Theory",
        group: "Economics & Finance",
    },
    Category {
        id: "q-fin.CP",
        name: "Computational Finance",
        group: "Economics & Finance",
    },
    Category {
        id: "q-fin.GN",
        name: "General Finance",
        group: "Economics & Finance",
    },
    Category {
        id: "q-fin.PM",
        name: "Portfolio Management",
        group: "Economics & Finance",
    },
    Category {
        id: "q-fin.RM",
        name: "Risk Management",
        group: "Economics & Finance",
    },
    Category {
        id: "q-fin.ST",
        name: "Statistical Finance",
        group: "Economics & Finance",
    },
    // ─── Other ──────────────────────────────────────────────────────────
    Category {
        id: "methods",
        name: "Methods & Methodology (cross-cutting)",
        group: "Other",
    },
    Category {
        id: "misc",
        name: "Miscellaneous",
        group: "Other",
    },
];

pub fn in_group(group: &str) -> impl Iterator<Item = &'static Category> + '_ {
    CATEGORIES.iter().filter(move |c| c.group == group)
}
