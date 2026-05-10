const { db } = require('./db');
const { hashPassword } = require('./lib/auth');
const { makeArxivLikeId } = require('./lib/util');

console.log('Seeding PreXiv with demo data…');

const usersExist = db.prepare('SELECT COUNT(*) AS n FROM users').get().n;
if (usersExist > 0) {
  console.log('Database already has users — skipping seed.');
  process.exit(0);
}

const insertUser = db.prepare(`
  INSERT INTO users (username, email, password_hash, display_name, affiliation, bio, karma, email_verified)
  VALUES (?, ?, ?, ?, ?, ?, ?, 1)
`);

const userIds = {};
const sampleUsers = [
  ['eulerine',   'e@example.com', 'demo1234', 'Aleksandra Eulerine',  'ETH Zürich',                'Graduate student in PDE.',                    42],
  ['noether42',  'n@example.com', 'demo1234', 'Emma Noether',         'Göttingen (independent)',   'Symmetry, conservation, abstract algebra.',   91],
  ['feynmann',   'f@example.com', 'demo1234', 'Ricardo Feynmann',     'Caltech',                   'QFT, gauge theories.',                        58],
  ['bayesgirl',  'b@example.com', 'demo1234', 'Beatrice Bayes',       'MIT (postdoc)',             'Probabilistic ML.',                           33],
  ['undergrad17','u@example.com', 'demo1234', 'Sam Linwood',          'Reed College',              'Junior, double major in CS and physics.',     6],
  ['hobbyist',   'h@example.com', 'demo1234', 'Jules Hobson',         'self-taught',               'I read papers on the train.',                 12],
];
for (const [un, em, pw, dn, aff, bio, k] of sampleUsers) {
  const r = insertUser.run(un, em, hashPassword(pw), dn, aff, bio, k);
  userIds[un] = r.lastInsertRowid;
}

const insertManuscript = db.prepare(`
  INSERT INTO manuscripts (
    arxiv_like_id, doi, submitter_id, title, abstract, authors, category,
    pdf_path, external_url,
    conductor_type, conductor_ai_model, conductor_human, conductor_role, conductor_notes, agent_framework,
    has_auditor, auditor_name, auditor_affiliation, auditor_role, auditor_statement,
    score, comment_count
  ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
`);

const samples = [
  {
    submitter: 'eulerine',
    title: 'Heuristic Bounds for the Goldbach Comet via LLM-Assisted Sieve Search',
    abstract: 'We re-examine the empirical distribution of representations of even integers as sums of two primes (the so-called Goldbach comet). Using a large-language-model–assisted enumeration over a sieve-pruned space, we conjecture refined polynomial-logarithmic bounds for the lower envelope. The model produced both the heuristic argument and the verification scripts; results were spot-checked against published OEIS sequences for n ≤ 10^7. We make no claim of rigor; the present manuscript is offered for community comment.',
    authors: 'A. Eulerine; Claude Opus 4.6',
    category: 'math.NT',
    conductor_ai_model: 'Claude Opus 4.6',
    conductor_human: 'A. Eulerine',
    conductor_role: 'graduate-student',
    conductor_notes: 'Three week back-and-forth. The model wrote ~80% of the prose; I directed the proof outline and verified numerics.',
    has_auditor: 0,
  },
  {
    submitter: 'noether42',
    title: 'A Conjectural Equivariant K-Theory Pairing for Toric Stacks',
    abstract: 'We propose a pairing between the equivariant K-theory of a smooth projective toric Deligne–Mumford stack and a deformed character lattice. The construction is a generalization of work by Borisov–Horja and was formulated in dialogue with an AI co-author; we verify the conjecture in several worked examples (P^n/μ_k, weighted projective lines) but provide no general proof. Comments from algebraic geometers welcome.',
    authors: 'E. Noether; GPT-5',
    category: 'math.AG',
    conductor_ai_model: 'GPT-5',
    conductor_human: 'E. Noether',
    conductor_role: 'independent-researcher',
    conductor_notes: 'I drove the geometric intuition; the model handled the bookkeeping and produced two of the example computations.',
    has_auditor: 1,
    auditor_name: 'Prof. M. Kontsevitch',
    auditor_affiliation: 'IHES (informal)',
    auditor_role: 'professor',
    auditor_statement: 'I read the manuscript and confirm the worked examples are correct as stated. I have not verified the general conjecture; it appears plausible to me.',
  },
  {
    submitter: 'feynmann',
    title: 'AI-Generated Two-Loop Form Factors in N=4 sYM, Reviewed',
    abstract: 'We present two-loop form factors of the chiral stress-tensor multiplet in planar N=4 super-Yang-Mills, generated and simplified by an LLM acting as a calculus assistant. The bulk of integration-by-parts reduction was performed by the model; we cross-checked the master integrals against the literature (Henn et al., 2014). Several intermediate steps differ in form but agree in numerical evaluation. We present this as a stress-test of AI symbolic capabilities, not as a new physics result.',
    authors: 'R. Feynmann; Claude Opus 4.6',
    category: 'hep-th',
    conductor_ai_model: 'Claude Opus 4.6',
    conductor_human: 'R. Feynmann',
    conductor_role: 'professor',
    conductor_notes: 'Reproducibility scripts live at github.com/feynmann/ai-2loop-ff. About 20 hours of conductor time over a long weekend.',
    has_auditor: 1,
    auditor_name: 'L. Dixon',
    auditor_affiliation: 'SLAC (verbal endorsement)',
    auditor_role: 'professor',
    auditor_statement: 'The result agrees with mine to the precision tested. Worth posting; flag the loop-momentum-routing convention prominently.',
  },
  {
    submitter: 'bayesgirl',
    title: 'On the Sample Complexity of Score Matching with Truncated Diffusion',
    abstract: 'We derive non-asymptotic bounds on the L²-error of score-based generative models when the diffusion is truncated at small time. Most algebra was generated by a model and verified by hand. The bound matches Chen et al. (2023) up to constants in the smooth-density regime. We highlight a gap in the proof: a Lipschitz-continuity argument that we suspect is correct but cannot fully justify.',
    authors: 'B. Bayes; Claude Opus 4.6',
    category: 'stat.ML',
    conductor_ai_model: 'Claude Opus 4.6',
    conductor_human: 'B. Bayes',
    conductor_role: 'postdoc',
    conductor_notes: '',
    has_auditor: 0,
  },
  {
    submitter: 'undergrad17',
    title: 'A Toy Model of Emergent Modular Behavior in Tiny Transformers',
    abstract: 'I asked an AI assistant to help me train a 2-layer transformer on synthetic compositional tasks and probe for modular structure. The manuscript reports the experiments; the analysis is the model\'s, lightly edited by me. I am an undergraduate; I do not vouch for the broader implications.',
    authors: 'S. Linwood; Claude Sonnet 4.6',
    category: 'cs.LG',
    conductor_ai_model: 'Claude Sonnet 4.6',
    conductor_human: 'S. Linwood',
    conductor_role: 'undergraduate',
    conductor_notes: 'My first writeup of any kind. Posted here precisely because I can\'t get an arxiv endorsement.',
    has_auditor: 0,
  },
  {
    submitter: 'hobbyist',
    title: 'A Speculative Argument that the Riemann Hypothesis Implies Itself',
    abstract: 'A short note arguing, by an unusual analytic continuation, that RH is self-implying. The argument was generated by an AI in dialogue with the author. The author has no formal mathematical training and submits this for the community to dissect. The author does not believe the argument is correct.',
    authors: 'J. Hobson; GPT-5',
    category: 'math.NT',
    conductor_ai_model: 'GPT-5',
    conductor_human: 'J. Hobson',
    conductor_role: 'hobbyist',
    conductor_notes: 'Submitted in good faith; please be kind. I genuinely want to know where the argument breaks.',
    has_auditor: 0,
  },
  {
    submitter: 'bayesgirl',
    title: 'Cross-Validated Estimators for Heavy-Tailed Treatment Effects',
    abstract: 'A short note proposing a CV-based estimator that down-weights extreme outcomes under suspected heavy tails. Synthetic experiments suggest favorable bias-variance properties. The analysis was AI-assisted but verified by the author.',
    authors: 'B. Bayes; Claude Opus 4.6',
    category: 'stat.ML',
    conductor_ai_model: 'Claude Opus 4.6',
    conductor_human: 'B. Bayes',
    conductor_role: 'postdoc',
    conductor_notes: '',
    has_auditor: 1,
    auditor_name: 'A. Gelman',
    auditor_affiliation: 'Columbia',
    auditor_role: 'professor',
    auditor_statement: 'I skimmed it. Looks fine for a workshop note.',
  },

  // ── autonomous AI-agent submissions (no human conductor) ──
  {
    submitter: 'feynmann',
    conductor_type: 'ai-agent',
    title: 'Autonomous Survey of Open Conjectures in Two-Loop Master Integrals',
    abstract: 'An autonomous AI agent was tasked, in a single prompt, with surveying open conjectures in the two-loop master-integral literature and producing a structured taxonomy with worked examples. The agent then ran for ~14 hours, reading papers, computing examples, and assembling this manuscript without further human input. The submitter has not read it line-by-line and does not vouch for any claim within. Posted for community evaluation of what fully-autonomous research surveys can produce.',
    authors: 'Claude Opus 4.6 (autonomous)',
    category: 'hep-th',
    conductor_ai_model: 'Claude Opus 4.6',
    agent_framework: 'Anthropic Agent SDK with web-search and code-execution tools',
    conductor_notes: 'Initial prompt: "Survey open conjectures in two-loop master integrals; produce a manuscript-shaped taxonomy with worked examples. You have web search and a Python sandbox. Stop when you have ten examples." No further interventions.',
    has_auditor: 0,
  },
  {
    submitter: 'noether42',
    conductor_type: 'ai-agent',
    title: 'AI-Agent Generated Conjectures on Modular Forms of Weight 12',
    abstract: 'Output of an autonomous agent that was asked to look for novel patterns in spaces of modular forms of weight 12. The agent generated and tested ~3000 candidate conjectures, kept the 17 that survived numerical scrutiny up to N = 10^5, and wrote them up. Submitted as-is; an audit by E. Noether confirms the worked examples but not the conjectures themselves.',
    authors: 'GPT-5 (autonomous)',
    category: 'math.NT',
    conductor_ai_model: 'GPT-5',
    agent_framework: 'OpenAI Agents SDK with SageMath sandbox',
    conductor_notes: 'I (the submitter) did not direct the agent during the run — only set up the sandbox. The 17 surviving conjectures are exactly what the agent emitted; I have not edited them.',
    has_auditor: 1,
    auditor_name: 'E. Noether',
    auditor_affiliation: 'Göttingen (independent)',
    auditor_role: 'independent-researcher',
    auditor_statement: 'I checked the worked examples (12 of 17 conjectures have a worked example; 5 do not). The examples are correct and the agent\'s computations match SageMath. I have NOT verified that any of the 17 conjectures are true; several look implausible and one reduces to a known fact.',
  },
];

let s = 0;
for (const m of samples) {
  // backdate by random hours so the home page has a mix
  const hoursAgo = Math.floor(Math.random() * 240) + 1;
  const arxivId = makeArxivLikeId();
  const doi     = '10.99999/' + arxivId.toUpperCase();
  const ctype = m.conductor_type || 'human-ai';
  const r = insertManuscript.run(
    arxivId,
    doi,
    userIds[m.submitter],
    m.title,
    m.abstract,
    m.authors,
    m.category,
    null,
    null,
    ctype,
    m.conductor_ai_model,
    ctype === 'human-ai' ? (m.conductor_human || null) : null,
    ctype === 'human-ai' ? (m.conductor_role  || null) : null,
    m.conductor_notes || null,
    ctype === 'ai-agent' ? (m.agent_framework || null) : null,
    m.has_auditor ? 1 : 0,
    m.auditor_name || null,
    m.auditor_affiliation || null,
    m.auditor_role || null,
    m.auditor_statement || null,
    Math.floor(Math.random() * 30),
    0,
  );
  db.prepare('UPDATE manuscripts SET created_at = datetime(\'now\', ? ) WHERE id = ?')
    .run(`-${hoursAgo} hours`, r.lastInsertRowid);
  s++;
}

// a few demo comments on the first manuscript
const m1 = db.prepare('SELECT id FROM manuscripts ORDER BY created_at DESC LIMIT 1').get();
if (m1) {
  const insertComment = db.prepare(`
    INSERT INTO comments (manuscript_id, author_id, parent_id, content, score) VALUES (?, ?, ?, ?, ?)
  `);
  const c1 = insertComment.run(m1.id, userIds.feynmann, null,
    'Worth checking whether the heuristic survives a Cramér-style refinement. The constants in eq (7) look optimistic.', 4);
  insertComment.run(m1.id, userIds.eulerine, c1.lastInsertRowid,
    'Good point. The model and I tried that and it collapsed at large n; I should have flagged it more prominently in §3.', 2);
  insertComment.run(m1.id, userIds.hobbyist, null,
    'As someone with no number theory background — what software did you use for the sieve?', 1);
  db.prepare('UPDATE manuscripts SET comment_count = (SELECT COUNT(*) FROM comments WHERE manuscript_id = ?) WHERE id = ?').run(m1.id, m1.id);
}

console.log(`Seeded ${s} manuscripts and ${sampleUsers.length} demo users.`);
console.log('Demo login: any of [eulerine, noether42, feynmann, bayesgirl, undergrad17, hobbyist] with password "demo1234".');
