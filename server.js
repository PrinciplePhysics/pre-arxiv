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
  let rows = [];
  if (q) {
    const like = '%' + q.replace(/[%_]/g, m => '\\' + m) + '%';
    rows = db.prepare(`
      SELECT m.*, u.username AS submitter_username, u.display_name AS submitter_display
      FROM manuscripts m JOIN users u ON u.id = m.submitter_id
      WHERE m.title LIKE ? ESCAPE '\\' OR m.abstract LIKE ? ESCAPE '\\' OR m.authors LIKE ? ESCAPE '\\' OR m.arxiv_like_id LIKE ? ESCAPE '\\'
      ORDER BY m.created_at DESC
      LIMIT 100
    `).all(like, like, like, like);
  }
  const voteMap = req.user ? buildVoteMap(req.user.id, 'manuscript', rows.map(r => r.id)) : {};
  res.render('search', { manuscripts: rows, voteMap, q });
});

// ─── routes: submission ─────────────────────────────────────────────────────
app.get('/submit', requireAuth, (req, res) => {
  res.render('submit', { values: {}, errors: [] });
});

app.post('/submit', submitLimiter, requireAuth, (req, res, next) => {
  upload.single('pdf')(req, res, (err) => {
    if (err) {
      flash(req, 'error', err.message || 'Upload failed.');
      return res.redirect('/submit');
    }
    next();
  });
}, csrfCheckParsed, (req, res) => {
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
  const pdf_path = req.file ? '/uploads/' + path.basename(req.file.path) : null;

  const r = db.prepare(`
    INSERT INTO manuscripts (
      arxiv_like_id, submitter_id, title, abstract, authors, category, pdf_path, external_url,
      conductor_ai_model, conductor_human, conductor_role, conductor_notes,
      has_auditor, auditor_name, auditor_affiliation, auditor_role, auditor_statement,
      score
    ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, 1)
  `).run(
    arxivId, req.user.id, v.title, v.abstract, v.authors, v.category, pdf_path, v.external_url,
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

app.get('/register', (req, res) => {
  if (req.user) return res.redirect('/');
  res.render('register', { values: {}, errors: [] });
});

app.post('/register', authLimiter, (req, res) => {
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
  if (!errors.length) {
    const dup = db.prepare('SELECT 1 FROM users WHERE username = ? OR email = ?').get(username, email);
    if (dup) errors.push('That username or email is already in use.');
  }
  if (errors.length) {
    return res.render('register', { values: { username, email, display_name, affiliation }, errors });
  }
  const r = db.prepare(`
    INSERT INTO users (username, email, password_hash, display_name, affiliation)
    VALUES (?, ?, ?, ?, ?)
  `).run(username, email, hashPassword(password), display_name, affiliation);
  req.session.userId = r.lastInsertRowid;
  flash(req, 'ok', 'Welcome to pre-arxiv.');
  res.redirect('/');
});

app.post('/logout', (req, res) => {
  req.session.destroy(() => res.redirect('/'));
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
