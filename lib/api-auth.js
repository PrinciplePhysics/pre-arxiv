// Bearer-token API authentication.
//
// Tokens are minted by `generateToken()` (a `prexiv_` prefix plus a 36-char
// base64url string from 27 random bytes) and stored in the `api_tokens`
// table as SHA-256 hashes — the plaintext is shown to the caller exactly
// once (at creation time) and never persisted. `findUserByBearer` looks the
// hash up on each authenticated request, honors `expires_at` if set, and
// touches `last_used_at` on every successful match so token-management UIs
// can show recency.
//
// The web UI continues to use the existing session / cookie flow; Bearer is
// API-only and is checked before the session in `loadUser`.

const crypto = require('crypto');
const { db } = require('../db');

const TOKEN_PREFIX = 'prexiv_';

/**
 * Mint a fresh API token with the `prexiv_` prefix and 36 base64url chars
 * of entropy (27 random bytes). Plaintext only — the caller must hash it
 * with `hashToken()` before storing.
 * @returns {string}
 */
function generateToken() {
  // 27 bytes -> 36 base64url chars.
  return TOKEN_PREFIX + crypto.randomBytes(27).toString('base64url');
}

/**
 * SHA-256 hex of the plaintext token. Used to look the token up in
 * `api_tokens.token_hash`.
 * @param {string} plain
 * @returns {string}
 */
function hashToken(plain) {
  return crypto.createHash('sha256').update(String(plain || '')).digest('hex');
}

/**
 * Extract the bearer token from the Authorization header, or null if absent.
 * @param {{headers?:{authorization?:string, Authorization?:string}}} req
 * @returns {string|null}
 */
function extractBearer(req) {
  const h = req && req.headers ? (req.headers.authorization || req.headers.Authorization) : null;
  if (!h || typeof h !== 'string') return null;
  const m = h.match(/^Bearer\s+(\S+)\s*$/i);
  return m ? m[1] : null;
}

/**
 * Returns the row from `users` (same shape `loadUser` returns from the
 * session path) or `null` if no valid Bearer token is present.
 * Also touches `api_tokens.last_used_at` on a successful match.
 * @param {{headers?:{authorization?:string, Authorization?:string}}} req
 * @returns {object|null}
 */
function findUserByBearer(req) {
  const plain = extractBearer(req);
  if (!plain) return null;
  if (!plain.startsWith(TOKEN_PREFIX)) return null;
  const h = hashToken(plain);
  const tok = db.prepare(`
    SELECT t.id AS token_id, t.expires_at, u.id, u.username, u.display_name, u.affiliation, u.karma
    FROM api_tokens t
    JOIN users u ON u.id = t.user_id
    WHERE t.token_hash = ?
  `).get(h);
  if (!tok) return null;
  if (tok.expires_at) {
    // expires_at is stored as ISO-8601 / SQLite DATETIME; parse permissively.
    const exp = Date.parse(tok.expires_at + (tok.expires_at.endsWith('Z') ? '' : 'Z'));
    if (Number.isFinite(exp) && exp <= Date.now()) return null;
  }
  // Best-effort touch — failures here shouldn't break auth.
  try {
    db.prepare('UPDATE api_tokens SET last_used_at = CURRENT_TIMESTAMP WHERE id = ?').run(tok.token_id);
  } catch (_e) { /* ignore */ }
  return {
    id: tok.id,
    username: tok.username,
    display_name: tok.display_name,
    affiliation: tok.affiliation,
    karma: tok.karma,
    _api_token_id: tok.token_id,
  };
}

module.exports = {
  generateToken,
  hashToken,
  findUserByBearer,
  extractBearer,
  TOKEN_PREFIX,
};
