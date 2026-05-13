// PreXiv JSON API mounted at /api/v1.
//
// Every operation a logged-in human can do via the website has a JSON twin
// here. Read endpoints are public; write endpoints require a Bearer token.
// Validation reuses parseManuscriptValues / validateManuscriptValues from
// the caller (we receive them via the factory's `deps` argument).
//
// Errors follow the shape `{ error: "<msg>", details?: [...] }`.

const express = require('express');
const crypto = require('crypto');
const { db, CATEGORIES, ROLES } = require('../db');
const { hashPassword, verifyPassword, validateUsername, isPasswordPwned } = require('./auth');
const { generateToken, hashToken, extractBearer } = require('./api-auth');
const { makeArxivLikeId, paginate, rankScore, ageHours } = require('./util');
const { buildOpenApi } = require('./openapi');
const { buildManifest } = require('./manifest');
const { verifyTotp } = require('./totp');
const zenodo = require('./zenodo');
const fs = require('fs');
const path = require('path');

// ─── API token expiry / rotation helpers ────────────────────────────────────
// Default 90 days from creation. `expires_in_days` may be passed (1–365), and
// 0 means never expires. Returns an ISO-ish "YYYY-MM-DD HH:MM:SS" string for
// SQLite, or null for never.
function expiresAtFromDays(days) {
  if (days === 0) return null;
  return new Date(Date.now() + days * 86400 * 1000).toISOString().slice(0, 19).replace('T', ' ');
}

function parseExpiresInDays(raw, fallback = 90) {
  if (raw === undefined || raw === null || String(raw).trim() === '') return { ok: true, value: fallback };
  const n = parseInt(raw, 10);
  if (!Number.isFinite(n) || n < 0 || n > 365) {
    return { ok: false, error: 'expires_in_days must be 0 (never) or 1–365.' };
  }
  return { ok: true, value: n };
}

// ─── Registration challenge (proof-of-work, in-memory) ──────────────────────
// Replaces the web's math CAPTCHA on the API path. Not durable — a server
// restart drops outstanding challenges, which is fine: clients always pull a
// fresh one before registering.
const REGISTER_CHALLENGE_TTL_MS = 5 * 60 * 1000;
const REGISTER_DIFFICULTY_BITS = 16;
const registerChallenges = new Map(); // challenge -> { expires_at, difficulty }

function gcRegisterChallenges() {
  const now = Date.now();
  for (const [k, v] of registerChallenges) {
    if (v.expires_at <= now) registerChallenges.delete(k);
  }
}

// Verify SHA-256(challenge ":" nonce) starts with `difficulty` zero bits.
function verifyPow(challenge, nonce, difficulty) {
  if (typeof challenge !== 'string' || typeof nonce !== 'string') return false;
  if (!Number.isInteger(difficulty) || difficulty <= 0 || difficulty > 32) return false;
  const h = crypto.createHash('sha256').update(challenge + ':' + nonce).digest();
  let bitsToCheck = difficulty;
  let i = 0;
  while (bitsToCheck >= 8) {
    if (h[i] !== 0) return false;
    bitsToCheck -= 8;
    i += 1;
  }
  if (bitsToCheck > 0) {
    const mask = 0xff << (8 - bitsToCheck) & 0xff;
    if ((h[i] & mask) !== 0) return false;
  }
  return true;
}

function makeSyntheticDoi(arxivLikeId) {
  return '10.99999/' + (arxivLikeId || '').toUpperCase();
}

function isAdmin(user) {
  if (!user) return false;
  const r = db.prepare('SELECT is_admin FROM users WHERE id = ?').get(user.id);
  return !!(r && r.is_admin);
}

function publicUser(u) {
  if (!u) return null;
  const out = {
    id: u.id,
    username: u.username,
    display_name: u.display_name || null,
    affiliation: u.affiliation || null,
    bio: u.bio || null,
    karma: u.karma || 0,
    is_admin: !!u.is_admin,
    email: u.email || null,
    email_verified: !!u.email_verified,
    created_at: u.created_at || null,
  };
  // Optional fields that may or may not exist depending on parallel migrations.
  if ('orcid' in u) out.orcid = u.orcid || null;
  return out;
}

function fetchUserFull(id) {
  // SELECT * so parallel-added columns (e.g. orcid) flow through publicUser.
  return db.prepare(`SELECT * FROM users WHERE id = ?`).get(id);
}

function fetchManuscript(idOrSlug) {
  return db.prepare(`
    SELECT m.*, u.username AS submitter_username, u.display_name AS submitter_display
    FROM manuscripts m JOIN users u ON u.id = m.submitter_id
    WHERE m.arxiv_like_id = ? OR m.id = ?
  `).get(idOrSlug, idOrSlug);
}

// Redact private conductor fields (`conductor_human`, `conductor_ai_model`)
// from the API response when the viewer is neither the submitter nor an
// admin. The `_public` flags themselves remain so clients can render an
// "(undisclosed)" label. Submitter/admin always see the real values.
function redactManuscript(m, viewer, viewerIsAdmin) {
  if (!m) return m;
  const isOwner = !!(viewer && m.submitter_id === viewer.id);
  if (isOwner || viewerIsAdmin) return m;
  const out = { ...m };
  if (m.conductor_ai_model_public === 0) out.conductor_ai_model = null;
  if (m.conductor_human_public    === 0) out.conductor_human    = null;
  return out;
}

function err(res, code, message, details) {
  const body = { error: message };
  if (details) body.details = details;
  return res.status(code).json(body);
}

function firstQueryString(value) {
  const raw = Array.isArray(value) ? value[0] : value;
  return raw == null ? '' : String(raw);
}

function uploadedFileFsPath(publicPath) {
  if (!publicPath || !String(publicPath).startsWith('/uploads/')) return null;
  const uploadDir = process.env.UPLOAD_DIR || path.join(__dirname, '..', 'public', 'uploads');
  return path.join(uploadDir, path.basename(publicPath));
}

// API-level requireAuth — always JSON 401, never an HTML redirect. We only
// accept Bearer (not session) because the API surface is for non-browser
// clients; a logged-in browser session that calls /api/v1/me without a
// Bearer header should see a clean 401, not get treated as authenticated.
function requireApiAuth(req, res, next) {
  if (!extractBearer(req)) return err(res, 401, 'Bearer token required.');
  if (!req.user) return err(res, 401, 'Invalid or expired Bearer token.');
  next();
}
function requireApiAdmin(req, res, next) {
  if (!extractBearer(req)) return err(res, 401, 'Bearer token required.');
  if (!req.user) return err(res, 401, 'Invalid or expired Bearer token.');
  if (!isAdmin(req.user)) return err(res, 403, 'Admin only.');
  next();
}
function requireApiVerified(req, res, next) {
  const u = req.user && db.prepare('SELECT email_verified FROM users WHERE id = ?').get(req.user.id);
  if (!u || !u.email_verified) return err(res, 403, 'Email verification is required to submit manuscripts.');
  next();
}

function buildVoteForUser(userId, type, id) {
  if (!userId) return 0;
  const row = db.prepare('SELECT value FROM votes WHERE user_id = ? AND target_type = ? AND target_id = ?')
    .get(userId, type, id);
  return row ? row.value : 0;
}

// Helper used by the flag handler — re-fetches the emit hook off the
// express app on every call so we never hold a stale reference.
function maybeEmitFlag(req, type, targetId, reason) {
  try {
    const emit = req.app && req.app.locals && req.app.locals.emitWebhook;
    if (typeof emit === 'function') {
      emit('flag.created', {
        target_type: type, target_id: targetId,
        reporter_id: req.user ? req.user.id : null,
        reporter_username: req.user ? (req.user.username || null) : null,
        reason,
        ts: new Date().toISOString(),
      });
    }
  } catch (_e) { /* ignore */ }
}

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
  const authorCol = type === 'manuscript' ? 'submitter_id' : 'author_id';
  const author = db.prepare(`SELECT ${authorCol} AS aid FROM ${table} WHERE id = ?`).get(targetId);
  if (author && author.aid !== userId) {
    db.prepare('UPDATE users SET karma = karma + ? WHERE id = ?').run(delta, author.aid);
  }
  return db.prepare(`SELECT score FROM ${table} WHERE id = ?`).get(targetId).score;
}

// `deps` carries: { parseManuscriptValues, validateManuscriptValues,
//                   authLimiter, submitLimiter, commentLimiter, voteLimiter,
//                   escapeFtsQuery }
function buildApiRouter(deps) {
  const router = express.Router();
  const { parseManuscriptValues, validateManuscriptValues,
          authLimiter, submitLimiter, commentLimiter, voteLimiter,
          escapeFtsQuery } = deps;

  // Make sure JSON errors bubble up as JSON, not as the HTML error page.
  router.use((req, res, next) => {
    res.setHeader('Cache-Control', 'no-store');
    next();
  });

  // ─── auth + identity ─────────────────────────────────────────────────────

  // Issue a fresh registration proof-of-work challenge. The challenge is a
  // random 16-byte hex string; the client must produce a `nonce` such that
  // SHA-256(challenge + ':' + nonce) has `difficulty` (default 16) leading
  // zero bits. ~65k SHA-256 hashes for the default difficulty — instant for
  // a real client, prohibitive for a flood.
  router.get('/register/challenge', (_req, res) => {
    gcRegisterChallenges();
    const challenge = crypto.randomBytes(16).toString('hex');
    registerChallenges.set(challenge, {
      expires_at: Date.now() + REGISTER_CHALLENGE_TTL_MS,
      difficulty: REGISTER_DIFFICULTY_BITS,
    });
    res.json({
      challenge,
      difficulty: REGISTER_DIFFICULTY_BITS,
      ttl_seconds: Math.floor(REGISTER_CHALLENGE_TTL_MS / 1000),
      hint: 'Submit POST /register with body fields `challenge` and `nonce` such that SHA-256(challenge+":"+nonce) starts with `difficulty` zero bits.',
    });
  });

  router.post('/register', authLimiter, async (req, res) => {
    const username     = (req.body.username || '').trim();
    const email        = (req.body.email || '').trim().toLowerCase();
    const password     = req.body.password || '';
    const display_name = (req.body.display_name || '').trim() || null;
    const affiliation  = (req.body.affiliation || '').trim() || null;
    const challenge    = (req.body.challenge || '').trim();
    const nonce        = (req.body.nonce ? String(req.body.nonce) : '').trim();

    const errors = [];
    const uErr = validateUsername(username);
    if (uErr) errors.push(uErr);
    if (!email || !/^[^@\s]+@[^@\s]+\.[^@\s]+$/.test(email)) errors.push('A valid email is required.');
    if (!password || password.length < 8) errors.push('Password must be ≥ 8 characters.');

    // Proof-of-work CAPTCHA-equivalent — replaces the web math CAPTCHA on the
    // API path. Pull from /register/challenge first.
    gcRegisterChallenges();
    if (!challenge || !nonce) {
      errors.push('challenge and nonce are required. Fetch one from GET /api/v1/register/challenge.');
    } else {
      const ch = registerChallenges.get(challenge);
      if (!ch) {
        errors.push('Unknown or expired challenge. Fetch a new one from GET /api/v1/register/challenge.');
      } else if (!verifyPow(challenge, nonce, ch.difficulty)) {
        errors.push('Invalid proof-of-work nonce.');
      }
    }
    if (!errors.length) {
      const dup = db.prepare('SELECT 1 FROM users WHERE username = ? OR email = ?').get(username, email);
      if (dup) errors.push('That username or email is already in use.');
    }
    // HIBP k-anonymity check. Network errors are tolerated (warn-and-allow).
    if (!errors.length && password) {
      try {
        if (await isPasswordPwned(password)) {
          errors.push('This password has appeared in known data breaches. Please pick a different one.');
        }
      } catch (_e) { /* defensive — isPasswordPwned already swallows network errors */ }
    }
    if (errors.length) return err(res, 422, 'Validation failed.', errors);

    // Consume the challenge so it can't be reused.
    registerChallenges.delete(challenge);

    // API path skips both the math CAPTCHA (replaced by POW above) and the
    // email-verify gate.
    const r = db.prepare(`
      INSERT INTO users (username, email, password_hash, display_name, affiliation, email_verified)
      VALUES (?, ?, ?, ?, ?, 1)
    `).run(username, email, hashPassword(password), display_name, affiliation);

    const plain = generateToken();
    const expiresAt = expiresAtFromDays(90);
    const tokRow = db.prepare(
      'INSERT INTO api_tokens (user_id, token_hash, name, expires_at) VALUES (?, ?, ?, ?)'
    ).run(r.lastInsertRowid, hashToken(plain), 'register', expiresAt);

    const u = fetchUserFull(r.lastInsertRowid);
    return res.json({
      user: publicUser(u),
      token: plain,
      verify_url: null,
      token_id: tokRow.lastInsertRowid,
      expires_at: expiresAt,
    });
  });

  router.post('/login', authLimiter, (req, res) => {
    const id  = (req.body.username_or_email || req.body.username || '').trim();
    const pw  = req.body.password || '';
    if (!id || !pw) return err(res, 400, 'username_or_email and password are required.');
    const u = db.prepare('SELECT id, password_hash, totp_enabled FROM users WHERE username = ? OR email = ?').get(id, id);
    if (!u || !verifyPassword(pw, u.password_hash)) return err(res, 401, 'Invalid username or password.');

    // 2FA: if enabled, the simple /login path must fail with a 2FA-required
    // hint. The caller then re-tries on /login/2fa with the code.
    if (u.totp_enabled) {
      return res.status(401).json({
        error: '2FA required',
        need_2fa: true,
        user_id: u.id,
        next: 'POST /api/v1/login/2fa with { user_id, password, code }',
      });
    }

    const plain = generateToken();
    const expiresAt = expiresAtFromDays(90);
    db.prepare('INSERT INTO api_tokens (user_id, token_hash, name, expires_at) VALUES (?, ?, ?, ?)')
      .run(u.id, hashToken(plain), 'login', expiresAt);
    return res.json({ user: publicUser(fetchUserFull(u.id)), token: plain, expires_at: expiresAt });
  });

  // 2FA second-factor login. Accepts either user_id+password+code or
  // username_or_email+password+code. Returns a token on success.
  router.post('/login/2fa', authLimiter, (req, res) => {
    const code = String(req.body.code || '').trim();
    const pw   = req.body.password || '';
    let u;
    if (req.body.user_id != null) {
      const id = parseInt(req.body.user_id, 10);
      if (!Number.isInteger(id) || id <= 0) return err(res, 400, 'Bad user_id.');
      u = db.prepare('SELECT id, password_hash, totp_enabled, totp_secret FROM users WHERE id = ?').get(id);
    } else {
      const ident = (req.body.username_or_email || req.body.username || '').trim();
      u = db.prepare('SELECT id, password_hash, totp_enabled, totp_secret FROM users WHERE username = ? OR email = ?').get(ident, ident);
    }
    if (!u) return err(res, 401, 'Invalid username or password.');
    if (!pw || !verifyPassword(pw, u.password_hash)) return err(res, 401, 'Invalid username or password.');
    if (!u.totp_enabled || !u.totp_secret) {
      // Caller asked for a 2FA login but this user has no 2FA — fall through
      // gracefully and issue a token, since password alone is sufficient.
      const plain = generateToken();
      const expiresAt = expiresAtFromDays(90);
      db.prepare('INSERT INTO api_tokens (user_id, token_hash, name, expires_at) VALUES (?, ?, ?, ?)')
        .run(u.id, hashToken(plain), 'login', expiresAt);
      return res.json({ user: publicUser(fetchUserFull(u.id)), token: plain, expires_at: expiresAt });
    }
    if (!verifyTotp(u.totp_secret, code, 1)) return err(res, 401, 'Invalid 2FA code.');
    const plain = generateToken();
    const expiresAt = expiresAtFromDays(90);
    db.prepare('INSERT INTO api_tokens (user_id, token_hash, name, expires_at) VALUES (?, ?, ?, ?)')
      .run(u.id, hashToken(plain), 'login', expiresAt);
    return res.json({ user: publicUser(fetchUserFull(u.id)), token: plain, expires_at: expiresAt });
  });

  router.post('/logout', requireApiAuth, (req, res) => {
    if (req.user._api_token_id) {
      db.prepare('DELETE FROM api_tokens WHERE id = ? AND user_id = ?').run(req.user._api_token_id, req.user.id);
    }
    res.json({ ok: true });
  });

  router.get('/me', requireApiAuth, (req, res) => {
    res.json(publicUser(fetchUserFull(req.user.id)));
  });

  // PATCH /me — update display_name, affiliation, bio, orcid. Any subset
  // is allowed. ORCID is validated as \d{4}-\d{4}-\d{4}-\d{3}[\dX] but
  // NOT verified — we trust the user's claim. Real OAuth would replace
  // this in a future iteration.
  router.patch('/me', requireApiAuth, (req, res) => {
    const body = req.body || {};
    const errors = [];

    // Build the update dynamically: only touch keys the caller provided.
    const updates = {};
    if ('display_name' in body) {
      const v = body.display_name == null ? '' : String(body.display_name).trim();
      if (v.length > 200) errors.push('display_name is too long (≤ 200).');
      updates.display_name = v || null;
    }
    if ('affiliation' in body) {
      const v = body.affiliation == null ? '' : String(body.affiliation).trim();
      if (v.length > 200) errors.push('affiliation is too long (≤ 200).');
      updates.affiliation = v || null;
    }
    if ('bio' in body) {
      const v = body.bio == null ? '' : String(body.bio).trim();
      if (v.length > 2000) errors.push('bio is too long (≤ 2000).');
      updates.bio = v || null;
    }
    if ('orcid' in body) {
      const raw = body.orcid == null ? '' : String(body.orcid).trim();
      if (raw) {
        const oErr = (deps.orcidError ? deps.orcidError(raw) : null);
        if (oErr) errors.push(oErr);
      }
      updates.orcid = raw ? (deps.normalizeOrcid ? deps.normalizeOrcid(raw) : raw.toUpperCase()) : null;
    }
    if (errors.length) return err(res, 422, 'Validation failed.', errors);

    const cols = Object.keys(updates);
    if (!cols.length) return res.json(publicUser(fetchUserFull(req.user.id)));
    const setSql = cols.map(c => c + ' = ?').join(', ');
    const args = cols.map(c => updates[c]);
    args.push(req.user.id);
    db.prepare(`UPDATE users SET ${setSql} WHERE id = ?`).run(...args);
    res.json(publicUser(fetchUserFull(req.user.id)));
  });

  router.get('/me/tokens', requireApiAuth, (req, res) => {
    const rows = db.prepare(
      'SELECT id, name, last_used_at, created_at, expires_at FROM api_tokens WHERE user_id = ? ORDER BY created_at DESC'
    ).all(req.user.id);
    res.json(rows);
  });

  router.post('/me/tokens', requireApiAuth, (req, res) => {
    const name = (req.body && typeof req.body.name === 'string') ? req.body.name.trim().slice(0, 200) : null;
    const expChoice = parseExpiresInDays(req.body && req.body.expires_in_days);
    if (!expChoice.ok) return err(res, 422, expChoice.error);
    const expiresAt = expiresAtFromDays(expChoice.value);
    const plain = generateToken();
    const r = db.prepare('INSERT INTO api_tokens (user_id, token_hash, name, expires_at) VALUES (?, ?, ?, ?)')
      .run(req.user.id, hashToken(plain), name || null, expiresAt);
    const row = db.prepare('SELECT id, name, created_at, expires_at FROM api_tokens WHERE id = ?').get(r.lastInsertRowid);
    res.json({ id: row.id, name: row.name, token: plain, created_at: row.created_at, expires_at: row.expires_at });
  });

  // Rotate: generate a new token under the same name (and expiry policy),
  // then delete the old one. Returns the new id + plaintext.
  router.post('/me/tokens/:id/rotate', requireApiAuth, (req, res) => {
    const id = parseInt(req.params.id, 10);
    if (!id) return err(res, 400, 'Bad token id.');
    const old = db.prepare('SELECT id, user_id, name FROM api_tokens WHERE id = ?').get(id);
    if (!old || old.user_id !== req.user.id) return err(res, 404, 'Token not found.');
    // Allow override of the new token's expiry; default 90 days from now.
    const expChoice = parseExpiresInDays(req.body && req.body.expires_in_days);
    if (!expChoice.ok) return err(res, 422, expChoice.error);
    const expiresAt = expiresAtFromDays(expChoice.value);
    const plain = generateToken();
    const r = db.prepare('INSERT INTO api_tokens (user_id, token_hash, name, expires_at) VALUES (?, ?, ?, ?)')
      .run(req.user.id, hashToken(plain), old.name, expiresAt);
    db.prepare('DELETE FROM api_tokens WHERE id = ?').run(id);
    const row = db.prepare('SELECT id, name, created_at, expires_at FROM api_tokens WHERE id = ?').get(r.lastInsertRowid);
    res.json({ id: row.id, name: row.name, token: plain, created_at: row.created_at, expires_at: row.expires_at, rotated_from: id });
  });

  router.delete('/me/tokens/:id', requireApiAuth, (req, res) => {
    const id = parseInt(req.params.id, 10);
    if (!id) return err(res, 400, 'Bad token id.');
    const t = db.prepare('SELECT id, user_id FROM api_tokens WHERE id = ?').get(id);
    if (!t || t.user_id !== req.user.id) return err(res, 404, 'Token not found.');
    db.prepare('DELETE FROM api_tokens WHERE id = ?').run(id);
    try { require('./audit').auditLog(req, 'token_revoke', 'api_token', id, null); } catch (_e) {}
    res.json({ ok: true });
  });

  // ─── GDPR data export / account deletion (API twin) ──────────────────────
  // The web routes in server.js own the actual serializer + anonymizer; we
  // pull them off `req.app.locals` so the logic stays in one place.
  router.get('/me/export', requireApiAuth, (req, res) => {
    const buildUserExport = req.app && req.app.locals && req.app.locals.buildUserExport;
    if (typeof buildUserExport !== 'function') return err(res, 500, 'Export helper unavailable.');
    const data = buildUserExport(req.user.id);
    if (!data) return err(res, 404, 'User not found.');
    try { require('./audit').auditLog(req, 'data_export', 'user', req.user.id, null); } catch (_e) {}
    res.json(data);
  });

  router.post('/me/delete-account', requireApiAuth, (req, res) => {
    const anonymizeUser = req.app && req.app.locals && req.app.locals.anonymizeUser;
    if (typeof anonymizeUser !== 'function') return err(res, 500, 'Delete helper unavailable.');
    const password = (req.body && typeof req.body.password === 'string') ? req.body.password : '';
    if (!password) return err(res, 422, 'password is required.');
    let result;
    try { result = anonymizeUser(req.user.id, password, req.ip); }
    catch (e) {
      console.warn('[api delete-account] failed:', e.message || e);
      return err(res, 500, 'Internal server error.');
    }
    if (!result.ok) return err(res, result.error === 'Password does not match.' ? 401 : 400, result.error);
    // The token used to authenticate this request was just revoked along with
    // every other token (anonymizeUser runs DELETE FROM api_tokens). Future
    // calls with the old token will 401.
    res.json({ ok: true });
  });

  // ─── webhooks (per-user agent subscriptions) ─────────────────────────────
  // Lazy-require so the API router still loads if lib/webhooks.js is missing
  // for any reason (e.g. the parallel agent is mid-deploy). All handlers
  // re-check on each request.
  const webhooksLib = (() => {
    try { return require('./webhooks'); }
    catch (_e) { return null; }
  })();

  router.get('/me/webhooks', requireApiAuth, (req, res) => {
    if (!webhooksLib) return err(res, 503, 'Webhooks subsystem unavailable.');
    const rows = db.prepare(`
      SELECT id, url, events, active, description, failure_count,
             last_attempt_at, last_status, created_at
      FROM webhooks WHERE user_id = ?
      ORDER BY created_at DESC
    `).all(req.user.id);
    // Parse the events JSON field for clients.
    const items = rows.map(r => ({
      ...r,
      events: (() => { try { return JSON.parse(r.events); } catch (_e) { return []; } })(),
    }));
    res.json({ items });
  });

  router.post('/me/webhooks', requireApiAuth, async (req, res) => {
    if (!webhooksLib) return err(res, 503, 'Webhooks subsystem unavailable.');
    const body = req.body || {};
    const url = (typeof body.url === 'string' ? body.url : '').trim().slice(0, 500);
    const description = (typeof body.description === 'string' ? body.description : '').trim().slice(0, 200) || null;
    let events = body.events;
    if (!Array.isArray(events)) events = [];
    events = events
      .map(s => String(s || '').trim())
      .filter(Boolean)
      .filter(s => webhooksLib.SUPPORTED_EVENTS.includes(s));

    const errors = [];
    const urlErr = await webhooksLib.validateWebhookUrl(url, { requireHttps: process.env.NODE_ENV === 'production' });
    if (urlErr) errors.push(urlErr);
    if (!events.length) errors.push('events must include at least one supported event name. Supported: ' + webhooksLib.SUPPORTED_EVENTS.join(', '));
    if (errors.length) return err(res, 422, 'Validation failed.', errors);

    const secret = webhooksLib.randomSecret();
    const r = db.prepare(`
      INSERT INTO webhooks (user_id, url, secret, events, active, description)
      VALUES (?, ?, ?, ?, 1, ?)
    `).run(req.user.id, url, secret, JSON.stringify(events), description);
    try { require('./audit').auditLog(req, 'webhook_create', 'webhook', r.lastInsertRowid, JSON.stringify({ url, events })); } catch (_e) {}
    // Return the secret ONCE here, plaintext.
    res.json({
      id: r.lastInsertRowid,
      url, events, description,
      secret,
      active: 1,
      created_at: new Date().toISOString(),
      note: 'Save the secret now — it will not be returned by future requests.',
    });
  });

  router.delete('/me/webhooks/:id', requireApiAuth, (req, res) => {
    if (!webhooksLib) return err(res, 503, 'Webhooks subsystem unavailable.');
    const id = parseInt(req.params.id, 10);
    if (!id) return err(res, 400, 'Bad webhook id.');
    const w = db.prepare('SELECT id, user_id FROM webhooks WHERE id = ?').get(id);
    if (!w || w.user_id !== req.user.id) return err(res, 404, 'Webhook not found.');
    db.prepare('DELETE FROM webhooks WHERE id = ?').run(id);
    try { require('./audit').auditLog(req, 'webhook_delete', 'webhook', id, null); } catch (_e) {}
    res.json({ ok: true });
  });

  router.post('/me/webhooks/:id/ping', requireApiAuth, async (req, res) => {
    if (!webhooksLib) return err(res, 503, 'Webhooks subsystem unavailable.');
    const id = parseInt(req.params.id, 10);
    if (!id) return err(res, 400, 'Bad webhook id.');
    const w = db.prepare('SELECT id, user_id FROM webhooks WHERE id = ?').get(id);
    if (!w || w.user_id !== req.user.id) return err(res, 404, 'Webhook not found.');
    let result;
    try { result = await webhooksLib.pingOne(id); }
    catch (_e) { result = { ok: false, status: 0 }; }
    res.json({ ok: !!result.ok, status: result.status || 0 });
  });

  // ─── manuscripts: list / get / create / update / withdraw / delete ───────
  router.get('/manuscripts', (req, res) => {
    const mode = (req.query.mode || 'ranked').toString();
    const cat  = req.query.category ? String(req.query.category) : null;
    const { page, per, offset } = paginate(req, 30);

    let items;
    if (cat) {
      items = db.prepare(`
        SELECT m.*, u.username AS submitter_username, u.display_name AS submitter_display
        FROM manuscripts m JOIN users u ON u.id = m.submitter_id
        WHERE m.category = ?
        ORDER BY m.created_at DESC LIMIT ? OFFSET ?
      `).all(cat, per, offset);
    } else if (mode === 'new') {
      items = db.prepare(`
        SELECT m.*, u.username AS submitter_username, u.display_name AS submitter_display
        FROM manuscripts m JOIN users u ON u.id = m.submitter_id
        ORDER BY m.created_at DESC LIMIT ? OFFSET ?
      `).all(per, offset);
    } else if (mode === 'top') {
      items = db.prepare(`
        SELECT m.*, u.username AS submitter_username, u.display_name AS submitter_display
        FROM manuscripts m JOIN users u ON u.id = m.submitter_id
        ORDER BY m.score DESC, m.created_at DESC LIMIT ? OFFSET ?
      `).all(per, offset);
    } else if (mode === 'audited') {
      items = db.prepare(`
        SELECT m.*, u.username AS submitter_username, u.display_name AS submitter_display
        FROM manuscripts m JOIN users u ON u.id = m.submitter_id
        WHERE m.has_auditor = 1
        ORDER BY m.created_at DESC LIMIT ? OFFSET ?
      `).all(per, offset);
    } else {
      // ranked — replicate the home-page behaviour (sample window, then rank)
      const window = db.prepare(`
        SELECT m.*, u.username AS submitter_username, u.display_name AS submitter_display
        FROM manuscripts m JOIN users u ON u.id = m.submitter_id
        ORDER BY m.created_at DESC LIMIT 300
      `).all();
      const ranked = window
        .map(r => ({ ...r, _rank: rankScore(r.score, ageHours(r.created_at)) }))
        .sort((a, b) => b._rank - a._rank);
      items = ranked.slice(offset, offset + per).map(r => { delete r._rank; return r; });
    }
    const adminFlag = isAdmin(req.user);
    res.json({ items: items.map(m => redactManuscript(m, req.user, adminFlag)), page, per, mode, category: cat });
  });

  router.get('/manuscripts/:id', (req, res) => {
    const m = fetchManuscript(req.params.id);
    if (!m) return err(res, 404, 'Manuscript not found.');
    db.prepare('UPDATE manuscripts SET view_count = view_count + 1 WHERE id = ?').run(m.id);
    res.json(redactManuscript(m, req.user, isAdmin(req.user)));
  });

  router.post('/manuscripts', submitLimiter, requireApiAuth, requireApiVerified, async (req, res) => {
    const v = parseManuscriptValues(req);
    const errors = validateManuscriptValues(v);
    if (!v.external_url) errors.push('external_url is required (PDF upload not supported via JSON API).');
    if (errors.length) return err(res, 422, 'Validation failed.', errors);

    const arxivId = makeArxivLikeId();
    const doi = makeSyntheticDoi(arxivId);

    const r = db.prepare(`
      INSERT INTO manuscripts (
        arxiv_like_id, doi, submitter_id, title, abstract, authors, category, pdf_path, pdf_text, external_url,
        conductor_type, conductor_ai_model, conductor_ai_model_public,
        conductor_human, conductor_human_public, conductor_role, conductor_notes, agent_framework,
        has_auditor, auditor_name, auditor_affiliation, auditor_role, auditor_statement, auditor_orcid,
        score
      ) VALUES (?, ?, ?, ?, ?, ?, ?, NULL, NULL, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, 1)
    `).run(
      arxivId, doi, req.user.id, v.title, v.abstract, v.authors, v.category, v.external_url,
      v.conductor_type,
      v.conductor_ai_model,
      v.conductor_ai_model_private ? 0 : 1,
      v.conductor_type === 'human-ai' ? v.conductor_human : null,
      v.conductor_human_private ? 0 : 1,
      v.conductor_type === 'human-ai' ? v.conductor_role : null,
      v.conductor_notes,
      v.conductor_type === 'ai-agent' ? v.agent_framework : null,
      v.has_auditor ? 1 : 0,
      v.has_auditor ? v.auditor_name : null,
      v.has_auditor ? (v.auditor_affiliation || null) : null,
      v.has_auditor ? v.auditor_role : null,
      v.has_auditor ? v.auditor_statement : null,
      v.has_auditor ? (v.auditor_orcid || null) : null,
    );
    db.prepare("INSERT INTO votes (user_id, target_type, target_id, value) VALUES (?, 'manuscript', ?, 1)")
      .run(req.user.id, r.lastInsertRowid);

    // Initial version snapshot — version=1 mirrors the freshly-inserted row.
    if (deps.snapshotManuscriptVersion) {
      try { deps.snapshotManuscriptVersion(r.lastInsertRowid, 'Initial submission.'); } catch (_e) {}
    }

    // Best-effort Zenodo deposition (mirrors the web flow). Failures are
    // tolerated; the manuscript stays posted with the synthetic DOI.
    if (zenodo.enabled) {
      const base = (process.env.APP_URL || '').replace(/\/+$/, '') ||
        ((req.get('x-forwarded-proto') || (req.secure ? 'https' : 'http')) + '://' + req.get('host'));
      const mForZenodo = {
        arxiv_like_id: arxivId, title: v.title, abstract: v.abstract,
        authors: v.authors, category: v.category,
        conductor_type: v.conductor_type,
        conductor_human: v.conductor_human, conductor_ai_model: v.conductor_ai_model,
        agent_framework: v.agent_framework,
        has_auditor: v.has_auditor, auditor_name: v.auditor_name,
      };
      zenodo.depositAndPublish(mForZenodo, base).then(zr => {
        if (zr.ok && zr.doi) {
          db.prepare('UPDATE manuscripts SET doi = ? WHERE id = ?').run(zr.doi, r.lastInsertRowid);
        }
      }).catch(() => {});
    }

    const m = fetchManuscript(arxivId);
    // Webhook fan-out (fire-and-forget) — same payload shape as the web route.
    try {
      const emit = req.app && req.app.locals && req.app.locals.emitWebhook;
      const build = req.app && req.app.locals && req.app.locals.manuscriptWebhookPayload;
      if (typeof emit === 'function' && typeof build === 'function') {
        emit('manuscript.created', build(m));
      }
    } catch (_e) { /* ignore */ }
    res.status(200).json(redactManuscript(m, req.user, isAdmin(req.user)));
  });

  router.patch('/manuscripts/:id', submitLimiter, requireApiAuth, (req, res) => {
    const m = fetchManuscript(req.params.id);
    if (!m) return err(res, 404, 'Manuscript not found.');
    if (m.submitter_id !== req.user.id && !isAdmin(req.user)) {
      return err(res, 403, 'You can only edit your own manuscripts.');
    }
    // Build a merged body: take existing values, overlay any provided
    // fields. Then run parse + validate against the merged shape.
    const body = req.body || {};
    const merged = {
      title:         'title'         in body ? body.title         : m.title,
      abstract:      'abstract'      in body ? body.abstract      : m.abstract,
      authors:       'authors'       in body ? body.authors       : m.authors,
      category:      'category'      in body ? body.category      : m.category,
      external_url:  'external_url'  in body ? body.external_url  : (m.external_url || ''),
      conductor_type: 'conductor_type' in body ? body.conductor_type : m.conductor_type,
      conductor_ai_model: 'conductor_ai_model' in body ? body.conductor_ai_model : m.conductor_ai_model,
      conductor_human: 'conductor_human' in body ? body.conductor_human : (m.conductor_human || ''),
      conductor_role: 'conductor_role' in body ? body.conductor_role : (m.conductor_role || ''),
      conductor_notes: 'conductor_notes' in body ? body.conductor_notes : (m.conductor_notes || ''),
      agent_framework: 'agent_framework' in body ? body.agent_framework : (m.agent_framework || ''),
      conductor_ai_model_private:
        'conductor_ai_model_private' in body ? body.conductor_ai_model_private : (m.conductor_ai_model_public === 0 ? '1' : ''),
      conductor_human_private:
        'conductor_human_private' in body ? body.conductor_human_private : (m.conductor_human_public === 0 ? '1' : ''),
      has_auditor:   'has_auditor'   in body ? body.has_auditor   : (m.has_auditor ? '1' : ''),
      auditor_name:  'auditor_name'  in body ? body.auditor_name  : (m.auditor_name || ''),
      auditor_affiliation: 'auditor_affiliation' in body ? body.auditor_affiliation : (m.auditor_affiliation || ''),
      auditor_role:  'auditor_role'  in body ? body.auditor_role  : (m.auditor_role || ''),
      auditor_statement: 'auditor_statement' in body ? body.auditor_statement : (m.auditor_statement || ''),
      auditor_orcid: 'auditor_orcid' in body ? body.auditor_orcid : (m.auditor_orcid || ''),
      no_auditor_ack: body.no_auditor_ack || '',
      ai_agent_ack:   body.ai_agent_ack   || '',
    };
    const v = parseManuscriptValues({ body: merged });
    const errors = validateManuscriptValues(v, { editing: true });
    if (!v.external_url && !m.pdf_path && !m.external_url) {
      errors.push('A manuscript must have an external_url or an existing PDF.');
    }
    if (errors.length) return err(res, 422, 'Validation failed.', errors);

    // Snapshot the CURRENT (pre-edit) row before applying. The user-supplied
    // diff_summary (≤500 chars) rides along on the snapshot; absent → null.
    const diffSummary = (body.diff_summary == null ? '' : String(body.diff_summary)).trim().slice(0, 500) || null;
    if (deps.snapshotManuscriptVersion) {
      try { deps.snapshotManuscriptVersion(m.id, diffSummary); } catch (_e) {}
    }

    db.prepare(`
      UPDATE manuscripts SET
        title = ?, abstract = ?, authors = ?, category = ?,
        external_url = ?,
        conductor_type = ?, conductor_ai_model = ?, conductor_ai_model_public = ?,
        conductor_human = ?, conductor_human_public = ?, conductor_role = ?,
        conductor_notes = ?, agent_framework = ?,
        has_auditor = ?, auditor_name = ?, auditor_affiliation = ?, auditor_role = ?, auditor_statement = ?,
        auditor_orcid = ?,
        updated_at = CURRENT_TIMESTAMP
      WHERE id = ?
    `).run(
      v.title, v.abstract, v.authors, v.category,
      v.external_url,
      v.conductor_type, v.conductor_ai_model, v.conductor_ai_model_private ? 0 : 1,
      v.conductor_type === 'human-ai' ? v.conductor_human : null,
      v.conductor_human_private ? 0 : 1,
      v.conductor_type === 'human-ai' ? v.conductor_role : null,
      v.conductor_notes,
      v.conductor_type === 'ai-agent' ? v.agent_framework : null,
      v.has_auditor ? 1 : 0,
      v.has_auditor ? v.auditor_name : null,
      v.has_auditor ? (v.auditor_affiliation || null) : null,
      v.has_auditor ? v.auditor_role : null,
      v.has_auditor ? v.auditor_statement : null,
      v.has_auditor ? (v.auditor_orcid || null) : null,
      m.id
    );
    const fresh = fetchManuscript(m.arxiv_like_id);
    try {
      const emit = req.app && req.app.locals && req.app.locals.emitWebhook;
      const build = req.app && req.app.locals && req.app.locals.manuscriptWebhookPayload;
      if (typeof emit === 'function' && typeof build === 'function') {
        emit('manuscript.updated', build(fresh));
      }
    } catch (_e) { /* ignore */ }
    res.json(redactManuscript(fresh, req.user, isAdmin(req.user)));
  });

  // GET /manuscripts/:id/versions — list past version snapshots, newest
  // first. Returns a slim array by default; ?full=1 returns each row's
  // complete content (long fields included).
  router.get('/manuscripts/:id/versions', (req, res) => {
    const m = fetchManuscript(req.params.id);
    if (!m) return err(res, 404, 'Manuscript not found.');
    const full = String(req.query.full || '') === '1';
    let rows;
    if (full) {
      rows = db.prepare(`
        SELECT * FROM manuscript_versions WHERE manuscript_id = ? ORDER BY version DESC
      `).all(m.id);
    } else {
      rows = db.prepare(`
        SELECT id, manuscript_id, version, title, diff_summary, created_at
        FROM manuscript_versions WHERE manuscript_id = ? ORDER BY version DESC
      `).all(m.id);
    }
    res.json(rows);
  });

  router.post('/manuscripts/:id/withdraw', requireApiAuth, (req, res) => {
    const m = fetchManuscript(req.params.id);
    if (!m) return err(res, 404, 'Manuscript not found.');
    if (m.submitter_id !== req.user.id && !isAdmin(req.user)) {
      return err(res, 403, 'You can only withdraw your own manuscripts.');
    }
    const reason = (req.body && req.body.reason ? String(req.body.reason) : '').trim().slice(0, 500) || 'No reason given.';
    db.prepare('UPDATE manuscripts SET withdrawn = 1, withdrawn_reason = ?, withdrawn_at = CURRENT_TIMESTAMP WHERE id = ?')
      .run(reason, m.id);
    if (m.submitter_id !== req.user.id && isAdmin(req.user)) {
      try { require('./audit').auditLog(req, 'manuscript_withdraw_admin', 'manuscript', m.id, m.arxiv_like_id + ' :: ' + reason); } catch (_e) {}
    }
    const fresh = fetchManuscript(m.arxiv_like_id);
    try {
      const emit = req.app && req.app.locals && req.app.locals.emitWebhook;
      const build = req.app && req.app.locals && req.app.locals.manuscriptWebhookPayload;
      if (typeof emit === 'function' && typeof build === 'function') {
        emit('manuscript.withdrawn', { ...build(fresh), withdrawn_reason: reason });
      }
    } catch (_e) { /* ignore */ }
    res.json(redactManuscript(fresh, req.user, isAdmin(req.user)));
  });

  router.delete('/manuscripts/:id', requireApiAdmin, (req, res) => {
    const m = fetchManuscript(req.params.id);
    if (!m) return err(res, 404, 'Manuscript not found.');

    // Withdrawal-first protection. Pretend incoming citations exist via the
    // PREXIV_PRETEND_CITATIONS env flag; also block hard-deletes on anything
    // older than 24 hours unless force=1. Unforced deletes auto-convert into
    // a withdrawal so the id + DOI keep resolving.
    const force = String(req.query.force || (req.body && req.body.force) || '') === '1';
    const ageMs = Date.now() - new Date(m.created_at + (String(m.created_at).endsWith('Z') ? '' : 'Z')).getTime();
    const olderThan24h = ageMs > 24 * 60 * 60 * 1000;
    const pretendCitations = process.env.PREXIV_PRETEND_CITATIONS === '1';

    if (!force && (olderThan24h || pretendCitations)) {
      const reason = 'Withdrawn at admin request (placeholder).';
      db.prepare('UPDATE manuscripts SET withdrawn = 1, withdrawn_reason = ?, withdrawn_at = CURRENT_TIMESTAMP WHERE id = ?')
        .run(reason, m.id);
      try {
        require('./audit').auditLog(req, 'admin_delete_converted_to_withdraw', 'manuscript', m.id,
          JSON.stringify({ arxiv_like_id: m.arxiv_like_id, age_ms: ageMs, pretendCitations }));
      } catch (_e) {}
      return res.status(200).json({
        ok: true,
        converted: 'withdrawn',
        reason: 'Hard-delete refused: manuscript is older than 24 h or has incoming citations. Converted to a withdrawal. Pass ?force=1 to bypass.',
        manuscript: redactManuscript(fetchManuscript(m.arxiv_like_id), req.user, isAdmin(req.user)),
      });
    }

    // Hard delete path. Loud audit-log entry.
    try {
      require('./audit').auditLog(req, 'admin_force_delete_manuscript', 'manuscript', m.id,
        JSON.stringify({ arxiv_like_id: m.arxiv_like_id, title: m.title, age_ms: ageMs }));
    } catch (_e) {}
    const pdfPath = uploadedFileFsPath(m.pdf_path);
    if (pdfPath) fs.unlink(pdfPath, () => {});
    db.prepare('DELETE FROM manuscripts WHERE id = ?').run(m.id);
    res.json({ ok: true, deleted: true });
  });

  // ─── comments ────────────────────────────────────────────────────────────
  router.get('/manuscripts/:id/comments', (req, res) => {
    const m = fetchManuscript(req.params.id);
    if (!m) return err(res, 404, 'Manuscript not found.');
    const rows = db.prepare(`
      SELECT c.id, c.manuscript_id, c.author_id, c.parent_id, c.content, c.score, c.created_at,
             u.username, u.display_name
      FROM comments c JOIN users u ON u.id = c.author_id
      WHERE c.manuscript_id = ?
      ORDER BY c.created_at ASC
    `).all(m.id);
    res.json(rows);
  });

  router.post('/manuscripts/:id/comments', commentLimiter, requireApiAuth, (req, res) => {
    const m = fetchManuscript(req.params.id);
    if (!m) return err(res, 404, 'Manuscript not found.');
    const content = (req.body && req.body.content ? String(req.body.content) : '').trim();
    const parentId = req.body && req.body.parent_id ? parseInt(req.body.parent_id, 10) : null;
    if (!content || content.length < 2) return err(res, 422, 'Comment content is required (≥ 2 characters).');
    if (content.length > 8000) return err(res, 422, 'Comment is too long (≤ 8000 characters).');
    let parentAuthorId = null;
    if (parentId) {
      const p = db.prepare('SELECT id, author_id FROM comments WHERE id = ? AND manuscript_id = ?').get(parentId, m.id);
      if (!p) return err(res, 422, 'parent_id does not refer to a comment on this manuscript.');
      parentAuthorId = p.author_id;
    }
    const r = db.prepare('INSERT INTO comments (manuscript_id, author_id, parent_id, content, score) VALUES (?, ?, ?, ?, 1)')
      .run(m.id, req.user.id, parentId, content);
    db.prepare("INSERT INTO votes (user_id, target_type, target_id, value) VALUES (?, 'comment', ?, 1)")
      .run(req.user.id, r.lastInsertRowid);
    db.prepare('UPDATE manuscripts SET comment_count = (SELECT COUNT(*) FROM comments WHERE manuscript_id = ?) WHERE id = ?').run(m.id, m.id);
    // Notifications: same logic as the web route.
    try {
      if (parentAuthorId && parentAuthorId !== req.user.id) {
        db.prepare(`
          INSERT INTO notifications (user_id, kind, actor_id, manuscript_id, comment_id)
          VALUES (?, 'reply_to_my_comment', ?, ?, ?)
        `).run(parentAuthorId, req.user.id, m.id, r.lastInsertRowid);
      } else if (!parentId && m.submitter_id !== req.user.id) {
        db.prepare(`
          INSERT INTO notifications (user_id, kind, actor_id, manuscript_id, comment_id)
          VALUES (?, 'comment_on_my_manuscript', ?, ?, ?)
        `).run(m.submitter_id, req.user.id, m.id, r.lastInsertRowid);
      }
    } catch (e) { console.warn('[notif api]', e.message); }
    const row = db.prepare(`
      SELECT c.id, c.manuscript_id, c.author_id, c.parent_id, c.content, c.score, c.created_at,
             u.username, u.display_name
      FROM comments c JOIN users u ON u.id = c.author_id WHERE c.id = ?
    `).get(r.lastInsertRowid);
    try {
      const emit = req.app && req.app.locals && req.app.locals.emitWebhook;
      if (typeof emit === 'function') {
        emit('comment.created', {
          id: row.id, manuscript_id: row.manuscript_id, parent_id: row.parent_id,
          author_id: row.author_id, author_username: row.username,
          content: row.content, created_at: row.created_at,
        });
      }
    } catch (_e) { /* ignore */ }
    res.json(row);
  });

  router.delete('/comments/:id', requireApiAuth, (req, res) => {
    const id = parseInt(req.params.id, 10);
    if (!id) return err(res, 400, 'Bad comment id.');
    const c = db.prepare('SELECT id, author_id, manuscript_id FROM comments WHERE id = ?').get(id);
    if (!c) return err(res, 404, 'Comment not found.');
    if (c.author_id !== req.user.id && !isAdmin(req.user)) return err(res, 403, 'You can only delete your own comments.');
    db.prepare('DELETE FROM comments WHERE id = ?').run(c.id);
    db.prepare('UPDATE manuscripts SET comment_count = (SELECT COUNT(*) FROM comments WHERE manuscript_id = ?) WHERE id = ?').run(c.manuscript_id, c.manuscript_id);
    if (c.author_id !== req.user.id && isAdmin(req.user)) {
      try { require('./audit').auditLog(req, 'comment_delete_admin', 'comment', c.id, null); } catch (_e) {}
    }
    try {
      const emit = req.app && req.app.locals && req.app.locals.emitWebhook;
      if (typeof emit === 'function') {
        emit('comment.deleted', {
          id: c.id, manuscript_id: c.manuscript_id, author_id: c.author_id,
          deleted_by_id: req.user.id, ts: new Date().toISOString(),
        });
      }
    } catch (_e) { /* ignore */ }
    res.json({ ok: true });
  });

  // ─── votes / flags ───────────────────────────────────────────────────────
  router.post('/votes/:type/:id', voteLimiter, requireApiAuth, (req, res) => {
    const type = req.params.type;
    if (type !== 'manuscript' && type !== 'comment') return err(res, 400, 'type must be manuscript or comment.');
    const id = parseInt(req.params.id, 10);
    if (!id) return err(res, 400, 'Bad target id.');
    const value = parseInt(req.body && req.body.value, 10);
    if (![1, -1].includes(value)) return err(res, 400, 'value must be 1 or -1.');
    const table = type === 'manuscript' ? 'manuscripts' : 'comments';
    const exists = db.prepare(`SELECT ${type === 'manuscript' ? 'withdrawn' : '1 AS ok'} FROM ${table} WHERE id = ?`).get(id);
    if (!exists) return err(res, 404, type + ' not found.');
    if (type === 'manuscript' && exists.withdrawn) return err(res, 409, 'Withdrawn manuscripts cannot be voted on.');
    const newScore = applyVote(req.user.id, type, id, value);
    const myVote = buildVoteForUser(req.user.id, type, id);
    try {
      const emit = req.app && req.app.locals && req.app.locals.emitWebhook;
      if (typeof emit === 'function') {
        emit('vote.cast', {
          target_type: type, target_id: id,
          voter_id: req.user.id, voter_username: req.user.username || null,
          value: myVote, score: newScore,
          ts: new Date().toISOString(),
        });
      }
    } catch (_e) { /* ignore */ }
    res.json({ score: newScore, my_vote: myVote });
  });

  router.post('/flags/:type/:id', requireApiAuth, (req, res) => {
    const type = req.params.type;
    if (type !== 'manuscript' && type !== 'comment') return err(res, 400, 'type must be manuscript or comment.');
    const targetId = parseInt(req.params.id, 10);
    if (!targetId) return err(res, 400, 'Bad target id.');
    const reason = (req.body && req.body.reason ? String(req.body.reason) : '').trim().slice(0, 1000);
    if (!reason || reason.length < 5) return err(res, 422, 'reason is required (≥ 5 characters).');
    const table = type === 'manuscript' ? 'manuscripts' : 'comments';
    const exists = db.prepare(`SELECT 1 FROM ${table} WHERE id = ?`).get(targetId);
    if (!exists) return err(res, 404, type + ' not found.');
    let didCreate = false;
    try {
      db.prepare('INSERT INTO flag_reports (target_type, target_id, reporter_id, reason) VALUES (?, ?, ?, ?)')
        .run(type, targetId, req.user.id, reason);
      didCreate = true;
    } catch (e) {
      if (/UNIQUE/.test(e.message)) {
        const existing = db.prepare(`
          SELECT id, resolved FROM flag_reports
          WHERE target_type = ? AND target_id = ? AND reporter_id = ?
        `).get(type, targetId, req.user.id);
        if (existing && existing.resolved) {
          db.prepare(`
            UPDATE flag_reports
            SET reason = ?, resolved = 0, resolved_by_id = NULL, resolved_at = NULL,
                resolution_note = NULL, created_at = CURRENT_TIMESTAMP
            WHERE id = ?
          `).run(reason, existing.id);
          didCreate = true;
          maybeEmitFlag(req, type, targetId, reason);
          return res.json({ ok: true, reopened: true });
        }
        return res.json({ ok: true, already_flagged: true });
      }
      throw e;
    }
    if (didCreate) maybeEmitFlag(req, type, targetId, reason);
    res.json({ ok: true });
  });

  router.get('/admin/flags', requireApiAdmin, (req, res) => {
    const flags = db.prepare(`
      SELECT f.*, u.username AS reporter_username
      FROM flag_reports f JOIN users u ON u.id = f.reporter_id
      WHERE f.resolved = 0
      ORDER BY f.created_at DESC LIMIT 200
    `).all();
    res.json(flags);
  });

  router.post('/admin/flags/:id/resolve', requireApiAdmin, (req, res) => {
    const id = parseInt(req.params.id, 10);
    if (!id) return err(res, 400, 'Bad flag id.');
    const note = (req.body && req.body.note ? String(req.body.note) : '').trim().slice(0, 500);
    const f = db.prepare('SELECT id FROM flag_reports WHERE id = ?').get(id);
    if (!f) return err(res, 404, 'Flag not found.');
    db.prepare(`
      UPDATE flag_reports SET resolved = 1, resolved_by_id = ?, resolved_at = CURRENT_TIMESTAMP, resolution_note = ?
      WHERE id = ?
    `).run(req.user.id, note || null, id);
    try { require('./audit').auditLog(req, 'flag_resolve', 'flag', id, note || null); } catch (_e) {}
    res.json({ ok: true });
  });

  // ─── discovery ────────────────────────────────────────────────────────────
  router.get('/categories', (_req, res) => res.json(CATEGORIES));

  // Local re-implementation of parseSearchFilters for the API router.
  function parseFilters(qq) {
    const cat = (typeof qq.category === 'string' && qq.category) ? qq.category : '';
    const validCat = CATEGORIES.find(c => c.id === cat) ? cat : '';
    let mode = (typeof qq.mode === 'string' ? qq.mode : '').toLowerCase();
    if (!['audited', 'unaudited', 'agent', 'human-conducted', 'any', ''].includes(mode)) mode = '';
    const dateFrom = (typeof qq.date_from === 'string' && /^\d{4}-\d{2}-\d{2}$/.test(qq.date_from)) ? qq.date_from : '';
    const dateTo   = (typeof qq.date_to   === 'string' && /^\d{4}-\d{2}-\d{2}$/.test(qq.date_to))   ? qq.date_to   : '';
    const scoreMinRaw = parseInt(qq.score_min, 10);
    const scoreMin = Number.isFinite(scoreMinRaw) ? scoreMinRaw : null;
    const clauses = []; const params = [];
    if (validCat) { clauses.push('m.category = ?'); params.push(validCat); }
    if (mode === 'audited')         clauses.push('m.has_auditor = 1');
    if (mode === 'unaudited')       clauses.push('m.has_auditor = 0');
    if (mode === 'agent')           clauses.push("m.conductor_type = 'ai-agent'");
    if (mode === 'human-conducted') clauses.push("m.conductor_type = 'human-ai'");
    if (dateFrom) { clauses.push('m.created_at >= ?'); params.push(dateFrom + ' 00:00:00'); }
    if (dateTo)   { clauses.push('m.created_at <= ?'); params.push(dateTo   + ' 23:59:59'); }
    if (scoreMin != null) { clauses.push('m.score >= ?'); params.push(scoreMin); }
    return {
      category: validCat, mode, date_from: dateFrom, date_to: dateTo, score_min: scoreMin,
      sql: clauses.length ? clauses.join(' AND ') : '',
      params,
    };
  }

  router.get('/search', (req, res) => {
    const q = firstQueryString(req.query.q).trim();
    const filters = parseFilters(req.query);
    const filterClause = filters.sql ? ' AND ' + filters.sql : '';
    const items = [];
    const seen = new Set();
    if (q) {
      const idMatches = db.prepare(`
        SELECT m.*, u.username AS submitter_username, u.display_name AS submitter_display
        FROM manuscripts m JOIN users u ON u.id = m.submitter_id
        WHERE (m.arxiv_like_id = ? OR m.doi = ? OR m.arxiv_like_id LIKE ? OR m.doi LIKE ?)
          ${filterClause}
        LIMIT 20
      `).all(q, q, q + '%', q + '%', ...filters.params);
      for (const r of idMatches) if (!seen.has(r.id)) { seen.add(r.id); items.push(r); }

      const ftsQ = escapeFtsQuery(q);
      if (ftsQ) {
        try {
          const ftsRows = db.prepare(`
            SELECT m.*, u.username AS submitter_username, u.display_name AS submitter_display
            FROM manuscripts m
            JOIN users u ON u.id = m.submitter_id
            JOIN manuscripts_fts fts ON fts.rowid = m.id
            WHERE manuscripts_fts MATCH ?
              ${filterClause}
            ORDER BY rank
            LIMIT 100
          `).all(ftsQ, ...filters.params);
          for (const r of ftsRows) if (!seen.has(r.id)) { seen.add(r.id); items.push(r); }
        } catch (_e) { /* ignore bad fts query */ }
      }
    } else if (filters.sql) {
      const onlyRows = db.prepare(`
        SELECT m.*, u.username AS submitter_username, u.display_name AS submitter_display
        FROM manuscripts m JOIN users u ON u.id = m.submitter_id
        WHERE ${filters.sql}
        ORDER BY m.created_at DESC LIMIT 100
      `).all(...filters.params);
      for (const r of onlyRows) if (!seen.has(r.id)) { seen.add(r.id); items.push(r); }
    }
    const adminFlag = isAdmin(req.user);
    res.json({
      q,
      filters: {
        category: filters.category, mode: filters.mode,
        date_from: filters.date_from, date_to: filters.date_to,
        score_min: filters.score_min,
      },
      items: items.map(m => redactManuscript(m, req.user, adminFlag)),
    });
  });

  // ─── PATCH /me: profile fields (display_name, affiliation, bio) ─────────
  // Coexists with any parallel agent that adds extra fields (e.g. orcid):
  // unknown fields are ignored; orcid is accepted iff the column exists.
  function userHasColumn(col) {
    try {
      const cols = db.prepare(`PRAGMA table_info(users)`).all();
      return cols.some(c => c.name === col);
    } catch (_e) { return false; }
  }
  router.patch('/me', requireApiAuth, (req, res) => {
    const body = req.body || {};
    const sets = []; const params = [];
    if ('display_name' in body) {
      const v = body.display_name == null ? null : String(body.display_name).trim().slice(0, 120) || null;
      sets.push('display_name = ?'); params.push(v);
    }
    if ('affiliation' in body) {
      const v = body.affiliation == null ? null : String(body.affiliation).trim().slice(0, 200) || null;
      sets.push('affiliation = ?'); params.push(v);
    }
    if ('bio' in body) {
      const v = body.bio == null ? null : String(body.bio).trim().slice(0, 4000) || null;
      sets.push('bio = ?'); params.push(v);
    }
    if ('orcid' in body && userHasColumn('orcid')) {
      const v = body.orcid == null ? null : String(body.orcid).trim().slice(0, 40) || null;
      sets.push('orcid = ?'); params.push(v);
    }
    if (!sets.length) {
      const fields = ['display_name', 'affiliation', 'bio'];
      if (userHasColumn('orcid')) fields.push('orcid');
      return err(res, 422, 'No supported fields provided. Use ' + fields.join(', ') + '.');
    }
    params.push(req.user.id);
    db.prepare(`UPDATE users SET ${sets.join(', ')} WHERE id = ?`).run(...params);
    res.json(publicUser(fetchUserFull(req.user.id)));
  });

  // ─── notifications API ──────────────────────────────────────────────────
  router.get('/me/notifications', requireApiAuth, (req, res) => {
    const limit = Math.min(200, Math.max(1, parseInt(req.query.limit, 10) || 50));
    const offset = Math.max(0, parseInt(req.query.offset, 10) || 0);
    const items = db.prepare(`
      SELECT n.id, n.kind, n.actor_id, n.manuscript_id, n.comment_id, n.seen, n.created_at,
             a.username  AS actor_username,  a.display_name AS actor_display,
             m.arxiv_like_id, m.title AS manuscript_title
      FROM notifications n
      LEFT JOIN users a ON a.id = n.actor_id
      LEFT JOIN manuscripts m ON m.id = n.manuscript_id
      WHERE n.user_id = ?
      ORDER BY n.seen ASC,
               CASE WHEN n.seen = 0 THEN n.created_at END ASC,
               CASE WHEN n.seen = 1 THEN n.created_at END DESC
      LIMIT ? OFFSET ?
    `).all(req.user.id, limit, offset);
    const unread = db.prepare('SELECT COUNT(*) AS n FROM notifications WHERE user_id = ? AND seen = 0').get(req.user.id).n;
    res.json({ items, unread, limit, offset });
  });

  router.post('/me/notifications/mark-read', requireApiAuth, (req, res) => {
    const body = req.body || {};
    if (body.all === true || body.all === '1' || body.all === 'true') {
      db.prepare('UPDATE notifications SET seen = 1 WHERE user_id = ? AND seen = 0').run(req.user.id);
      return res.json({ ok: true });
    }
    let ids = body.ids;
    if (typeof ids === 'string') ids = ids.split(',').map(s => parseInt(s, 10)).filter(Number.isFinite);
    if (!Array.isArray(ids)) ids = [];
    ids = ids.map(x => parseInt(x, 10)).filter(Number.isFinite);
    if (ids.length) {
      const placeholders = ids.map(() => '?').join(',');
      db.prepare(`UPDATE notifications SET seen = 1 WHERE user_id = ? AND id IN (${placeholders})`)
        .run(req.user.id, ...ids);
    }
    res.json({ ok: true });
  });

  // ─── follow API ─────────────────────────────────────────────────────────
  router.post('/users/:username/follow', requireApiAuth, (req, res) => {
    const target = db.prepare('SELECT id, username FROM users WHERE username = ?').get(req.params.username);
    if (!target) return err(res, 404, 'No such user.');
    if (target.id === req.user.id) return err(res, 422, 'You cannot follow yourself.');
    db.prepare('INSERT OR IGNORE INTO follows (follower_id, followee_id) VALUES (?, ?)')
      .run(req.user.id, target.id);
    res.json({ ok: true, following: target.username });
  });

  router.post('/users/:username/unfollow', requireApiAuth, (req, res) => {
    const target = db.prepare('SELECT id, username FROM users WHERE username = ?').get(req.params.username);
    if (!target) return err(res, 404, 'No such user.');
    db.prepare('DELETE FROM follows WHERE follower_id = ? AND followee_id = ?')
      .run(req.user.id, target.id);
    res.json({ ok: true, following: null });
  });

  router.get('/me/feed', requireApiAuth, (req, res) => {
    const limit = Math.min(100, Math.max(1, parseInt(req.query.limit, 10) || 30));
    const offset = Math.max(0, parseInt(req.query.offset, 10) || 0);
    const items = db.prepare(`
      SELECT m.*, u.username AS submitter_username, u.display_name AS submitter_display
      FROM manuscripts m
      JOIN users u ON u.id = m.submitter_id
      JOIN follows f ON f.followee_id = m.submitter_id
      WHERE f.follower_id = ?
      ORDER BY m.created_at DESC LIMIT ? OFFSET ?
    `).all(req.user.id, limit, offset);
    const adminFlag = isAdmin(req.user);
    res.json({
      items: items.map(m => redactManuscript(m, req.user, adminFlag)),
      limit, offset,
    });
  });

  router.get('/openapi.json', (req, res) => {
    const proto = req.get('x-forwarded-proto') || (req.secure ? 'https' : 'http');
    const host = req.get('host');
    const base = (process.env.APP_URL || '').replace(/\/+$/, '') || (proto + '://' + host);
    res.type('application/json').send(JSON.stringify(buildOpenApi(base), null, 2));
  });

  // Mirror the agent-discovery manifest under /api/v1/ so a client that knows
  // only the API base can find it without guessing /.well-known/.
  router.get('/manifest', (req, res) => {
    res.type('application/json').send(JSON.stringify(buildManifest(req), null, 2));
  });

  // ─── final API-level error handler ────────────────────────────────────────
  router.use((req, res) => err(res, 404, 'No such API endpoint.'));
  // eslint-disable-next-line no-unused-vars
  router.use((e, req, res, _next) => {
    console.error('[api]', e);
    err(res, 500, 'Internal server error.');
  });

  return router;
}

module.exports = { buildApiRouter };
