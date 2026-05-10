const bcrypt = require('bcryptjs');
const { db } = require('../db');
const { findUserByBearer } = require('./api-auth');

function hashPassword(plain) {
  return bcrypt.hashSync(plain, 10);
}
function verifyPassword(plain, hash) {
  return bcrypt.compareSync(plain, hash);
}

function loadUser(req, res, next) {
  res.locals.user = null;
  // Bearer auth wins when present — API clients shouldn't be subject to
  // browser-cookie / session semantics. If the header is malformed or the
  // token is unknown, we silently fall through to the session path; the
  // route-level auth check will reject the request if it requires auth.
  const bearerUser = findUserByBearer(req);
  if (bearerUser) {
    req.user = bearerUser;
    res.locals.user = bearerUser;
    return next();
  }
  if (req.session && req.session.userId) {
    const u = db.prepare('SELECT id, username, display_name, affiliation, karma FROM users WHERE id = ?').get(req.session.userId);
    if (u) {
      req.user = u;
      res.locals.user = u;
    }
  }
  next();
}

function requireAuth(req, res, next) {
  if (!req.user) {
    const target = encodeURIComponent(req.originalUrl);
    return res.redirect('/login?next=' + target);
  }
  next();
}

const RESERVED = new Set(['admin', 'root', 'pre-arxiv', 'prexiv', 'arxiv', 'system', 'moderator', 'mod']);
function validateUsername(u) {
  if (!u || typeof u !== 'string') return 'Username is required.';
  if (u.length < 3 || u.length > 32) return 'Username must be 3–32 characters.';
  if (!/^[a-zA-Z0-9_-]+$/.test(u)) return 'Username may contain letters, digits, underscore, hyphen.';
  if (RESERVED.has(u.toLowerCase())) return 'That username is reserved.';
  return null;
}

module.exports = { hashPassword, verifyPassword, loadUser, requireAuth, validateUsername };
