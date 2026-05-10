const path = require('path');
const fs = require('fs');
const crypto = require('crypto');
const express = require('express');
const session = require('express-session');
const SQLiteStore = require('connect-sqlite3')(session);
const multer = require('multer');
const helmet = require('helmet');
const rateLimit = require('express-rate-limit');

const { db, CATEGORIES, ROLES } = require('./db');
const { hashPassword, verifyPassword, loadUser, requireAuth, validateUsername } = require('./lib/auth');
const { timeAgo, escapeHtml, rankScore, makeArxivLikeId, renderMarkdown, ageHours, paginate } = require('./lib/util');
const { sendMail, absoluteUrl } = require('./lib/email');
const { renderBibtex, renderRis, renderPlain } = require('./lib/citation');
const zenodo = require('./lib/zenodo');

const app = express();
const PORT = parseInt(process.env.PORT, 10) || 3000;
const IS_PROD = process.env.NODE_ENV === 'production';
const DATA_DIR   = process.env.DATA_DIR   || path.join(__dirname, 'data');
const UPLOAD_DIR = process.env.UPLOAD_DIR || path.join(__dirname, 'public', 'uploads');
const SESSION_SECRET = process.env.SESSION_SECRET;
if (!SESSION_SECRET) {
  if (IS_PROD) {
    console.error('SESSION_SECRET env var is required in production. Refusing to start.');
    process.exit(1);
  }
  console.warn('[warn] SESSION_SECRET not set — using a dev-only default. Do NOT use this in production.');
}

if (!fs.existsSync(DATA_DIR))   fs.mkdirSync(DATA_DIR,   { recursive: true });
if (!fs.existsSync(UPLOAD_DIR)) fs.mkdirSync(UPLOAD_DIR, { recursive: true });

// Behind a proxy on Render / Fly / etc., trust X-Forwarded-* so secure cookies
// and rate-limiter IPs work correctly.
app.set('trust proxy', IS_PROD ? 1 : false);

app.set('view engine', 'ejs');
app.set('views', path.join(__dirname, 'views'));

// ─── security headers ───────────────────────────────────────────────────────
app.use(helmet({
  contentSecurityPolicy: {
    directives: {
      defaultSrc: ["'self'"],
      scriptSrc:  ["'self'", "'unsafe-inline'", 'https://cdn.jsdelivr.net'],
      styleSrc:   ["'self'", "'unsafe-inline'", 'https://cdn.jsdelivr.net'],
      fontSrc:    ["'self'", 'https://cdn.jsdelivr.net', 'data:'],
      imgSrc:     ["'self'", 'data:'],
      connectSrc: ["'self'"],
      objectSrc:  ["'self'"],
      frameAncestors: ["'self'"],
    },
  },
  crossOriginEmbedderPolicy: false,
}));

app.use(express.static(path.join(__dirname, 'public')));
app.use(express.urlencoded({ extended: true, limit: '2mb' }));
app.use(express.json({ limit: '2mb' }));

app.use(session({
  store: new SQLiteStore({ db: 'sessions.db', dir: DATA_DIR }),
  secret: SESSION_SECRET || 'pre-arxiv-dev-secret-change-me',
  resave: false,
  saveUninitialized: false,
  cookie: {
    maxAge: 1000 * 60 * 60 * 24 * 30,
    sameSite: 'lax',
    secure: IS_PROD,
    httpOnly: true,
  },
}));

app.use(loadUser);

// ─── expose admin status for templates ──────────────────────────────────────
app.use((req, res, next) => {
  res.locals.isAdmin = false;
  if (req.user) {
    const row = db.prepare('SELECT is_admin FROM users WHERE id = ?').get(req.user.id);
    res.locals.isAdmin = !!(row && row.is_admin);
  }
  next();
});

// ─── CSRF (hand-rolled double-submit using session token) ───────────────────
function csrfTokenFor(req) {
  if (!req.session.csrfToken) {
    req.session.csrfToken = crypto.randomBytes(24).toString('base64url');
  }
  return req.session.csrfToken;
}
function verifyCsrf(req, res, next) {
  if (req.method === 'GET' || req.method === 'HEAD' || req.method === 'OPTIONS') return next();
  // Multipart bodies aren't parsed yet at this point — defer to per-route check
  // (see /submit, which calls multer first then csrfCheckParsed).
  const ct = (req.get('Content-Type') || '').toLowerCase();
  if (ct.startsWith('multipart/form-data')) return next();
  return csrfCheckParsed(req, res, next);
}
function csrfCheckParsed(req, res, next) {
  const token = (req.body && req.body._csrf) || req.get('X-CSRF-Token');
  if (!token || !req.session.csrfToken || token !== req.session.csrfToken) {
    return res.status(403).render('error', { code: 403, msg: 'CSRF check failed. Reload the page and try again.' });
  }
  next();
}

// expose helpers in templates
app.use((req, res, next) => {
  res.locals.timeAgo = timeAgo;
  res.locals.escapeHtml = escapeHtml;
  res.locals.renderMarkdown = renderMarkdown;
  res.locals.CATEGORIES = CATEGORIES;
  res.locals.ROLES = ROLES;
  res.locals.flash = req.session.flash || null;
  delete req.session.flash;
  res.locals.path = req.path;
  res.locals.currentQuery = req.query;
  res.locals.csrfToken = csrfTokenFor(req);
  next();
});

app.use(verifyCsrf);

// ─── rate limiting ──────────────────────────────────────────────────────────
const limit = (windowMs, max, message) => rateLimit({
  windowMs, max, standardHeaders: true, legacyHeaders: false,
  message: { error: message },
  // skip in dev for ergonomics
  skip: () => !IS_PROD && process.env.RATE_LIMIT !== '1',
});
const authLimiter    = limit(15 * 60 * 1000, 10,  'Too many login/register attempts. Try again later.');
const submitLimiter  = limit(60 * 60 * 1000, 6,   'Too many submissions in the last hour.');
const commentLimiter = limit(10 * 60 * 1000, 20,  'Too many comments. Slow down.');
const voteLimiter    = limit(60 * 1000,      60,  'Too many votes. Slow down.');

// ─── upload config ──────────────────────────────────────────────────────────
const upload = multer({
  storage: multer.diskStorage({
    destination: UPLOAD_DIR,
    filename: (_req, file, cb) => {
      const safe = file.originalname.replace(/[^a-zA-Z0-9._-]/g, '_').slice(0, 80);
      cb(null, Date.now() + '-' + Math.floor(Math.random() * 1e6) + '-' + safe);
    }
  }),
  limits: { fileSize: 30 * 1024 * 1024 },
  fileFilter: (_req, file, cb) => {
    const ok = file.mimetype === 'application/pdf' || file.originalname.toLowerCase().endsWith('.pdf');
    cb(ok ? null : new Error('Only PDF files are allowed.'), ok);
  }
});

// ─── helpers ────────────────────────────────────────────────────────────────
function flash(req, type, msg) { req.session.flash = { type, msg }; }

function buildVoteMap(userId, type, ids) {
  const map = {};
  if (!userId || !ids.length) return map;
  const placeholders = ids.map(() => '?').join(',');
  const rows = db.prepare(`SELECT target_id, value FROM votes WHERE user_id = ? AND target_type = ? AND target_id IN (${placeholders})`)
    .all(userId, type, ...ids);
  for (const r of rows) map[r.target_id] = r.value;
  return map;
}

function rankManuscripts(rows) {
  return rows
    .map(r => ({ ...r, rankValue: rankScore(r.score, ageHours(r.created_at)) }))
    .sort((a, b) => b.rankValue - a.rankValue);
}

// Synthetic DOI in Crossref's reserved 10.99999 prefix (test-only — never
// resolves on doi.org). Used so manuscripts have a DOI-shaped citation
// identifier without paying a registrar.
function makeSyntheticDoi(arxivLikeId) {
  return '10.99999/' + (arxivLikeId || '').toUpperCase();
}

// Best-effort PDF -> plain text. Bounded so a malformed PDF can't OOM us;
// failures are logged and return null (we just won't have full-text search
// for that manuscript).
const MAX_PDF_TEXT = 500_000; // ~500 KB of text per manuscript
async function extractPdfText(filepath) {
  try {
    const { PDFParse } = require('pdf-parse');
    const buf = fs.readFileSync(filepath);
    const parser = new PDFParse({ data: buf });
    const result = await parser.getText();
    let txt = (result && result.text) ? String(result.text) : '';
    txt = txt.replace(/\s+/g, ' ').trim();
    if (txt.length > MAX_PDF_TEXT) txt = txt.slice(0, MAX_PDF_TEXT);
    return txt || null;
  } catch (e) {
    console.warn('[pdf-parse] failed for ' + filepath + ': ' + e.message);
    return null;
  }
}

function escapeFtsQuery(q) {
  return q.split(/\s+/).filter(Boolean)
    .map(t => '"' + t.replace(/"/g, '""') + '"')
    .join(' ');
}

// ─── routes: home / browse ──────────────────────────────────────────────────
app.get('/', (req, res) => {
  const { page, per, offset } = paginate(req, 30);
  // pull a wider window then rank
  const window = db.prepare(`
    SELECT m.*, u.username AS submitter_username, u.display_name AS submitter_display
    FROM manuscripts m JOIN users u ON u.id = m.submitter_id
    ORDER BY m.created_at DESC
    LIMIT 300
  `).all();
  const ranked = rankManuscripts(window).slice(offset, offset + per);
  const voteMap = req.user ? buildVoteMap(req.user.id, 'manuscript', ranked.map(r => r.id)) : {};
  res.render('index', { manuscripts: ranked, voteMap, mode: 'ranked', page, per });
});

app.get('/new', (req, res) => {
  const { page, per, offset } = paginate(req, 30);
  const rows = db.prepare(`
    SELECT m.*, u.username AS submitter_username, u.display_name AS submitter_display
    FROM manuscripts m JOIN users u ON u.id = m.submitter_id
    ORDER BY m.created_at DESC LIMIT ? OFFSET ?
  `).all(per, offset);
  const voteMap = req.user ? buildVoteMap(req.user.id, 'manuscript', rows.map(r => r.id)) : {};
  res.render('index', { manuscripts: rows, voteMap, mode: 'new', page, per });
});

app.get('/top', (req, res) => {
  const { page, per, offset } = paginate(req, 30);
  const rows = db.prepare(`
    SELECT m.*, u.username AS submitter_username, u.display_name AS submitter_display
    FROM manuscripts m JOIN users u ON u.id = m.submitter_id
    ORDER BY m.score DESC, m.created_at DESC LIMIT ? OFFSET ?
  `).all(per, offset);
  const voteMap = req.user ? buildVoteMap(req.user.id, 'manuscript', rows.map(r => r.id)) : {};
  res.render('index', { manuscripts: rows, voteMap, mode: 'top', page, per });
});

app.get('/audited', (req, res) => {
  const { page, per, offset } = paginate(req, 30);
  const rows = db.prepare(`
    SELECT m.*, u.username AS submitter_username, u.display_name AS submitter_display
    FROM manuscripts m JOIN users u ON u.id = m.submitter_id
    WHERE m.has_auditor = 1
    ORDER BY m.created_at DESC LIMIT ? OFFSET ?
  `).all(per, offset);
  const voteMap = req.user ? buildVoteMap(req.user.id, 'manuscript', rows.map(r => r.id)) : {};
  res.render('index', { manuscripts: rows, voteMap, mode: 'audited', page, per });
});

app.get('/browse', (req, res) => {
  const counts = {};
  for (const r of db.prepare('SELECT category, COUNT(*) AS n FROM manuscripts GROUP BY category').all()) {
    counts[r.category] = r.n;
  }
  res.render('browse', { counts });
});

app.get('/browse/:cat', (req, res) => {
  const cat = req.params.cat;
  const meta = CATEGORIES.find(c => c.id === cat);
  if (!meta) return res.status(404).render('error', { code: 404, msg: 'Unknown category.' });
  const { page, per, offset } = paginate(req, 30);
  const rows = db.prepare(`
    SELECT m.*, u.username AS submitter_username, u.display_name AS submitter_display
    FROM manuscripts m JOIN users u ON u.id = m.submitter_id
    WHERE m.category = ?
    ORDER BY m.created_at DESC LIMIT ? OFFSET ?
  `).all(cat, per, offset);
  const voteMap = req.user ? buildVoteMap(req.user.id, 'manuscript', rows.map(r => r.id)) : {};
  res.render('index', { manuscripts: rows, voteMap, mode: 'category', categoryMeta: meta, page, per });
});

app.get('/search', (req, res) => {
  const q = (req.query.q || '').trim();
  const rows = [];
  const seen = new Set();
  if (q) {
    // Exact-id matches first (arxiv-like or DOI).
    const idMatches = db.prepare(`
      SELECT m.*, u.username AS submitter_username, u.display_name AS submitter_display
      FROM manuscripts m JOIN users u ON u.id = m.submitter_id
      WHERE m.arxiv_like_id = ? OR m.doi = ? OR m.arxiv_like_id LIKE ? OR m.doi LIKE ?
      LIMIT 20
    `).all(q, q, q + '%', q + '%');
    for (const r of idMatches) if (!seen.has(r.id)) { seen.add(r.id); rows.push(r); }

    // FTS over title + abstract + authors + pdf body.
    const ftsQ = escapeFtsQuery(q);
    if (ftsQ) {
      try {
        const ftsRows = db.prepare(`
          SELECT m.*, u.username AS submitter_username, u.display_name AS submitter_display
          FROM manuscripts m
          JOIN users u ON u.id = m.submitter_id
          JOIN manuscripts_fts fts ON fts.rowid = m.id
          WHERE manuscripts_fts MATCH ?
          ORDER BY rank
          LIMIT 100
        `).all(ftsQ);
        for (const r of ftsRows) if (!seen.has(r.id)) { seen.add(r.id); rows.push(r); }
      } catch (_e) {
        // bad query (rare) — fall through silently
      }
    }
  }
  const voteMap = req.user ? buildVoteMap(req.user.id, 'manuscript', rows.map(r => r.id)) : {};
  res.render('search', { manuscripts: rows, voteMap, q });
});

// ─── routes: submission ─────────────────────────────────────────────────────
function requireVerified(req, res, next) {
  if (!req.user) return res.redirect('/login?next=' + encodeURIComponent(req.originalUrl));
  const u = db.prepare('SELECT email_verified FROM users WHERE id = ?').get(req.user.id);
  if (!u || !u.email_verified) {
    flash(req, 'error', 'Please verify your email address before submitting.');
    return res.redirect('/verify-pending');
  }
  next();
}

app.get('/submit', requireVerified, (req, res) => {
  res.render('submit', { values: {}, errors: [] });
});

app.post('/submit', submitLimiter, requireVerified, (req, res, next) => {
  upload.single('pdf')(req, res, (err) => {
    if (err) {
      flash(req, 'error', err.message || 'Upload failed.');
      return res.redirect('/submit');
    }
    next();
  });
}, csrfCheckParsed, async (req, res) => {
  const errors = [];
  const v = {
    title: (req.body.title || '').trim(),
    abstract: (req.body.abstract || '').trim(),
    authors: (req.body.authors || '').trim(),
    category: (req.body.category || '').trim(),
    external_url: (req.body.external_url || '').trim() || null,
    conductor_ai_model: (req.body.conductor_ai_model || '').trim(),
    conductor_human: (req.body.conductor_human || '').trim(),
    conductor_role: (req.body.conductor_role || '').trim(),
    conductor_notes: (req.body.conductor_notes || '').trim() || null,
    has_auditor: req.body.has_auditor === 'on' || req.body.has_auditor === '1' || req.body.has_auditor === 'true',
    auditor_name: (req.body.auditor_name || '').trim(),
    auditor_affiliation: (req.body.auditor_affiliation || '').trim(),
    auditor_role: (req.body.auditor_role || '').trim(),
    auditor_statement: (req.body.auditor_statement || '').trim(),
    no_auditor_ack: req.body.no_auditor_ack === 'on' || req.body.no_auditor_ack === '1',
  };

  if (!v.title || v.title.length < 5)         errors.push('Title is required (≥ 5 characters).');
  if (v.title.length > 300)                   errors.push('Title is too long (≤ 300 characters).');
  if (!v.abstract || v.abstract.length < 50)  errors.push('Abstract is required (≥ 50 characters).');
  if (v.abstract.length > 5000)               errors.push('Abstract is too long (≤ 5000 characters).');
  if (!v.authors)                             errors.push('Authors line is required (e.g., "Jane Doe; Claude Opus 4.6").');
  if (!CATEGORIES.find(c => c.id === v.category)) errors.push('Pick a valid category.');
  if (!v.conductor_ai_model)                  errors.push('Conductor: AI model is required.');
  if (!v.conductor_human)                     errors.push('Conductor: human name is required.');
  if (!ROLES.includes(v.conductor_role))      errors.push('Conductor: pick a valid role.');

  if (v.has_auditor) {
    if (!v.auditor_name)                      errors.push('Auditor name is required when an auditor is listed.');
    if (!ROLES.includes(v.auditor_role))      errors.push('Auditor: pick a valid role.');
    if (!v.auditor_statement || v.auditor_statement.length < 20)
      errors.push('Auditor statement is required (≥ 20 characters).');
  } else if (!v.no_auditor_ack) {
    errors.push('You must acknowledge that without an auditor you are NOT responsible for the correctness of the work, and that this manuscript is unaudited.');
  }

  if (!req.file && !v.external_url) {
    errors.push('Provide either a PDF upload or an external URL to the manuscript.');
  }

  if (errors.length) {
    if (req.file) fs.unlink(req.file.path, () => {});
    return res.render('submit', { values: v, errors });
  }

  const arxivId = makeArxivLikeId();
  let   doi     = makeSyntheticDoi(arxivId); // may be replaced by Zenodo below
  const pdf_path = req.file ? '/uploads/' + path.basename(req.file.path) : null;

  // Best-effort PDF text extraction. Synchronous-ish (we await once before insert).
  let pdf_text = null;
  if (req.file) {
    pdf_text = await extractPdfText(req.file.path);
  }

  const r = db.prepare(`
    INSERT INTO manuscripts (
      arxiv_like_id, doi, submitter_id, title, abstract, authors, category, pdf_path, pdf_text, external_url,
      conductor_ai_model, conductor_human, conductor_role, conductor_notes,
      has_auditor, auditor_name, auditor_affiliation, auditor_role, auditor_statement,
      score
    ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, 1)
  `).run(
    arxivId, doi, req.user.id, v.title, v.abstract, v.authors, v.category, pdf_path, pdf_text, v.external_url,
    v.conductor_ai_model, v.conductor_human, v.conductor_role, v.conductor_notes,
    v.has_auditor ? 1 : 0,
    v.has_auditor ? v.auditor_name : null,
    v.has_auditor ? (v.auditor_affiliation || null) : null,
    v.has_auditor ? v.auditor_role : null,
    v.has_auditor ? v.auditor_statement : null,
  );
  // self-upvote
  db.prepare("INSERT INTO votes (user_id, target_type, target_id, value) VALUES (?, 'manuscript', ?, 1)")
    .run(req.user.id, r.lastInsertRowid);

  // Best-effort Zenodo deposition. If ZENODO_TOKEN is set we replace the
  // synthetic DOI with the real one from Zenodo. Failures are logged and
  // tolerated — the manuscript stays posted with the synthetic id.
  if (zenodo.enabled) {
    const base = (process.env.APP_URL || '').replace(/\/+$/, '') ||
      ((req.get('x-forwarded-proto') || (req.secure ? 'https' : 'http')) + '://' + req.get('host'));
    // Build the manuscript object the helper expects.
    const mForZenodo = {
      arxiv_like_id: arxivId, title: v.title, abstract: v.abstract,
      authors: v.authors, category: v.category,
      conductor_human: v.conductor_human, conductor_ai_model: v.conductor_ai_model,
      has_auditor: v.has_auditor, auditor_name: v.auditor_name,
    };
    zenodo.depositAndPublish(mForZenodo, base).then(zr => {
      if (zr.ok && zr.doi) {
        db.prepare('UPDATE manuscripts SET doi = ? WHERE id = ?').run(zr.doi, r.lastInsertRowid);
        console.log(`[zenodo] minted ${zr.doi} (${zr.sandbox ? 'sandbox' : 'production'}) for ${arxivId}`);
      }
    }).catch(() => {});
  }

  flash(req, 'ok', 'Manuscript posted as ' + arxivId + '.');
  res.redirect('/m/' + arxivId);
});

// ─── routes: manuscript detail + comments ───────────────────────────────────
app.get('/m/:id', (req, res) => {
  const m = db.prepare(`
    SELECT m.*, u.username AS submitter_username, u.display_name AS submitter_display
    FROM manuscripts m JOIN users u ON u.id = m.submitter_id
    WHERE m.arxiv_like_id = ? OR m.id = ?
  `).get(req.params.id, req.params.id);
  if (!m) return res.status(404).render('error', { code: 404, msg: 'Manuscript not found.' });

  db.prepare('UPDATE manuscripts SET view_count = view_count + 1 WHERE id = ?').run(m.id);

  const comments = db.prepare(`
    SELECT c.*, u.username, u.display_name FROM comments c
    JOIN users u ON u.id = c.author_id
    WHERE c.manuscript_id = ?
    ORDER BY c.created_at ASC
  `).all(m.id);

  // build a tree
  const byId = {};
  for (const c of comments) { c.children = []; byId[c.id] = c; }
  const top = [];
  for (const c of comments) {
    if (c.parent_id && byId[c.parent_id]) byId[c.parent_id].children.push(c);
    else top.push(c);
  }

  const myMsVote   = req.user ? (db.prepare("SELECT value FROM votes WHERE user_id = ? AND target_type = 'manuscript' AND target_id = ?").get(req.user.id, m.id) || {}).value : null;
  const cVoteMap   = req.user ? buildVoteMap(req.user.id, 'comment', comments.map(c => c.id)) : {};

  res.render('manuscript', { m, comments: top, allComments: comments, myMsVote, cVoteMap });
});

app.post('/m/:id/comment', commentLimiter, requireAuth, (req, res) => {
  const m = db.prepare('SELECT id FROM manuscripts WHERE arxiv_like_id = ? OR id = ?').get(req.params.id, req.params.id);
  if (!m) return res.status(404).render('error', { code: 404, msg: 'Manuscript not found.' });
  const content = (req.body.content || '').trim();
  const parentId = req.body.parent_id ? parseInt(req.body.parent_id, 10) : null;
  if (!content || content.length < 2) {
    flash(req, 'error', 'Comment cannot be empty.');
    return res.redirect('/m/' + req.params.id);
  }
  if (content.length > 8000) {
    flash(req, 'error', 'Comment is too long.');
    return res.redirect('/m/' + req.params.id);
  }
  const r = db.prepare('INSERT INTO comments (manuscript_id, author_id, parent_id, content, score) VALUES (?, ?, ?, ?, 1)')
    .run(m.id, req.user.id, parentId, content);
  db.prepare("INSERT INTO votes (user_id, target_type, target_id, value) VALUES (?, 'comment', ?, 1)")
    .run(req.user.id, r.lastInsertRowid);
  db.prepare('UPDATE manuscripts SET comment_count = (SELECT COUNT(*) FROM comments WHERE manuscript_id = ?) WHERE id = ?').run(m.id, m.id);
  res.redirect('/m/' + req.params.id + '#c' + r.lastInsertRowid);
});

// ─── routes: voting ─────────────────────────────────────────────────────────
function applyVote(userId, type, targetId, value) {
  const table = type === 'manuscript' ? 'manuscripts' : 'comments';
  const existing = db.prepare('SELECT value FROM votes WHERE user_id = ? AND target_type = ? AND target_id = ?')
    .get(userId, type, targetId);
  let delta = 0;
  if (!existing) {
    db.prepare('INSERT INTO votes (user_id, target_type, target_id, value) VALUES (?, ?, ?, ?)')
      .run(userId, type, targetId, value);
    delta = value;
  } else if (existing.value === value) {
    db.prepare('DELETE FROM votes WHERE user_id = ? AND target_type = ? AND target_id = ?')
      .run(userId, type, targetId);
    delta = -value;
  } else {
    db.prepare('UPDATE votes SET value = ? WHERE user_id = ? AND target_type = ? AND target_id = ?')
      .run(value, userId, type, targetId);
    delta = value - existing.value;
  }
  db.prepare(`UPDATE ${table} SET score = score + ? WHERE id = ?`).run(delta, targetId);
  // update author karma
  const authorCol = type === 'manuscript' ? 'submitter_id' : 'author_id';
  const author = db.prepare(`SELECT ${authorCol} AS aid FROM ${table} WHERE id = ?`).get(targetId);
  if (author && author.aid !== userId) {
    db.prepare('UPDATE users SET karma = karma + ? WHERE id = ?').run(delta, author.aid);
  }
  return db.prepare(`SELECT score FROM ${table} WHERE id = ?`).get(targetId).score;
}

app.post('/vote/:type/:id', voteLimiter, requireAuth, (req, res) => {
  const type = req.params.type;
  if (type !== 'manuscript' && type !== 'comment') return res.status(400).json({ error: 'bad type' });
  const id = parseInt(req.params.id, 10);
  const value = parseInt(req.body.value, 10);
  if (![1, -1].includes(value)) return res.status(400).json({ error: 'bad value' });
  const newScore = applyVote(req.user.id, type, id, value);
  const myVote = (db.prepare('SELECT value FROM votes WHERE user_id = ? AND target_type = ? AND target_id = ?').get(req.user.id, type, id) || {}).value || 0;
  if (req.headers.accept && req.headers.accept.includes('application/json')) {
    return res.json({ score: newScore, myVote });
  }
  res.redirect(req.get('Referer') || '/');
});

// ─── routes: auth ───────────────────────────────────────────────────────────
app.get('/login', (req, res) => {
  if (req.user) return res.redirect('/');
  res.render('login', { values: {}, errors: [], next: req.query.next || '/' });
});

app.post('/login', authLimiter, (req, res) => {
  const username = (req.body.username || '').trim();
  const password = req.body.password || '';
  const next = (req.body.next && /^\/[^/]/.test(req.body.next)) ? req.body.next : '/';
  const errors = [];
  if (!username || !password) errors.push('Username and password are required.');
  let user;
  if (!errors.length) {
    user = db.prepare('SELECT id, password_hash FROM users WHERE username = ? OR email = ?').get(username, username);
    if (!user || !verifyPassword(password, user.password_hash)) {
      errors.push('Invalid username or password.');
    }
  }
  if (errors.length) return res.render('login', { values: { username }, errors, next });
  req.session.userId = user.id;
  flash(req, 'ok', 'Welcome back.');
  res.redirect(next);
});

// ─── CAPTCHA (simple math) ──────────────────────────────────────────────────
function freshCaptcha(req) {
  const a = 1 + Math.floor(Math.random() * 9);
  const b = 1 + Math.floor(Math.random() * 9);
  const op = Math.random() < 0.5 ? '+' : (a >= b ? '-' : '+');
  const answer = op === '+' ? a + b : a - b;
  req.session.captcha = { a, b, op, answer, issuedAt: Date.now() };
  return req.session.captcha;
}
function verifyCaptcha(req) {
  const c = req.session.captcha;
  if (!c) return false;
  if (Date.now() - c.issuedAt > 1000 * 60 * 30) return false; // 30 min validity
  const guess = parseInt((req.body.captcha || '').trim(), 10);
  return Number.isFinite(guess) && guess === c.answer;
}

app.get('/register', (req, res) => {
  if (req.user) return res.redirect('/');
  const captcha = freshCaptcha(req);
  res.render('register', { values: {}, errors: [], captcha });
});

app.post('/register', authLimiter, async (req, res) => {
  const username     = (req.body.username || '').trim();
  const email        = (req.body.email || '').trim().toLowerCase();
  const password     = req.body.password || '';
  const display_name = (req.body.display_name || '').trim() || null;
  const affiliation  = (req.body.affiliation || '').trim() || null;
  const errors = [];
  const uErr = validateUsername(username);
  if (uErr) errors.push(uErr);
  if (!email || !/^[^@\s]+@[^@\s]+\.[^@\s]+$/.test(email)) errors.push('A valid email is required.');
  if (!password || password.length < 8) errors.push('Password must be ≥ 8 characters.');
  if (!verifyCaptcha(req)) errors.push('CAPTCHA answer is incorrect.');
  if (!errors.length) {
    const dup = db.prepare('SELECT 1 FROM users WHERE username = ? OR email = ?').get(username, email);
    if (dup) errors.push('That username or email is already in use.');
  }
  if (errors.length) {
    return res.render('register', {
      values: { username, email, display_name, affiliation },
      errors,
      captcha: freshCaptcha(req),
    });
  }
  const verifyToken = crypto.randomBytes(24).toString('base64url');
  const verifyExpires = Date.now() + 1000 * 60 * 60 * 24 * 3; // 3 days
  const r = db.prepare(`
    INSERT INTO users (username, email, password_hash, display_name, affiliation,
                       email_verified, email_verify_token, email_verify_expires)
    VALUES (?, ?, ?, ?, ?, 0, ?, ?)
  `).run(username, email, hashPassword(password), display_name, affiliation, verifyToken, verifyExpires);
  req.session.userId = r.lastInsertRowid;

  const verifyLink = absoluteUrl(req, '/verify/' + verifyToken);
  const result = await sendMail({
    to: email,
    subject: 'Verify your email for pre-arxiv',
    text:
`Welcome to pre-arxiv.

Please confirm your email address by visiting:

  ${verifyLink}

This link expires in 3 days. If you didn't sign up, you can ignore this email.`,
  });

  // In dev/no-SMTP mode, surface the link directly on the next page.
  req.session.lastVerifyLink = result.devMode ? verifyLink : null;
  res.redirect('/verify-pending');
});

app.get('/verify-pending', (req, res) => {
  if (!req.user) return res.redirect('/');
  const u = db.prepare('SELECT email, email_verified FROM users WHERE id = ?').get(req.user.id);
  if (u && u.email_verified) return res.redirect('/');
  const link = req.session.lastVerifyLink || null;
  delete req.session.lastVerifyLink;
  res.render('verify_pending', { email: u ? u.email : '', devLink: link });
});

app.get('/verify/:token', (req, res) => {
  const tok = req.params.token;
  const u = db.prepare('SELECT id, email_verify_expires FROM users WHERE email_verify_token = ?').get(tok);
  if (!u) {
    return res.status(400).render('error', { code: 400, msg: 'Verification link is invalid or has already been used.' });
  }
  if (u.email_verify_expires && u.email_verify_expires < Date.now()) {
    return res.status(400).render('error', { code: 400, msg: 'Verification link has expired. Request a new one.' });
  }
  db.prepare(`UPDATE users SET email_verified = 1, email_verify_token = NULL, email_verify_expires = NULL WHERE id = ?`).run(u.id);
  if (!req.user) req.session.userId = u.id;
  flash(req, 'ok', 'Email verified. You can now submit manuscripts.');
  res.redirect('/');
});

app.post('/verify/resend', authLimiter, requireAuth, async (req, res) => {
  const u = db.prepare('SELECT id, email, email_verified FROM users WHERE id = ?').get(req.user.id);
  if (!u) return res.redirect('/');
  if (u.email_verified) { flash(req, 'ok', 'Already verified.'); return res.redirect('/'); }
  const verifyToken = crypto.randomBytes(24).toString('base64url');
  const verifyExpires = Date.now() + 1000 * 60 * 60 * 24 * 3;
  db.prepare('UPDATE users SET email_verify_token = ?, email_verify_expires = ? WHERE id = ?')
    .run(verifyToken, verifyExpires, u.id);
  const link = absoluteUrl(req, '/verify/' + verifyToken);
  const result = await sendMail({
    to: u.email,
    subject: 'New pre-arxiv verification link',
    text: `Use this link to verify your email:\n\n  ${link}\n\nThis one expires in 3 days.`,
  });
  req.session.lastVerifyLink = result.devMode ? link : null;
  res.redirect('/verify-pending');
});

app.post('/logout', (req, res) => {
  req.session.destroy(() => res.redirect('/'));
});

// ─── routes: password reset ─────────────────────────────────────────────────
app.get('/forgot', (req, res) => {
  res.render('forgot', { values: {}, errors: [], devLink: req.session.lastResetLink || null });
  delete req.session.lastResetLink;
});

app.post('/forgot', authLimiter, async (req, res) => {
  const email = (req.body.email || '').trim().toLowerCase();
  if (!email) return res.render('forgot', { values: {}, errors: ['Email is required.'], devLink: null });

  const u = db.prepare('SELECT id FROM users WHERE email = ?').get(email);
  // Generic response either way to avoid email enumeration
  let devLink = null;
  if (u) {
    const token = crypto.randomBytes(24).toString('base64url');
    const expires = Date.now() + 1000 * 60 * 60; // 1 hour
    db.prepare('UPDATE users SET password_reset_token = ?, password_reset_expires = ? WHERE id = ?')
      .run(token, expires, u.id);
    const link = absoluteUrl(req, '/reset/' + token);
    const result = await sendMail({
      to: email,
      subject: 'pre-arxiv password reset',
      text: `A password reset was requested for this email.\n\nIf it was you, follow this link within 1 hour:\n\n  ${link}\n\nIf it wasn't, ignore this message — nothing has changed.`,
    });
    if (result.devMode) devLink = link;
  }
  req.session.lastResetLink = devLink;
  flash(req, 'ok', 'If an account exists for that email, a reset link has been sent.');
  res.redirect('/forgot');
});

app.get('/reset/:token', (req, res) => {
  const u = db.prepare('SELECT id, password_reset_expires FROM users WHERE password_reset_token = ?').get(req.params.token);
  if (!u || (u.password_reset_expires && u.password_reset_expires < Date.now())) {
    return res.status(400).render('error', { code: 400, msg: 'Reset link is invalid or has expired.' });
  }
  res.render('reset', { token: req.params.token, errors: [] });
});

app.post('/reset/:token', authLimiter, (req, res) => {
  const password = req.body.password || '';
  const password2 = req.body.password2 || '';
  const errors = [];
  if (!password || password.length < 8) errors.push('Password must be ≥ 8 characters.');
  if (password !== password2)             errors.push('Passwords do not match.');
  const u = db.prepare('SELECT id, password_reset_expires FROM users WHERE password_reset_token = ?').get(req.params.token);
  if (!u || (u.password_reset_expires && u.password_reset_expires < Date.now())) {
    return res.status(400).render('error', { code: 400, msg: 'Reset link is invalid or has expired.' });
  }
  if (errors.length) return res.render('reset', { token: req.params.token, errors });
  db.prepare('UPDATE users SET password_hash = ?, password_reset_token = NULL, password_reset_expires = NULL WHERE id = ?')
    .run(hashPassword(password), u.id);
  flash(req, 'ok', 'Password updated. Log in with your new password.');
  res.redirect('/login');
});

// ─── routes: user profile ───────────────────────────────────────────────────
app.get('/u/:username', (req, res) => {
  const u = db.prepare('SELECT id, username, display_name, affiliation, bio, karma, created_at FROM users WHERE username = ?').get(req.params.username);
  if (!u) return res.status(404).render('error', { code: 404, msg: 'No such user.' });
  const submissions = db.prepare(`
    SELECT m.*, ? AS submitter_username, ? AS submitter_display
    FROM manuscripts m WHERE m.submitter_id = ?
    ORDER BY m.created_at DESC LIMIT 50
  `).all(u.username, u.display_name, u.id);
  const conductedAs = db.prepare(`
    SELECT m.*, uu.username AS submitter_username, uu.display_name AS submitter_display
    FROM manuscripts m JOIN users uu ON uu.id = m.submitter_id
    WHERE m.conductor_human = ? OR m.conductor_human = ?
    ORDER BY m.created_at DESC LIMIT 50
  `).all(u.display_name || '', u.username);
  const auditedCount = db.prepare(`SELECT COUNT(*) AS n FROM manuscripts WHERE auditor_name LIKE ? OR auditor_name = ?`).get(`%${u.display_name || u.username}%`, u.username).n;
  res.render('user', { profile: u, submissions, conductedAs, auditedCount });
});

// ─── routes: moderation (withdraw / delete / flag / admin queue) ───────────
function isAdmin(user) { return !!(user && db.prepare('SELECT is_admin FROM users WHERE id = ?').get(user.id)?.is_admin); }
function requireAdmin(req, res, next) {
  if (!req.user) return res.redirect('/login?next=' + encodeURIComponent(req.originalUrl));
  if (!isAdmin(req.user)) return res.status(403).render('error', { code: 403, msg: 'Admin only.' });
  next();
}

app.post('/m/:id/withdraw', requireAuth, (req, res) => {
  const m = db.prepare('SELECT id, submitter_id FROM manuscripts WHERE arxiv_like_id = ? OR id = ?').get(req.params.id, req.params.id);
  if (!m) return res.status(404).render('error', { code: 404, msg: 'Manuscript not found.' });
  const allowed = (m.submitter_id === req.user.id) || isAdmin(req.user);
  if (!allowed) return res.status(403).render('error', { code: 403, msg: 'You can only withdraw your own manuscripts.' });
  const reason = (req.body.reason || '').trim().slice(0, 500) || 'No reason given.';
  db.prepare(`UPDATE manuscripts SET withdrawn = 1, withdrawn_reason = ?, withdrawn_at = CURRENT_TIMESTAMP WHERE id = ?`)
    .run(reason, m.id);
  flash(req, 'ok', 'Manuscript withdrawn. The page now shows a tombstone.');
  res.redirect('/m/' + req.params.id);
});

app.post('/m/:id/delete', requireAdmin, (req, res) => {
  const m = db.prepare('SELECT id, pdf_path FROM manuscripts WHERE arxiv_like_id = ? OR id = ?').get(req.params.id, req.params.id);
  if (!m) return res.status(404).render('error', { code: 404, msg: 'Manuscript not found.' });
  // best-effort PDF cleanup
  if (m.pdf_path) {
    const p = path.join(__dirname, 'public', m.pdf_path.replace(/^\//, ''));
    fs.unlink(p, () => {});
  }
  db.prepare('DELETE FROM manuscripts WHERE id = ?').run(m.id);
  flash(req, 'ok', 'Manuscript deleted.');
  res.redirect('/');
});

app.post('/comment/:id/delete', requireAuth, (req, res) => {
  const c = db.prepare('SELECT c.id, c.author_id, c.manuscript_id, m.arxiv_like_id FROM comments c JOIN manuscripts m ON m.id = c.manuscript_id WHERE c.id = ?').get(req.params.id);
  if (!c) return res.status(404).render('error', { code: 404, msg: 'Comment not found.' });
  const allowed = (c.author_id === req.user.id) || isAdmin(req.user);
  if (!allowed) return res.status(403).render('error', { code: 403, msg: 'You can only delete your own comments.' });
  db.prepare('DELETE FROM comments WHERE id = ?').run(c.id);
  db.prepare('UPDATE manuscripts SET comment_count = (SELECT COUNT(*) FROM comments WHERE manuscript_id = ?) WHERE id = ?').run(c.manuscript_id, c.manuscript_id);
  res.redirect('/m/' + c.arxiv_like_id);
});

app.post('/flag/:type/:id', requireAuth, (req, res) => {
  const type = req.params.type;
  if (type !== 'manuscript' && type !== 'comment') return res.status(400).render('error', { code: 400, msg: 'Bad flag target.' });
  const targetId = parseInt(req.params.id, 10);
  if (!targetId) return res.status(400).render('error', { code: 400, msg: 'Bad target id.' });
  const reason = (req.body.reason || '').trim().slice(0, 1000);
  if (!reason || reason.length < 5) {
    flash(req, 'error', 'Please give a brief reason for the flag (≥ 5 characters).');
    return res.redirect(req.get('Referer') || '/');
  }
  try {
    db.prepare('INSERT INTO flag_reports (target_type, target_id, reporter_id, reason) VALUES (?, ?, ?, ?)')
      .run(type, targetId, req.user.id, reason);
    flash(req, 'ok', 'Thanks — flagged for review.');
  } catch (e) {
    if (/UNIQUE/.test(e.message)) {
      flash(req, 'ok', 'You have already flagged this. The moderators will see it.');
    } else throw e;
  }
  res.redirect(req.get('Referer') || '/');
});

app.get('/admin', requireAdmin, (req, res) => {
  const flags = db.prepare(`
    SELECT f.*, u.username AS reporter_username
    FROM flag_reports f JOIN users u ON u.id = f.reporter_id
    WHERE f.resolved = 0
    ORDER BY f.created_at DESC LIMIT 200
  `).all();
  // hydrate target data
  const enriched = flags.map(f => {
    if (f.target_type === 'manuscript') {
      const m = db.prepare('SELECT id, arxiv_like_id, title, withdrawn FROM manuscripts WHERE id = ?').get(f.target_id);
      return { ...f, target: m, targetUrl: m ? '/m/' + m.arxiv_like_id : null };
    } else {
      const c = db.prepare(`
        SELECT c.id, c.content, c.author_id, m.arxiv_like_id, u.username AS author_username
        FROM comments c JOIN manuscripts m ON m.id = c.manuscript_id
        JOIN users u ON u.id = c.author_id
        WHERE c.id = ?
      `).get(f.target_id);
      return { ...f, target: c, targetUrl: c ? '/m/' + c.arxiv_like_id + '#c' + c.id : null };
    }
  });
  res.render('admin', { flags: enriched });
});

app.post('/admin/flag/:id/resolve', requireAdmin, (req, res) => {
  const id = parseInt(req.params.id, 10);
  const note = (req.body.note || '').trim().slice(0, 500);
  db.prepare(`UPDATE flag_reports SET resolved = 1, resolved_by_id = ?, resolved_at = CURRENT_TIMESTAMP, resolution_note = ? WHERE id = ?`)
    .run(req.user.id, note || null, id);
  res.redirect('/admin');
});

// ─── routes: citation export ────────────────────────────────────────────────
function getManuscriptForCite(idOrSlug) {
  return db.prepare(`
    SELECT m.*, u.username AS submitter_username, u.display_name AS submitter_display
    FROM manuscripts m JOIN users u ON u.id = m.submitter_id
    WHERE m.arxiv_like_id = ? OR m.id = ?
  `).get(idOrSlug, idOrSlug);
}
function citeBaseUrl(req) {
  if (process.env.APP_URL) return process.env.APP_URL.replace(/\/+$/, '');
  const proto = req.get('x-forwarded-proto') || (req.secure ? 'https' : 'http');
  return proto + '://' + req.get('host');
}
app.get('/m/:id/cite', (req, res) => {
  const m = getManuscriptForCite(req.params.id);
  if (!m) return res.status(404).render('error', { code: 404, msg: 'Manuscript not found.' });
  const base = citeBaseUrl(req);
  res.render('cite', {
    m,
    bib:   renderBibtex(m, base),
    ris:   renderRis(m, base),
    plain: renderPlain(m, base),
  });
});
app.get('/m/:id/cite.bib', (req, res) => {
  const m = getManuscriptForCite(req.params.id);
  if (!m) return res.status(404).type('text/plain').send('not found');
  res.type('application/x-bibtex').send(renderBibtex(m, citeBaseUrl(req)));
});
app.get('/m/:id/cite.ris', (req, res) => {
  const m = getManuscriptForCite(req.params.id);
  if (!m) return res.status(404).type('text/plain').send('not found');
  res.type('application/x-research-info-systems').send(renderRis(m, citeBaseUrl(req)));
});

// ─── routes: about / static ─────────────────────────────────────────────────
app.get('/about', (req, res) => res.render('about'));
app.get('/guidelines', (req, res) => res.render('guidelines'));

// ─── 404 ────────────────────────────────────────────────────────────────────
app.use((req, res) => {
  res.status(404).render('error', { code: 404, msg: 'Page not found.' });
});

app.use((err, req, res, _next) => {
  console.error(err);
  res.status(500).render('error', { code: 500, msg: 'Something went wrong on our end.' });
});

if (require.main === module) {
  app.listen(PORT, () => {
    console.log(`pre-arxiv listening on http://localhost:${PORT}`);
  });
}

module.exports = app;
