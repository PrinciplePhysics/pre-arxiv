// Shared helpers used by multiple route modules. server.js builds these once
// and threads them into each route module's `register(app, deps)` call so the
// routes don't reach back into closure scope.
//
// Most helpers here are thin wrappers around `db` and `fs` that several
// route files need (manuscript fetch, vote application, FTS escaping,
// validation, citation, ranking, …). Per-feature helpers that are only
// used by one route module live in that module instead.

const fs = require('fs');
const path = require('path');
const crypto = require('crypto');
const { db, CATEGORIES, ROLES } = require('../db');
const { rankScore, ageHours } = require('./util');

const UPLOAD_DIR = process.env.UPLOAD_DIR || path.join(__dirname, '..', 'public', 'uploads');

// ─── flash + login session ──────────────────────────────────────────────────

/**
 * Stash a one-shot flash message on the session.
 * @param {import('express').Request} req
 * @param {'ok'|'error'} type
 * @param {string} msg
 */
function flash(req, type, msg) {
  req.session.flash = { type, msg };
}

/**
 * Regenerate the session and stamp it with userId + a fresh CSRF token.
 * @param {import('express').Request} req
 * @param {number} userId
 * @param {(err?: Error) => void} cb
 */
function establishLoginSession(req, userId, cb) {
  req.session.regenerate((/** @type {Error|undefined} */ err) => {
    if (err) return cb(err);
    req.session.userId = userId;
    req.session.csrfToken = crypto.randomBytes(24).toString('base64url');
    cb();
  });
}

/**
 * Establish a logged-in session and redirect, with optional flash success.
 * @param {import('express').Request} req
 * @param {import('express').Response} res
 * @param {number} userId
 * @param {string} target
 * @param {string|null} msg
 * @param {import('express').NextFunction} next
 */
function finishLoggedInRedirect(req, res, userId, target, msg, next) {
  establishLoginSession(req, userId, (err) => {
    if (err) return next(err);
    if (msg) flash(req, 'ok', msg);
    res.redirect(target);
  });
}

// ─── voting ─────────────────────────────────────────────────────────────────

/**
 * Look up the current user's votes for a list of (type, id) pairs.
 * @param {number|undefined|null} userId
 * @param {'manuscript'|'comment'} type
 * @param {number[]} ids
 * @returns {Record<number, number>} map id -> vote value (1 or -1)
 */
function buildVoteMap(userId, type, ids) {
  /** @type {Record<number, number>} */
  const map = {};
  if (!userId || !ids.length) return map;
  const placeholders = ids.map(() => '?').join(',');
  const rows = /** @type {{target_id:number, value:number}[]} */ (
    db.prepare(
      `SELECT target_id, value FROM votes WHERE user_id = ? AND target_type = ? AND target_id IN (${placeholders})`
    ).all(userId, type, ...ids)
  );
  for (const r of rows) map[r.target_id] = r.value;
  return map;
}

/**
 * Sort a list of manuscript rows by HN-style ranked score (newer + higher score floats up).
 * @template {{score:number, created_at:string}} R
 * @param {R[]} rows
 * @returns {(R & {rankValue:number})[]}
 */
function rankManuscripts(rows) {
  return rows
    .map(r => ({ ...r, rankValue: rankScore(r.score, ageHours(r.created_at)) }))
    .sort((a, b) => b.rankValue - a.rankValue);
}

/**
 * Apply a vote toggle to a manuscript or comment. Returns null if the target
 * does not exist; { withdrawn: true } if voting on a withdrawn manuscript;
 * otherwise { score } where score is the new total after the toggle.
 * @param {number} userId
 * @param {'manuscript'|'comment'} type
 * @param {number} targetId
 * @param {1|-1} value
 * @returns {{score:number}|{withdrawn:true}|null}
 */
function applyVote(userId, type, targetId, value) {
  const table = type === 'manuscript' ? 'manuscripts' : 'comments';
  const target = type === 'manuscript'
    ? /** @type {{id:number, author_id:number, score:number, withdrawn:number}|undefined} */ (
        db.prepare('SELECT id, submitter_id AS author_id, score, withdrawn FROM manuscripts WHERE id = ?').get(targetId)
      )
    : /** @type {{id:number, author_id:number, score:number}|undefined} */ (
        db.prepare('SELECT id, author_id, score FROM comments WHERE id = ?').get(targetId)
      );
  if (!target) return null;
  if (type === 'manuscript' && /** @type {any} */ (target).withdrawn) return { withdrawn: true };
  const existing = /** @type {{value:number}|undefined} */ (
    db.prepare('SELECT value FROM votes WHERE user_id = ? AND target_type = ? AND target_id = ?')
      .get(userId, type, targetId)
  );
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
  if (target.author_id !== userId) {
    db.prepare('UPDATE users SET karma = karma + ? WHERE id = ?').run(delta, target.author_id);
  }
  return { score: target.score + delta };
}

// ─── manuscript fetch + validation ──────────────────────────────────────────

/**
 * Fetch a manuscript by either its prexiv id slug or numeric id.
 * @param {string|number} idOrSlug
 * @returns {object|undefined}
 */
function fetchManuscript(idOrSlug) {
  return db.prepare(`
    SELECT m.*, u.username AS submitter_username, u.display_name AS submitter_display
    FROM manuscripts m JOIN users u ON u.id = m.submitter_id
    WHERE m.arxiv_like_id = ? OR m.id = ?
  `).get(idOrSlug, idOrSlug);
}

/**
 * Fetch a manuscript and ensure the current user can edit it (owner or admin).
 * Renders the appropriate 403/404 error view and returns null if not editable.
 * @param {import('express').Request} req
 * @param {import('express').Response} res
 * @param {(user:any) => boolean} isAdminFn
 * @returns {object|null}
 */
function fetchEditableManuscript(req, res, isAdminFn) {
  const m = /** @type {{id:number, submitter_id:number}|undefined} */ (
    db.prepare(`
      SELECT m.*, u.username AS submitter_username, u.display_name AS submitter_display
      FROM manuscripts m JOIN users u ON u.id = m.submitter_id
      WHERE m.arxiv_like_id = ? OR m.id = ?
    `).get(req.params.id, req.params.id)
  );
  if (!m) { res.status(404).render('error', { code: 404, msg: 'Manuscript not found.' }); return null; }
  const allowed = (m.submitter_id === req.user.id) || isAdminFn(req.user);
  if (!allowed) { res.status(403).render('error', { code: 403, msg: 'You can only edit your own manuscripts.' }); return null; }
  return m;
}

/**
 * Synthetic DOI in Crossref's reserved 10.99999 prefix (test-only — never
 * resolves on doi.org). Used so manuscripts have a DOI-shaped citation
 * identifier without paying a registrar.
 * @param {string} arxivLikeId
 * @returns {string}
 */
function makeSyntheticDoi(arxivLikeId) {
  return '10.99999/' + (arxivLikeId || '').toUpperCase();
}

// ─── PDF + URL handling ─────────────────────────────────────────────────────

const MAX_PDF_TEXT = 500_000; // ~500 KB of text per manuscript

/**
 * Best-effort PDF -> plain text. Bounded so a malformed PDF can't OOM us;
 * failures are logged and return null (we just won't have full-text search
 * for that manuscript).
 * @param {string} filepath
 * @returns {Promise<string|null>}
 */
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
    console.warn('[pdf-parse] failed for ' + filepath + ': ' + (/** @type {Error} */ (e)).message);
    return null;
  }
}

/**
 * Verify a freshly-uploaded PDF on disk by reading its first 5 bytes. The
 * PDF spec requires `%PDF-` as the file signature; anything else is rejected.
 * Returns null on success, or a reason string on failure (caller should unlink + flash that reason).
 * @param {string} filePath
 * @returns {string|null}
 */
function verifyUploadedPdf(filePath) {
  let fd = -1;
  try {
    fd = fs.openSync(filePath, 'r');
    const buf = Buffer.alloc(5);
    const n = fs.readSync(fd, buf, 0, 5, 0);
    if (n < 5) return 'File is too short to be a PDF.';
    const sig = buf.toString('latin1');
    if (sig !== '%PDF-') return 'File does not have a valid PDF header.';
    return null;
  } catch (e) {
    return 'Could not read uploaded file: ' + ((/** @type {Error} */ (e)).message || 'unknown error');
  } finally {
    if (fd >= 0) { try { fs.closeSync(fd); } catch (_) {} }
  }
}

/**
 * Convert a public /uploads/foo.pdf URL into the absolute filesystem path
 * inside UPLOAD_DIR. Returns null if the input is not under /uploads/.
 * @param {string|null|undefined} publicPath
 * @returns {string|null}
 */
function uploadedFileFsPath(publicPath) {
  if (!publicPath || !String(publicPath).startsWith('/uploads/')) return null;
  return path.join(UPLOAD_DIR, path.basename(publicPath));
}

/**
 * Best-effort cleanup of any files multer wrote to disk for the request.
 * @param {import('express').Request & {file?:any, files?:any}} req
 */
function cleanupUploadedRequestFiles(req) {
  /** @type {{path?:string}[]} */
  const files = [];
  if (req.file) files.push(req.file);
  if (Array.isArray(req.files)) files.push(...req.files);
  else if (req.files && typeof req.files === 'object') {
    for (const group of Object.values(req.files)) {
      if (Array.isArray(group)) files.push(...group);
    }
  }
  for (const file of files) {
    if (file && file.path) fs.unlink(file.path, () => {});
  }
}

/**
 * Normalise an external URL string. If it parses as a valid http(s) URL, the
 * canonical form is returned; otherwise the trimmed input is returned for
 * error reporting.
 * @param {string|null|undefined} raw
 * @returns {string|null}
 */
function normalizeExternalUrl(raw) {
  if (!raw) return null;
  try {
    const u = new URL(String(raw).trim());
    if (u.protocol === 'http:' || u.protocol === 'https:') return u.toString();
  } catch (_e) { /* validation reports the useful message */ }
  return String(raw).trim();
}

/**
 * Validate an external URL. Returns a human-readable reason string if the URL
 * is malformed or uses a disallowed scheme; null if it's safe.
 * @param {string|null|undefined} url
 * @returns {string|null}
 */
function externalUrlError(url) {
  if (!url) return null;
  if (url.length > 500) return 'External URL is too long (≤ 500 characters).';
  try {
    const u = new URL(url);
    if (u.protocol !== 'http:' && u.protocol !== 'https:') {
      return 'External URL must use http:// or https://.';
    }
  } catch (_e) {
    return 'External URL must be a valid absolute URL.';
  }
  return null;
}

/**
 * Truthy iff externalUrlError returns null. Exposed to templates for
 * conditional display of the "open in new tab" link.
 * @param {string|null|undefined} url
 * @returns {boolean}
 */
function isSafeExternalUrl(url) {
  return !externalUrlError(url);
}

// ─── ORCID identifiers ──────────────────────────────────────────────────────
const ORCID_RE = /^\d{4}-\d{4}-\d{4}-\d{3}[\dX]$/;

/**
 * Validate an ORCID identifier. Format: \d{4}-\d{4}-\d{4}-\d{3}[\dX]
 * @param {string|null|undefined} raw
 * @returns {string|null} null if valid (or empty), else a human-readable error
 */
function orcidError(raw) {
  if (raw == null || raw === '') return null;
  const s = String(raw).trim().toUpperCase();
  if (!ORCID_RE.test(s)) {
    return 'ORCID must match the format XXXX-XXXX-XXXX-XXXX (last char digit or X).';
  }
  return null;
}

/**
 * Normalise an ORCID identifier to upper-case trimmed form, or null.
 * @param {string|null|undefined} raw
 * @returns {string|null}
 */
function normalizeOrcid(raw) {
  if (raw == null) return null;
  const s = String(raw).trim().toUpperCase();
  return s || null;
}

// ─── input parsing for /submit and /api/v1/manuscripts ──────────────────────

/**
 * Read a "checkbox-y" form/JSON value as a boolean. HTML form checkboxes
 * arrive as the literal string 'on' (or '1' / 'true' if hand-crafted); JSON
 * API clients send a real boolean. Treat 0/null/undefined/'' as false.
 * @param {unknown} v
 * @returns {boolean}
 */
function checkboxBool(v) {
  if (v === true) return true;
  if (v === false || v == null) return false;
  const s = String(v).trim().toLowerCase();
  return s === 'on' || s === '1' || s === 'true' || s === 'yes';
}

/**
 * @param {unknown} value
 * @returns {string}
 */
function firstQueryString(value) {
  const raw = Array.isArray(value) ? value[0] : value;
  return raw == null ? '' : String(raw);
}

/**
 * Coerce req.body fields into the canonical manuscript-submission shape.
 * @param {{body: any}} req
 * @returns {Record<string, any>}
 */
function parseManuscriptValues(req) {
  // Coerce to string, but allow real booleans / numbers to pass through
  // their string form. JSON callers may send `null` for an absent field.
  const s = (/** @type {unknown} */ v) => (v == null ? '' : String(v));
  const ct = s(req.body.conductor_type).trim();
  return {
    title: s(req.body.title).trim(),
    abstract: s(req.body.abstract).trim(),
    authors: s(req.body.authors).trim(),
    category: s(req.body.category).trim(),
    external_url: normalizeExternalUrl(s(req.body.external_url).trim()),
    conductor_type: (ct === 'ai-agent') ? 'ai-agent' : 'human-ai',
    conductor_ai_model: s(req.body.conductor_ai_model).trim(),
    conductor_human: s(req.body.conductor_human).trim(),
    conductor_role: s(req.body.conductor_role).trim(),
    conductor_notes: s(req.body.conductor_notes).trim() || null,
    agent_framework: s(req.body.agent_framework).trim() || null,
    conductor_ai_model_private: checkboxBool(req.body.conductor_ai_model_private),
    conductor_human_private:    checkboxBool(req.body.conductor_human_private),
    has_auditor: checkboxBool(req.body.has_auditor),
    auditor_name: s(req.body.auditor_name).trim(),
    auditor_affiliation: s(req.body.auditor_affiliation).trim(),
    auditor_role: s(req.body.auditor_role).trim(),
    auditor_statement: s(req.body.auditor_statement).trim(),
    auditor_orcid: normalizeOrcid(req.body.auditor_orcid),
    no_auditor_ack: checkboxBool(req.body.no_auditor_ack),
    ai_agent_ack:   checkboxBool(req.body.ai_agent_ack),
  };
}

/**
 * Validate the result of parseManuscriptValues. Returns an array of
 * human-readable error strings; an empty array means "valid".
 * @param {Record<string, any>} v
 * @param {{editing?: boolean}} [opts]
 * @returns {string[]}
 */
function validateManuscriptValues(v, opts = {}) {
  /** @type {string[]} */
  const errors = [];
  const isEdit = !!opts.editing;
  if (!v.title || v.title.length < 5)         errors.push('Title is required (≥ 5 characters).');
  if (v.title.length > 300)                   errors.push('Title is too long (≤ 300 characters).');
  if (!v.abstract || v.abstract.length < 50)  errors.push('Abstract is required (≥ 50 characters).');
  if (v.abstract.length > 5000)               errors.push('Abstract is too long (≤ 5000 characters).');
  if (!v.authors)                             errors.push('Authors line is required (e.g., "Jane Doe; Example Lab").');
  if (!CATEGORIES.find((/** @type {{id:string}} */ c) => c.id === v.category)) errors.push('Pick a valid category.');
  if (!v.conductor_ai_model)                  errors.push('Conductor: AI model is required.');
  const urlErr = externalUrlError(v.external_url);
  if (urlErr) errors.push(urlErr);

  if (v.conductor_type === 'human-ai') {
    if (!v.conductor_human)                   errors.push('Conductor: human conductor name is required.');
    if (!ROLES.includes(v.conductor_role))    errors.push('Conductor: pick a valid role for the human conductor.');
  } else {
    if (!isEdit && !v.ai_agent_ack) {
      errors.push('You must acknowledge that this manuscript was produced by an AI agent acting autonomously, that no human conductor directed production, and that you remain responsible for lawful posting and accurate disclosure.');
    }
  }

  if (v.has_auditor) {
    if (!v.auditor_name)                      errors.push('Auditor name is required when an auditor is listed.');
    if (!ROLES.includes(v.auditor_role))      errors.push('Auditor: pick a valid role.');
    if (!v.auditor_statement || v.auditor_statement.length < 20)
      errors.push('Auditor statement is required (≥ 20 characters).');
    const oErr = orcidError(v.auditor_orcid);
    if (oErr) errors.push('Auditor: ' + oErr);
  } else if (!isEdit && v.conductor_type === 'human-ai' && !v.no_auditor_ack) {
    errors.push('You must acknowledge that no human auditor is signing a correctness statement and that this manuscript is unaudited.');
  }

  return errors;
}

// ─── search ─────────────────────────────────────────────────────────────────

/**
 * Quote each whitespace-separated term so SQLite FTS5 treats it as a literal
 * phrase. Defends against accidental operator injection (e.g. a trailing `*`
 * or unbalanced parenthesis from user input).
 * @param {string} q
 * @returns {string}
 */
function escapeFtsQuery(q) {
  return q.split(/\s+/).filter(Boolean)
    .map(t => '"' + t.replace(/"/g, '""') + '"')
    .join(' ');
}

/**
 * Parse filter query params shared by /search and /api/v1/search.
 * @param {Record<string, any>} qq
 * @returns {{
 *   category:string, mode:string, dateFrom:string, dateTo:string, scoreMin:number|null,
 *   sql:string, params:any[]
 * }} `sql` is the additional WHERE clause fragment (no leading AND/WHERE).
 */
function parseSearchFilters(qq) {
  const cat = (typeof qq.category === 'string' && qq.category) ? qq.category : '';
  const validCat = CATEGORIES.find((/** @type {{id:string}} */ c) => c.id === cat) ? cat : '';
  let mode = (typeof qq.mode === 'string' ? qq.mode : '').toLowerCase();
  if (!['audited', 'unaudited', 'agent', 'human-conducted', 'any', ''].includes(mode)) mode = '';
  const dateFrom = (typeof qq.date_from === 'string' && /^\d{4}-\d{2}-\d{2}$/.test(qq.date_from)) ? qq.date_from : '';
  const dateTo   = (typeof qq.date_to   === 'string' && /^\d{4}-\d{2}-\d{2}$/.test(qq.date_to))   ? qq.date_to   : '';
  const scoreMinRaw = parseInt(qq.score_min, 10);
  const scoreMin = Number.isFinite(scoreMinRaw) ? scoreMinRaw : null;

  /** @type {string[]} */
  const clauses = [];
  /** @type {any[]} */
  const params = [];
  if (validCat) { clauses.push('m.category = ?'); params.push(validCat); }
  if (mode === 'audited')         clauses.push('m.has_auditor = 1');
  if (mode === 'unaudited')       clauses.push('m.has_auditor = 0');
  if (mode === 'agent')           clauses.push("m.conductor_type = 'ai-agent'");
  if (mode === 'human-conducted') clauses.push("m.conductor_type = 'human-ai'");
  if (dateFrom) { clauses.push('m.created_at >= ?'); params.push(dateFrom + ' 00:00:00'); }
  if (dateTo)   { clauses.push('m.created_at <= ?'); params.push(dateTo   + ' 23:59:59'); }
  if (scoreMin != null) { clauses.push('m.score >= ?'); params.push(scoreMin); }

  return {
    category: validCat, mode, dateFrom, dateTo, scoreMin,
    sql: clauses.length ? clauses.join(' AND ') : '',
    params,
  };
}

// ─── auth-related guards ────────────────────────────────────────────────────

/**
 * Build a requireVerified middleware that bounces unverified users to
 * /verify-pending. The factory shape is so we can inject `flash` cleanly.
 * @returns {import('express').RequestHandler}
 */
function buildRequireVerified() {
  return function requireVerified(req, res, next) {
    if (!req.user) return res.redirect('/login?next=' + encodeURIComponent(req.originalUrl));
    const u = /** @type {{email_verified:number}|undefined} */ (
      db.prepare('SELECT email_verified FROM users WHERE id = ?').get(req.user.id)
    );
    if (!u || !u.email_verified) {
      flash(req, 'error', 'Please verify your email address before submitting.');
      return res.redirect('/verify-pending');
    }
    next();
  };
}

/**
 * Is the user an admin? Pure read — no mutation.
 * @param {{id:number}|null|undefined} user
 * @returns {boolean}
 */
function isAdmin(user) {
  if (!user) return false;
  const r = /** @type {{is_admin:number}|undefined} */ (
    db.prepare('SELECT is_admin FROM users WHERE id = ?').get(user.id)
  );
  return !!(r && r.is_admin);
}

/**
 * Express middleware: require an admin user, otherwise redirect to login or 403.
 * @param {import('express').Request} req
 * @param {import('express').Response} res
 * @param {import('express').NextFunction} next
 */
function requireAdmin(req, res, next) {
  if (!req.user) return res.redirect('/login?next=' + encodeURIComponent(req.originalUrl));
  if (!isAdmin(req.user)) return res.status(403).render('error', { code: 403, msg: 'Admin only.' });
  next();
}

module.exports = {
  // session helpers
  flash,
  establishLoginSession,
  finishLoggedInRedirect,
  // voting
  buildVoteMap,
  rankManuscripts,
  applyVote,
  // manuscripts
  fetchManuscript,
  fetchEditableManuscript,
  makeSyntheticDoi,
  // PDF + URL
  extractPdfText,
  verifyUploadedPdf,
  uploadedFileFsPath,
  cleanupUploadedRequestFiles,
  normalizeExternalUrl,
  externalUrlError,
  isSafeExternalUrl,
  orcidError,
  normalizeOrcid,
  // input parsing
  checkboxBool,
  firstQueryString,
  parseManuscriptValues,
  validateManuscriptValues,
  // search
  escapeFtsQuery,
  parseSearchFilters,
  // auth guards
  buildRequireVerified,
  isAdmin,
  requireAdmin,
};
