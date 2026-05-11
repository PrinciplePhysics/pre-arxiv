// User profile + per-user account routes:
//   GET  /u/:username              — public profile (submissions, conducted-as, audited count, follow stats)
//   GET  /me/edit                  — profile edit form
//   POST /me/edit                  — save display_name / affiliation / bio / orcid
//   GET  /me/tokens                — list and create personal API tokens (web UI)
//   POST /me/tokens                — mint a new token (with optional expiry)
//   POST /me/tokens/:id/revoke     — revoke a token
//   GET  /me/2fa                   — TOTP setup / status page
//   POST /me/2fa                   — issue a fresh TOTP secret (pending-only)
//   POST /me/2fa/verify            — confirm the code, enable 2FA
//   POST /me/2fa/disable           — verify code then disable

const { db } = require('../db');
const { requireAuth } = require('../lib/auth');
const { generateToken, hashToken } = require('../lib/api-auth');
const { generateSecret, getOtpauthUrl, verifyTotp } = require('../lib/totp');

/**
 * @typedef {object} ProfileDeps
 * @property {(req:any, type:'ok'|'error', msg:string) => void} flash
 * @property {(raw:string|null|undefined) => string|null} orcidError
 * @property {(raw:string|null|undefined) => string|null} normalizeOrcid
 * @property {(req:any, action:string, type:string|null, id:number|null, detail:string|null) => void} [auditLog]
 */

/**
 * Register the user-profile + /me/* routes.
 * @param {import('express').Application} app
 * @param {ProfileDeps} deps
 */
function register(app, deps) {
  const { flash, orcidError, normalizeOrcid } = deps;
  const audit = deps.auditLog || ((/** @type {any} */ _r, /** @type {any} */ _a, /** @type {any} */ _t, /** @type {any} */ _i, /** @type {any} */ _d) => {});

  // ─── /me — friendly redirects to the canonical pages ─────────────────────
  // /me                 → my own /u/:username profile
  // /me/delete          → typo-tolerant alias for /me/delete-account
  // /me/notifications/  (with the trailing slash) is handled in routes/social.js
  app.get('/me', requireAuth, (req, res) => {
    res.redirect('/u/' + req.user.username);
  });
  app.get('/me/delete', requireAuth, (_req, res) => {
    res.redirect(301, '/me/delete-account');
  });

  // ─── /u/:username public profile ──────────────────────────────────────────
  app.get('/u/:username', (req, res) => {
    const u = /** @type {{id:number, username:string, display_name:string|null, affiliation:string|null, bio:string|null, karma:number, created_at:string}|undefined} */ (
      db.prepare('SELECT id, username, display_name, affiliation, bio, karma, created_at FROM users WHERE username = ?').get(req.params.username)
    );
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
    const auditedRow = /** @type {{n:number}} */ (
      db.prepare(`SELECT COUNT(*) AS n FROM manuscripts WHERE auditor_name LIKE ? OR auditor_name = ?`)
        .get(`%${u.display_name || u.username}%`, u.username)
    );
    const auditedCount = auditedRow.n;

    let followerCount = 0, followingCount = 0;
    try {
      followerCount  = /** @type {{n:number}} */ (db.prepare('SELECT COUNT(*) AS n FROM follows WHERE followee_id = ?').get(u.id)).n;
      followingCount = /** @type {{n:number}} */ (db.prepare('SELECT COUNT(*) AS n FROM follows WHERE follower_id = ?').get(u.id)).n;
    } catch (_e) { /* table may not exist yet */ }
    let isFollowing = false;
    let isSelf = false;
    if (req.user) {
      isSelf = (req.user.id === u.id);
      if (!isSelf) {
        try {
          const f = db.prepare('SELECT 1 AS ok FROM follows WHERE follower_id = ? AND followee_id = ?').get(req.user.id, u.id);
          isFollowing = !!f;
        } catch (_e) { /* ignore */ }
      }
    }

    res.render('user', {
      profile: u, submissions, conductedAs, auditedCount,
      followerCount, followingCount, isFollowing, isSelf,
    });
  });

  // ─── /me/edit ─────────────────────────────────────────────────────────────
  app.get('/me/edit', requireAuth, (req, res) => {
    const u = /** @type {{id:number, username:string, display_name:string|null, affiliation:string|null, bio:string|null, orcid:string|null}|undefined} */ (
      db.prepare('SELECT id, username, display_name, affiliation, bio, orcid FROM users WHERE id = ?').get(req.user.id)
    );
    if (!u) return res.redirect('/');
    res.render('me_edit', { values: u, errors: [] });
  });

  app.post('/me/edit', requireAuth, (req, res) => {
    const display_name = (req.body.display_name == null ? '' : String(req.body.display_name)).trim().slice(0, 200) || null;
    const affiliation  = (req.body.affiliation  == null ? '' : String(req.body.affiliation)).trim().slice(0, 200) || null;
    const bio          = (req.body.bio          == null ? '' : String(req.body.bio)).trim().slice(0, 2000) || null;
    const orcidRaw     = (req.body.orcid        == null ? '' : String(req.body.orcid)).trim();
    /** @type {string[]} */
    const errors = [];
    const oErr = orcidError(orcidRaw);
    if (oErr) errors.push(oErr);
    if (errors.length) {
      return res.render('me_edit', {
        values: { display_name, affiliation, bio, orcid: orcidRaw },
        errors,
      });
    }
    db.prepare(`
      UPDATE users SET display_name = ?, affiliation = ?, bio = ?, orcid = ?
      WHERE id = ?
    `)
      .run(display_name, affiliation, bio, normalizeOrcid(orcidRaw), req.user.id);
    flash(req, 'ok', 'Profile updated.');
    res.redirect('/u/' + req.user.username);
  });

  // ─── /me/tokens ───────────────────────────────────────────────────────────
  app.get('/me/tokens', requireAuth, (req, res) => {
    const tokens = db.prepare(
      'SELECT id, name, last_used_at, created_at, expires_at FROM api_tokens WHERE user_id = ? ORDER BY created_at DESC'
    )
      .all(req.user.id);
    const justCreated = req.session.justCreatedToken || null;
    delete req.session.justCreatedToken;

    const proto  = req.get('x-forwarded-proto') || (req.secure ? 'https' : 'http');
    const host   = req.get('host');
    const origin = (process.env.APP_URL || '').replace(/\/+$/, '') || (proto + '://' + host);
    const apiBase = origin + '/api/v1';

    // Build manifest if available; fall back to a minimal stub.
    /** @type {any} */
    let manifest = null;
    try {
      const { buildManifest } = require('../lib/manifest');
      if (typeof buildManifest === 'function') manifest = buildManifest(req);
    } catch (_e) { /* lib/manifest.js may be missing */ }
    if (!manifest) manifest = { operations: [], manuscriptBodySchema: { required: [], conditional_required: {}, optional: [] }, mcp: { tools: [] } };

    res.render('me_tokens', { tokens, justCreated, origin, apiBase, manifest });
  });

  app.post('/me/tokens', requireAuth, (req, res) => {
    const name = (req.body.name || '').trim().slice(0, 200) || null;
    // Custom expiry: integer days, 0 = never. Default 90 days.
    let expiresInDays = 90;
    const raw = req.body.expires_in_days;
    if (raw !== undefined && raw !== null && String(raw).trim() !== '') {
      const n = parseInt(raw, 10);
      if (!Number.isFinite(n) || n < 0 || n > 365) {
        flash(req, 'error', 'expires_in_days must be 0 (never) or 1–365.');
        return res.redirect('/me/tokens');
      }
      expiresInDays = n;
    }
    const expiresAtIso = expiresInDays === 0 ? null
      : new Date(Date.now() + expiresInDays * 86400 * 1000).toISOString().slice(0, 19).replace('T', ' ');
    const plain = generateToken();
    const r = db.prepare('INSERT INTO api_tokens (user_id, token_hash, name, expires_at) VALUES (?, ?, ?, ?)')
      .run(req.user.id, hashToken(plain), name, expiresAtIso);
    req.session.justCreatedToken = { id: r.lastInsertRowid, name, token: plain };
    flash(req, 'ok', 'API token created. Copy it now — it will not be shown again.');
    res.redirect('/me/tokens');
  });

  app.post('/me/tokens/:id/revoke', requireAuth, (req, res) => {
    const id = parseInt(req.params.id, 10);
    if (id) {
      const t = /** @type {{id:number, user_id:number}|undefined} */ (
        db.prepare('SELECT id, user_id FROM api_tokens WHERE id = ?').get(id)
      );
      if (t && t.user_id === req.user.id) {
        db.prepare('DELETE FROM api_tokens WHERE id = ?').run(id);
        audit(req, 'token_revoke', 'api_token', id, null);
        flash(req, 'ok', 'Token revoked.');
      } else {
        flash(req, 'error', 'Token not found.');
      }
    }
    res.redirect('/me/tokens');
  });

  // ─── /me/2fa setup / verify / disable ─────────────────────────────────────
  app.get('/me/2fa', requireAuth, (req, res) => {
    const u = /** @type {{totp_enabled:number}|undefined} */ (
      db.prepare('SELECT totp_enabled FROM users WHERE id = ?').get(req.user.id)
    );
    const totpEnabled = !!(u && u.totp_enabled);
    const pending = totpEnabled ? null : (req.session.pending_totp || null);
    res.render('me_2fa', { totpEnabled, pending, errors: [] });
  });

  app.post('/me/2fa', requireAuth, (req, res) => {
    const u = /** @type {{totp_enabled:number, username:string}|undefined} */ (
      db.prepare('SELECT totp_enabled, username FROM users WHERE id = ?').get(req.user.id)
    );
    if (u && u.totp_enabled) {
      flash(req, 'error', '2FA is already enabled. Disable it first to re-enroll.');
      return res.redirect('/me/2fa');
    }
    const secret = generateSecret(20);
    const label = 'PreXiv:' + (u && u.username ? u.username : 'user');
    const otpauth = getOtpauthUrl(label, secret, { issuer: 'PreXiv' });
    req.session.pending_totp = { secret, otpauth, ts: Date.now() };
    res.redirect('/me/2fa');
  });

  app.post('/me/2fa/verify', requireAuth, (req, res) => {
    const u = /** @type {{totp_enabled:number}|undefined} */ (
      db.prepare('SELECT totp_enabled FROM users WHERE id = ?').get(req.user.id)
    );
    if (u && u.totp_enabled) {
      flash(req, 'error', '2FA is already enabled.');
      return res.redirect('/me/2fa');
    }
    const pending = req.session.pending_totp;
    if (!pending || !pending.secret) {
      flash(req, 'error', 'No 2FA setup is in progress. Start over.');
      return res.redirect('/me/2fa');
    }
    if (Date.now() - (pending.ts || 0) > 15 * 60 * 1000) {
      delete req.session.pending_totp;
      flash(req, 'error', '2FA setup expired. Start over.');
      return res.redirect('/me/2fa');
    }
    const code = String(req.body.code || '').trim();
    if (!verifyTotp(pending.secret, code, 1)) {
      return res.render('me_2fa', { totpEnabled: false, pending, errors: ['Invalid code. Make sure your authenticator clock is in sync, then try again.'] });
    }
    db.prepare('UPDATE users SET totp_secret = ?, totp_enabled = 1 WHERE id = ?')
      .run(pending.secret, req.user.id);
    delete req.session.pending_totp;
    audit(req, '2fa_enable', 'user', req.user.id, null);
    flash(req, 'ok', '2FA enabled. Future logins will require a code from your authenticator.');
    res.redirect('/me/2fa');
  });

  app.post('/me/2fa/disable', requireAuth, (req, res) => {
    const u = /** @type {{totp_enabled:number, totp_secret:string}|undefined} */ (
      db.prepare('SELECT totp_enabled, totp_secret FROM users WHERE id = ?').get(req.user.id)
    );
    if (!u || !u.totp_enabled) {
      flash(req, 'error', '2FA is not enabled.');
      return res.redirect('/me/2fa');
    }
    const code = String(req.body.code || '').trim();
    if (!verifyTotp(u.totp_secret, code, 1)) {
      return res.render('me_2fa', { totpEnabled: true, pending: null, errors: ['Invalid code. 2FA was NOT disabled.'] });
    }
    db.prepare('UPDATE users SET totp_secret = NULL, totp_enabled = 0 WHERE id = ?').run(req.user.id);
    audit(req, '2fa_disable', 'user', req.user.id, null);
    flash(req, 'ok', '2FA disabled.');
    res.redirect('/me/2fa');
  });
}

module.exports = { register };
