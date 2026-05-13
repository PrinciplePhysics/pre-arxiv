// PreXiv — Express orchestrator.
//
// This file used to carry every route inline; the per-feature handlers now
// live in routes/*.js and shared helpers in lib/helpers.js. server.js is
// responsible for:
//   1. Process-level config (port, secrets, data dir, upload dir).
//   2. Middleware setup (helmet, body parsers, sessions, CSRF, rate limiters).
//   3. Building the helper bag (`deps`) shared by every routes module.
//   4. Calling each route module's `register(app, deps)` in the right order.
//   5. Mounting the JSON API and the 404 / 500 fallbacks.
//
// To add a route to PreXiv: pick the right routes/*.js file (or add a new
// one), pull anything new it needs out of `deps`, and register it inside
// that file's `register(app, deps)`. Do not add `app.<verb>(...)` calls
// here — keep this file an orchestrator.

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
const { loadUser } = require('./lib/auth');
const { timeAgo, escapeHtml, renderMarkdown } = require('./lib/util');
const { extractBearer } = require('./lib/api-auth');
const { buildApiRouter } = require('./lib/api');
const { auditLog } = require('./lib/audit');
const helpers = require('./lib/helpers');
const zenodo = require('./lib/zenodo');

// Best-effort optional modules — server.js stays bootable on a stripped-down
// tree (e.g. before the parallel agent's manifest/versions/oai/webhooks land).
/** @type {{snapshotManuscriptVersion?:any, currentManuscriptVersionNumber?:any, listVersions?:any, unifiedDiff?:any}} */
let versionsLib = {};
try { versionsLib = require('./lib/versions'); } catch (_e) { /* optional */ }
/** @type {any} */
let webhooksLib = null;
try { webhooksLib = require('./lib/webhooks'); } catch (_e) { /* optional */ }

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

// ─── structured access log ──────────────────────────────────────────────────
// One JSON line per finished request, on stdout. Skips /healthz and /readyz.
app.use((req, res, next) => {
  if (req.path === '/healthz' || req.path === '/readyz') return next();
  const start = process.hrtime.bigint();
  res.on('finish', () => {
    const ms = Number((process.hrtime.bigint() - start) / 1000000n);
    /** @type {Record<string, any>} */
    const entry = {
      ts:     new Date().toISOString(),
      method: req.method,
      path:   req.originalUrl || req.url,
      status: res.statusCode,
      ms,
      ip:     req.ip,
    };
    if (req.user && req.user.id) entry.user_id = req.user.id;
    try { process.stdout.write(JSON.stringify(entry) + '\n'); } catch { /* ignore */ }
  });
  next();
});

// Liveness / readiness probes BEFORE helmet/session/CSRF so monitors get a
// cheap, side-effect-free answer that doesn't depend on cookie machinery.
require('./routes/discovery').register(app, {});

// ─── security headers ───────────────────────────────────────────────────────
// `upgrade-insecure-requests` is helmet's default but in dev (HTTP localhost)
// Safari honors it strictly and tries to fetch /css/style.css over HTTPS,
// which the server doesn't listen on — the stylesheet then silently fails to
// load and the page renders unstyled. Chrome has a localhost exception;
// Safari does not. We enable the directive only in production.
app.use(helmet({
  contentSecurityPolicy: {
    directives: {
      defaultSrc: ["'self'"],
      scriptSrc:  ["'self'", "'unsafe-inline'", 'https://cdn.jsdelivr.net'],
      styleSrc:   ["'self'", "'unsafe-inline'", 'https://cdn.jsdelivr.net', 'https://fonts.googleapis.com'],
      fontSrc:    ["'self'", 'https://cdn.jsdelivr.net', 'https://fonts.gstatic.com', 'data:'],
      imgSrc:     ["'self'", 'data:'],
      connectSrc: ["'self'"],
      objectSrc:  ["'none'"],
      frameAncestors: ["'self'"],
      // helmet defaults to []; setting to null removes the directive entirely
      // in dev so Safari doesn't try to upgrade HTTP→HTTPS on localhost.
      upgradeInsecureRequests: IS_PROD ? [] : null,
    },
  },
  crossOriginEmbedderPolicy: false,
}));

// Force PDF downloads to be served with `Content-Disposition: attachment`.
app.use('/uploads', (req, res, next) => {
  if (req.path.toLowerCase().endsWith('.pdf')) {
    const fname = path.basename(req.path) || 'manuscript.pdf';
    const safe = fname.replace(/[\r\n"]/g, '');
    res.setHeader('Content-Disposition', `attachment; filename="${safe}"`);
    res.setHeader('X-Content-Type-Options', 'nosniff');
  }
  next();
}, express.static(UPLOAD_DIR));
app.use(express.static(path.join(__dirname, 'public')));
app.use(express.urlencoded({ extended: true, limit: '2mb' }));
app.use(express.json({ limit: '2mb' }));
app.use((err, req, res, next) => {
  if (err && err.type === 'entity.parse.failed') {
    if (req.path.startsWith('/api/v1/')) {
      return res.status(400).json({ error: 'Invalid JSON.' });
    }
    return res.status(400).type('text/plain').send('Invalid request body.');
  }
  next(err);
});

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

// ─── per-request locals (admin flag, unread count, theme cookie) ───────────
app.use((req, res, next) => {
  res.locals.isAdmin = false;
  if (req.user) {
    const row = db.prepare('SELECT is_admin FROM users WHERE id = ?').get(req.user.id);
    res.locals.isAdmin = !!(row && row.is_admin);
  }
  next();
});

function parseCookies(req) {
  /** @type {Record<string, string>} */
  const out = {};
  const h = req.headers && req.headers.cookie;
  if (!h || typeof h !== 'string') return out;
  for (const part of h.split(/;\s*/)) {
    if (!part) continue;
    const eq = part.indexOf('=');
    if (eq < 0) continue;
    const k = part.slice(0, eq).trim();
    const v = part.slice(eq + 1).trim();
    if (k) {
      try { out[k] = decodeURIComponent(v); } catch { out[k] = v; }
    }
  }
  return out;
}

app.use((req, res, next) => {
  res.locals.unreadCount = 0;
  if (req.user) {
    try {
      const row = db.prepare('SELECT COUNT(*) AS n FROM notifications WHERE user_id = ? AND seen = 0').get(req.user.id);
      res.locals.unreadCount = row ? row.n : 0;
    } catch (_e) { /* table not ready yet */ }
  }
  const cookies = parseCookies(req);
  const t = cookies.prexiv_theme;
  res.locals.theme = (t === 'light' || t === 'dark' || t === 'auto') ? t : 'auto';
  next();
});

// ─── notifications + webhook helpers (shared with routes + API) ─────────────
function createNotification(userId, kind, opts = {}) {
  if (!userId) return;
  const actorId = opts.actor_id || null;
  if (actorId && actorId === userId) return; // never notify yourself
  try {
    db.prepare(`
      INSERT INTO notifications (user_id, kind, actor_id, manuscript_id, comment_id)
      VALUES (?, ?, ?, ?, ?)
    `).run(userId, kind, actorId, opts.manuscript_id || null, opts.comment_id || null);
  } catch (e) {
    console.warn('[notif] insert failed:', e.message);
  }
}
app.locals.createNotification = createNotification;

/**
 * Build a JSON-safe webhook payload from a manuscript row.
 * @param {any} m
 * @returns {object|null}
 */
function manuscriptWebhookPayload(m) {
  if (!m) return null;
  return {
    id: m.id,
    arxiv_like_id: m.arxiv_like_id,
    doi: m.doi,
    title: m.title,
    authors: m.authors,
    category: m.category,
    abstract: m.abstract,
    submitter_username: m.submitter_username || null,
    conductor_type: m.conductor_type,
    has_auditor: !!m.has_auditor,
    withdrawn: !!m.withdrawn,
    created_at: m.created_at,
    updated_at: m.updated_at || null,
  };
}

function safeEmit(event, payload) {
  if (!webhooksLib) return;
  try { webhooksLib.emit(event, payload); } catch (e) {
    console.warn('[webhook] emit failed:', e.message || e);
  }
}
app.locals.emitWebhook = safeEmit;
app.locals.manuscriptWebhookPayload = manuscriptWebhookPayload;
app.locals.parseSearchFilters = helpers.parseSearchFilters;

// ─── CSRF (hand-rolled double-submit using session token) ───────────────────
function csrfTokenFor(req) {
  if (!req.session.csrfToken) {
    req.session.csrfToken = crypto.randomBytes(24).toString('base64url');
  }
  return req.session.csrfToken;
}
function isMultipartUploadRoute(req) {
  return req.method === 'POST' && (
    req.path === '/submit' ||
    /^\/m\/[^/]+\/edit$/.test(req.path)
  );
}
function verifyCsrf(req, res, next) {
  if (req.method === 'GET' || req.method === 'HEAD' || req.method === 'OPTIONS') return next();
  if (req.path.startsWith('/api/v1/')) return next();
  const ct = (req.get('Content-Type') || '').toLowerCase();
  if (ct.startsWith('multipart/form-data')) {
    if (isMultipartUploadRoute(req)) return next();
    return res.status(403).render('error', { code: 403, msg: 'CSRF check failed. Reload the page and try again.' });
  }
  return csrfCheckParsed(req, res, next);
}
function csrfCheckParsed(req, res, next) {
  if (extractBearer(req) && req.user && req.user._api_token_id) return next();
  const token = (req.body && req.body._csrf) || req.get('X-CSRF-Token');
  if (!token || !req.session.csrfToken || token !== req.session.csrfToken) {
    helpers.cleanupUploadedRequestFiles(req);
    return res.status(403).render('error', { code: 403, msg: 'CSRF check failed. Reload the page and try again.' });
  }
  next();
}

// expose helpers in templates
app.use((req, res, next) => {
  const isApi = req.path.startsWith('/api/v1/');
  res.locals.timeAgo = timeAgo;
  res.locals.escapeHtml = escapeHtml;
  res.locals.renderMarkdown = renderMarkdown;
  res.locals.isSafeExternalUrl = helpers.isSafeExternalUrl;
  res.locals.CATEGORIES = CATEGORIES;
  res.locals.ROLES = ROLES;
  res.locals.flash = isApi ? null : (req.session.flash || null);
  if (!isApi) delete req.session.flash;
  res.locals.path = req.path;
  res.locals.currentQuery = req.query;
  res.locals.csrfToken = isApi ? null : csrfTokenFor(req);
  res.locals.cookieConsentSet = false;
  if (!isApi) {
    const ck = (req.headers && req.headers.cookie) || '';
    res.locals.cookieConsentSet = /(?:^|;\s*)prexiv_cookie_consent=1\b/.test(ck);
  }
  next();
});

app.use(verifyCsrf);

// ─── rate limiting ──────────────────────────────────────────────────────────
const limit = (windowMs, max, message) => rateLimit({
  windowMs, max, standardHeaders: true, legacyHeaders: false,
  message: { error: message },
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
    const okMime = file.mimetype === 'application/pdf';
    const okExt  = file.originalname.toLowerCase().endsWith('.pdf');
    const ok = okMime && okExt;
    cb(ok ? null : new Error('Only PDF files are allowed (mime + .pdf extension required).'), ok);
  }
});

// ─── route registration ─────────────────────────────────────────────────────
const requireVerified = helpers.buildRequireVerified();

// `deps` carries everything routes/*.js need. Each routes module destructures
// the keys it cares about — adding a new dep here is non-breaking.
const deps = {
  // limiters
  authLimiter, submitLimiter, commentLimiter, voteLimiter,
  // middleware
  upload, csrfCheckParsed, requireVerified,
  requireAdmin: helpers.requireAdmin,
  // session helpers
  flash: helpers.flash,
  finishLoggedInRedirect: helpers.finishLoggedInRedirect,
  establishLoginSession: helpers.establishLoginSession,
  // voting
  buildVoteMap: helpers.buildVoteMap,
  rankManuscripts: helpers.rankManuscripts,
  applyVote: helpers.applyVote,
  // manuscripts
  fetchManuscript: helpers.fetchManuscript,
  fetchEditableManuscript: helpers.fetchEditableManuscript,
  makeSyntheticDoi: helpers.makeSyntheticDoi,
  parseManuscriptValues: helpers.parseManuscriptValues,
  validateManuscriptValues: helpers.validateManuscriptValues,
  // PDF + URL
  extractPdfText: helpers.extractPdfText,
  verifyUploadedPdf: helpers.verifyUploadedPdf,
  uploadedFileFsPath: helpers.uploadedFileFsPath,
  isSafeExternalUrl: helpers.isSafeExternalUrl,
  // search
  escapeFtsQuery: helpers.escapeFtsQuery,
  firstQueryString: helpers.firstQueryString,
  parseSearchFilters: helpers.parseSearchFilters,
  // ORCID
  orcidError: helpers.orcidError,
  normalizeOrcid: helpers.normalizeOrcid,
  // role checks
  isAdmin: helpers.isAdmin,
  // versioning + webhooks + audit (best-effort optional)
  snapshotManuscriptVersion: versionsLib.snapshotManuscriptVersion,
  currentManuscriptVersionNumber: versionsLib.currentManuscriptVersionNumber,
  listVersions: versionsLib.listVersions,
  unifiedDiff: versionsLib.unifiedDiff,
  safeEmit,
  manuscriptWebhookPayload,
  createNotification,
  auditLog,
  isProd: IS_PROD,
  // misc
  zenodo,
};

require('./routes/home').register(app, deps);
require('./routes/manuscript').register(app, deps);
require('./routes/comments').register(app, deps);
require('./routes/votes').register(app, deps);
require('./routes/auth').register(app, deps);
require('./routes/profile').register(app, deps);
require('./routes/social').register(app, deps);
require('./routes/me_account').register(app, deps);
require('./routes/admin').register(app, deps);
require('./routes/static').register(app, deps);

// ─── routes: API mount (JSON only, /api/v1) ────────────────────────────────
// Bearer tokens replace cookie+CSRF on this surface. JSON body parsing is
// already in place above. Errors here render JSON, never HTML.
app.use('/api/v1', buildApiRouter({
  parseManuscriptValues: helpers.parseManuscriptValues,
  validateManuscriptValues: helpers.validateManuscriptValues,
  authLimiter,
  submitLimiter,
  commentLimiter,
  voteLimiter,
  escapeFtsQuery: helpers.escapeFtsQuery,
  orcidError: helpers.orcidError,
  normalizeOrcid: helpers.normalizeOrcid,
  snapshotManuscriptVersion: versionsLib.snapshotManuscriptVersion,
}));

// ─── 404 ────────────────────────────────────────────────────────────────────
app.use((req, res) => {
  res.status(404).render('error', { code: 404, msg: 'Page not found.' });
});

app.use((err, req, res, _next) => {
  console.error(err);
  res.status(500).render('error', { code: 500, msg: 'Something went wrong on our end.' });
});

if (require.main === module) {
  const server = app.listen(PORT, () => {
    console.log(`PreXiv listening on http://localhost:${PORT}`);
  });

  // ─── graceful shutdown ────────────────────────────────────────────────────
  let shuttingDown = false;
  function shutdown(signal) {
    if (shuttingDown) return;
    shuttingDown = true;
    console.log(`[shutdown] received ${signal} — closing server`);
    const forceExit = setTimeout(() => {
      console.log('[shutdown] timeout reached — forcing exit');
      process.exit(0);
    }, 10_000);
    forceExit.unref();
    server.close(() => {
      try { db.close(); } catch (_e) { /* best-effort */ }
      console.log('[shutdown] complete');
      process.exit(0);
    });
  }
  process.on('SIGTERM', () => shutdown('SIGTERM'));
  process.on('SIGINT',  () => shutdown('SIGINT'));
}

module.exports = app;
