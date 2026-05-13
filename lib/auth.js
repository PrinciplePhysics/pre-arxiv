const bcrypt = require('bcryptjs');
const crypto = require('crypto');
const { db } = require('../db');
const { findUserByBearer } = require('./api-auth');

/**
 * Bcrypt-hash a plaintext password (cost 10).
 * @param {string} plain
 * @returns {string}
 */
function hashPassword(plain) {
  return bcrypt.hashSync(plain, 10);
}

/**
 * Constant-time compare a plaintext password against a bcrypt hash.
 * @param {string} plain
 * @param {string} hash
 * @returns {boolean}
 */
function verifyPassword(plain, hash) {
  return bcrypt.compareSync(plain, hash);
}

/**
 * Have-I-Been-Pwned k-anonymity check.
 *
 * Sends only the first 5 hex chars of the SHA-1 hash to the HIBP range API
 * and scans the response for our suffix. Returns true iff the hash appears
 * at least once in the breach corpus. On any network error or 3-second
 * timeout we warn-and-allow rather than blocking.
 * @param {string} plainPassword
 * @returns {Promise<boolean>}
 */
async function isPasswordPwned(plainPassword) {
  if (typeof plainPassword !== 'string' || !plainPassword) return false;
  let sha1;
  try {
    sha1 = crypto.createHash('sha1').update(plainPassword).digest('hex').toUpperCase();
  } catch (_e) { return false; }
  const prefix = sha1.slice(0, 5);
  const suffix = sha1.slice(5);
  try {
    const res = await fetch('https://api.pwnedpasswords.com/range/' + prefix, {
      headers: { 'Add-Padding': 'true', 'User-Agent': 'PreXiv-pwned-check' },
      signal: AbortSignal.timeout(3000),
    });
    if (!res.ok) {
      console.warn('[hibp] non-OK response', res.status);
      return false;
    }
    const text = await res.text();
    // Each line: SUFFIX:COUNT (hex suffix, decimal count). HIBP returns CRLF.
    for (const line of text.split(/\r?\n/)) {
      const idx = line.indexOf(':');
      if (idx === -1) continue;
      const sfx = line.slice(0, idx).trim().toUpperCase();
      if (sfx !== suffix) continue;
      const count = parseInt(line.slice(idx + 1).trim(), 10);
      if (Number.isFinite(count) && count >= 1) return true;
      return false;
    }
    return false;
  } catch (e) {
    console.warn('[hibp] check failed:', e.message || e);
    return false;
  }
}

/**
 * Express middleware. Resolve the current user from either a Bearer token
 * (scripts / AI agents) or the session cookie (browser), and attach the row
 * as `req.user` and `res.locals.user`. Bearer auth is allowed on both the
 * JSON API and protected web routes so an authorized agent can perform the
 * same actions a human can perform through the GUI.
 *
 * If a malformed/unknown Authorization header is present, refuse to fall back
 * to the cookie path. A request with a bad token plus a valid browser cookie
 * should not accidentally authenticate as the browser user.
 * @param {any} req
 * @param {any} res
 * @param {any} next
 */
function loadUser(req, res, next) {
  res.locals.user = null;
  const authHeader = req.headers.authorization || req.headers.Authorization;

  // Bearer auth wins when present. If it is malformed or unknown, do not fall
  // back to the browser session; otherwise an invalid Bearer header plus a
  // cookie can accidentally authenticate as the web user.
  const bearerUser = findUserByBearer(req);
  if (bearerUser) {
    req.user = bearerUser;
    res.locals.user = bearerUser;
    return next();
  }
  if (authHeader) return next();

  if (req.session && req.session.userId) {
    const u = db.prepare('SELECT id, username, display_name, affiliation, karma FROM users WHERE id = ?').get(req.session.userId);
    if (u) {
      req.user = u;
      res.locals.user = u;
    }
  }
  next();
}

/**
 * Express middleware. Bounce unauthenticated visitors to /login?next=<here>.
 * Use {@link requireApiAuth} (in lib/api.js) for API-level auth.
 * @param {any} req
 * @param {any} res
 * @param {any} next
 */
function requireAuth(req, res, next) {
  if (!req.user) {
    const target = encodeURIComponent(req.originalUrl);
    return res.redirect('/login?next=' + target);
  }
  next();
}

const RESERVED = new Set(['admin', 'root', 'pre-arxiv', 'prexiv', 'arxiv', 'system', 'moderator', 'mod']);

/**
 * Reject syntactically-bad or reserved usernames.
 * @param {string|null|undefined} u
 * @returns {string|null} null if valid, else a human-readable error
 */
function validateUsername(u) {
  if (!u || typeof u !== 'string') return 'Username is required.';
  if (u.length < 3 || u.length > 32) return 'Username must be 3–32 characters.';
  if (!/^[a-zA-Z0-9_-]+$/.test(u)) return 'Username may contain letters, digits, underscore, hyphen.';
  if (RESERVED.has(u.toLowerCase())) return 'That username is reserved.';
  return null;
}

module.exports = { hashPassword, verifyPassword, loadUser, requireAuth, validateUsername, isPasswordPwned };
