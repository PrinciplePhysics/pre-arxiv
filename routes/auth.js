// Authentication routes: login (with optional 2FA), register, email verify,
// password reset, logout. The 2FA setup pages (/me/2fa) live in
// routes/profile.js with the rest of the /me/* surface; only the /login/2fa
// challenge step lives here, since it's part of the login flow.

const crypto = require('crypto');
const { db } = require('../db');
const { hashPassword, verifyPassword, requireAuth, validateUsername, isPasswordPwned } = require('../lib/auth');
const { sendMail, absoluteUrl } = require('../lib/email');
const { verifyTotp } = require('../lib/totp');

/**
 * @typedef {object} AuthDeps
 * @property {import('express').RequestHandler} authLimiter
 * @property {(req:any, type:'ok'|'error', msg:string) => void} flash
 * @property {(req:any, res:any, userId:number, target:string, msg:string|null, next:any) => void} finishLoggedInRedirect
 * @property {(req:any, userId:number, cb:(err?:Error)=>void) => void} establishLoginSession
 */

/**
 * Register the auth routes:
 *   GET  /login          — login form
 *   POST /login          — username+password (then 2FA challenge if enabled)
 *   GET  /login/2fa      — 2FA challenge page
 *   POST /login/2fa      — verify TOTP and finish login
 *   GET  /register       — registration form (with math CAPTCHA)
 *   POST /register       — create account, send verify link, log user in (still unverified)
 *   GET  /verify-pending — interstitial page that shows the dev-mode link
 *   GET  /verify/:token  — consume an email-verify token
 *   POST /verify/resend  — generate a fresh verify link
 *   POST /logout         — destroy the session
 *   GET  /forgot         — password-reset request form
 *   POST /forgot         — issue a reset token (always succeeds visually, even on unknown email)
 *   GET  /reset/:token   — reset form
 *   POST /reset/:token   — set a new password
 *
 * @param {import('express').Application} app
 * @param {AuthDeps} deps
 */
function register(app, deps) {
  const { authLimiter, flash, finishLoggedInRedirect, establishLoginSession } = deps;

  // ─── login ────────────────────────────────────────────────────────────────
  app.get('/login', (req, res) => {
    if (req.user) return res.redirect('/');
    res.render('login', { values: {}, errors: [], next: req.query.next || '/' });
  });

  app.post('/login', authLimiter, (req, res, nextFn) => {
    const username = (req.body.username || '').trim();
    const password = req.body.password || '';
    const next = (req.body.next && /^\/[^/]/.test(req.body.next)) ? req.body.next : '/';
    /** @type {string[]} */
    const errors = [];
    if (!username || !password) errors.push('Username and password are required.');
    let user;
    if (!errors.length) {
      user = /** @type {{id:number, password_hash:string, totp_enabled:number}|undefined} */ (
        db.prepare('SELECT id, password_hash, totp_enabled FROM users WHERE username = ? OR email = ?').get(username, username)
      );
      if (!user || !verifyPassword(password, user.password_hash)) {
        errors.push('Invalid username or password.');
      }
    }
    if (errors.length) return res.render('login', { values: { username }, errors, next });

    // If 2FA is enabled, defer the actual login until /login/2fa verifies the
    // code. Stash the user_id + intended next in the session — note that we
    // explicitly do NOT set req.session.userId here.
    if (user.totp_enabled) {
      req.session.awaiting_2fa = { user_id: user.id, next, ts: Date.now() };
      return res.redirect('/login/2fa');
    }
    finishLoggedInRedirect(req, res, user.id, next, 'Welcome back.', nextFn);
  });

  app.get('/login/2fa', (req, res) => {
    if (req.user) return res.redirect('/');
    const a = req.session.awaiting_2fa;
    if (!a || !a.user_id) return res.redirect('/login');
    if (Date.now() - (a.ts || 0) > 5 * 60 * 1000) {
      delete req.session.awaiting_2fa;
      return res.redirect('/login');
    }
    res.render('login_2fa', { errors: [], next: a.next || '/' });
  });

  app.post('/login/2fa', authLimiter, (req, res, nextFn) => {
    const a = req.session.awaiting_2fa;
    if (!a || !a.user_id) return res.redirect('/login');
    if (Date.now() - (a.ts || 0) > 5 * 60 * 1000) {
      delete req.session.awaiting_2fa;
      return res.redirect('/login');
    }
    const code = String((req.body.code || '')).trim();
    const u = /** @type {{id:number, totp_secret:string, totp_enabled:number}|undefined} */ (
      db.prepare('SELECT id, totp_secret, totp_enabled FROM users WHERE id = ?').get(a.user_id)
    );
    if (!u || !u.totp_enabled || !u.totp_secret) {
      delete req.session.awaiting_2fa;
      return res.redirect('/login');
    }
    if (!verifyTotp(u.totp_secret, code, 1)) {
      return res.render('login_2fa', { errors: ['Invalid 2FA code.'], next: a.next || '/' });
    }
    const target = a.next || '/';
    delete req.session.awaiting_2fa;
    finishLoggedInRedirect(req, res, u.id, target, 'Welcome back.', nextFn);
  });

  // ─── register + CAPTCHA ───────────────────────────────────────────────────
  function freshCaptcha(/** @type {any} */ req) {
    const a = 1 + Math.floor(Math.random() * 9);
    const b = 1 + Math.floor(Math.random() * 9);
    const op = Math.random() < 0.5 ? '+' : (a >= b ? '-' : '+');
    const answer = op === '+' ? a + b : a - b;
    req.session.captcha = { a, b, op, answer, issuedAt: Date.now() };
    return req.session.captcha;
  }
  function verifyCaptcha(/** @type {any} */ req) {
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

  app.post('/register', authLimiter, async (req, res, nextFn) => {
    const username     = (req.body.username || '').trim();
    const email        = (req.body.email || '').trim().toLowerCase();
    const password     = req.body.password || '';
    const display_name = (req.body.display_name || '').trim() || null;
    const affiliation  = (req.body.affiliation || '').trim() || null;
    /** @type {string[]} */
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
    if (!errors.length && password) {
      if (await isPasswordPwned(password)) {
        errors.push('This password has appeared in known data breaches. Please pick a different one.');
      }
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

    const verifyLink = absoluteUrl(req, '/verify/' + verifyToken);
    try {
      const result = await sendMail({
        to: email,
        subject: 'Verify your email for PreXiv',
        text:
`Welcome to PreXiv.

Please confirm your email address by visiting:

  ${verifyLink}

This link expires in 3 days. If you didn't sign up, you can ignore this email.`,
      });

      establishLoginSession(req, /** @type {number} */ (r.lastInsertRowid), (err) => {
        if (err) return nextFn(err);
        req.session.lastVerifyLink = result.devMode ? verifyLink : null;
        res.redirect('/verify-pending');
      });
    } catch (e) {
      nextFn(/** @type {Error} */ (e));
    }
  });

  app.get('/verify-pending', (req, res) => {
    if (!req.user) return res.redirect('/');
    const u = /** @type {{email:string, email_verified:number}|undefined} */ (
      db.prepare('SELECT email, email_verified FROM users WHERE id = ?').get(req.user.id)
    );
    if (u && u.email_verified) return res.redirect('/');
    const link = req.session.lastVerifyLink || null;
    delete req.session.lastVerifyLink;
    res.render('verify_pending', { email: u ? u.email : '', devLink: link });
  });

  app.get('/verify/:token', (req, res, nextFn) => {
    const tok = req.params.token;
    const u = /** @type {{id:number, email_verify_expires:number|null}|undefined} */ (
      db.prepare('SELECT id, email_verify_expires FROM users WHERE email_verify_token = ?').get(tok)
    );
    if (!u) {
      return res.status(400).render('error', { code: 400, msg: 'Verification link is invalid or has already been used.' });
    }
    if (u.email_verify_expires && u.email_verify_expires < Date.now()) {
      return res.status(400).render('error', { code: 400, msg: 'Verification link has expired. Request a new one.' });
    }
    db.prepare(`UPDATE users SET email_verified = 1, email_verify_token = NULL, email_verify_expires = NULL WHERE id = ?`).run(u.id);
    if (!req.user) {
      return finishLoggedInRedirect(req, res, u.id, '/', 'Email verified. You can now submit manuscripts.', nextFn);
    }
    flash(req, 'ok', 'Email verified. You can now submit manuscripts.');
    res.redirect('/');
  });

  app.post('/verify/resend', authLimiter, requireAuth, async (req, res) => {
    const u = /** @type {{id:number, email:string, email_verified:number}|undefined} */ (
      db.prepare('SELECT id, email, email_verified FROM users WHERE id = ?').get(req.user.id)
    );
    if (!u) return res.redirect('/');
    if (u.email_verified) { flash(req, 'ok', 'Already verified.'); return res.redirect('/'); }
    const verifyToken = crypto.randomBytes(24).toString('base64url');
    const verifyExpires = Date.now() + 1000 * 60 * 60 * 24 * 3;
    db.prepare('UPDATE users SET email_verify_token = ?, email_verify_expires = ? WHERE id = ?')
      .run(verifyToken, verifyExpires, u.id);
    const link = absoluteUrl(req, '/verify/' + verifyToken);
    const result = await sendMail({
      to: u.email,
      subject: 'New PreXiv verification link',
      text: `Use this link to verify your email:\n\n  ${link}\n\nThis one expires in 3 days.`,
    });
    req.session.lastVerifyLink = result.devMode ? link : null;
    res.redirect('/verify-pending');
  });

  app.post('/logout', (req, res) => {
    req.session.destroy(() => res.redirect('/'));
  });

  // ─── password reset ───────────────────────────────────────────────────────
  app.get('/forgot', (req, res) => {
    res.render('forgot', { values: {}, errors: [], devLink: req.session.lastResetLink || null });
    delete req.session.lastResetLink;
  });

  app.post('/forgot', authLimiter, async (req, res) => {
    const email = (req.body.email || '').trim().toLowerCase();
    if (!email) return res.render('forgot', { values: {}, errors: ['Email is required.'], devLink: null });

    const u = /** @type {{id:number}|undefined} */ (
      db.prepare('SELECT id FROM users WHERE email = ?').get(email)
    );
    // Generic response either way to avoid email enumeration
    /** @type {string|null} */
    let devLink = null;
    if (u) {
      const token = crypto.randomBytes(24).toString('base64url');
      const expires = Date.now() + 1000 * 60 * 60; // 1 hour
      db.prepare('UPDATE users SET password_reset_token = ?, password_reset_expires = ? WHERE id = ?')
        .run(token, expires, u.id);
      const link = absoluteUrl(req, '/reset/' + token);
      const result = await sendMail({
        to: email,
        subject: 'PreXiv password reset',
        text: `A password reset was requested for this email.\n\nIf it was you, follow this link within 1 hour:\n\n  ${link}\n\nIf it wasn't, ignore this message — nothing has changed.`,
      });
      if (result.devMode) devLink = link;
    }
    req.session.lastResetLink = devLink;
    flash(req, 'ok', 'If an account exists for that email, a reset link has been sent.');
    res.redirect('/forgot');
  });

  app.get('/reset/:token', (req, res) => {
    const u = /** @type {{id:number, password_reset_expires:number|null}|undefined} */ (
      db.prepare('SELECT id, password_reset_expires FROM users WHERE password_reset_token = ?').get(req.params.token)
    );
    if (!u || (u.password_reset_expires && u.password_reset_expires < Date.now())) {
      return res.status(400).render('error', { code: 400, msg: 'Reset link is invalid or has expired.' });
    }
    res.render('reset', { token: req.params.token, errors: [] });
  });

  app.post('/reset/:token', authLimiter, async (req, res) => {
    const password = req.body.password || '';
    const password2 = req.body.password2 || '';
    /** @type {string[]} */
    const errors = [];
    if (!password || password.length < 8) errors.push('Password must be ≥ 8 characters.');
    if (password !== password2)             errors.push('Passwords do not match.');
    const u = /** @type {{id:number, password_reset_expires:number|null}|undefined} */ (
      db.prepare('SELECT id, password_reset_expires FROM users WHERE password_reset_token = ?').get(req.params.token)
    );
    if (!u || (u.password_reset_expires && u.password_reset_expires < Date.now())) {
      return res.status(400).render('error', { code: 400, msg: 'Reset link is invalid or has expired.' });
    }
    if (!errors.length && password) {
      if (await isPasswordPwned(password)) {
        errors.push('This password has appeared in known data breaches. Please pick a different one.');
      }
    }
    if (errors.length) return res.render('reset', { token: req.params.token, errors });
    db.prepare('UPDATE users SET password_hash = ?, password_reset_token = NULL, password_reset_expires = NULL WHERE id = ?')
      .run(hashPassword(password), u.id);
    flash(req, 'ok', 'Password updated. Log in with your new password.');
    res.redirect('/login');
  });
}

module.exports = { register };
