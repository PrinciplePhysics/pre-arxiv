// Account-level routes for the logged-in user:
//   GET  /me/export                   — JSON dump of everything we hold
//   GET  /me/delete-account           — confirmation page (with live counts)
//   POST /me/delete-account           — anonymise the account in a single tx
//   GET  /me/webhooks                 — list / create webhook subscriptions
//   POST /me/webhooks                 — create one (secret shown once)
//   POST /me/webhooks/:id/delete      — revoke
//   POST /me/webhooks/:id/ping        — manually fire a synthetic ping
//
// The webhooks routes silently no-op if lib/webhooks.js is missing (sandbox
// trees). The export and delete routes only require the standard schema.

const { db } = require('../db');
const { requireAuth, verifyPassword } = (() => {
  const a = require('../lib/auth');
  return { requireAuth: a.requireAuth, verifyPassword: a.verifyPassword };
})();

/** @type {any} */
let webhooks = null;
try { webhooks = require('../lib/webhooks'); } catch (_e) { /* optional module */ }

/**
 * @typedef {object} MeAccountDeps
 * @property {(req:any, type:'ok'|'error', msg:string) => void} flash
 * @property {(req:any, action:string, type:string|null, id:number|null, detail:string|null) => void} [auditLog]
 * @property {boolean} [isProd]
 */

/**
 * Build a JSON-safe export of every user-owned record. Whitelisted column
 * lists — adding a new schema column won't accidentally leak through.
 * @param {number} userId
 * @returns {object|null} null iff no such user
 */
function buildUserExport(userId) {
  const u = db.prepare(`
    SELECT id, username, email, display_name, affiliation, bio, karma,
           is_admin, email_verified, totp_enabled, orcid, created_at
    FROM users WHERE id = ?
  `).get(userId);
  if (!u) return null;

  const manuscripts = db.prepare(`
    SELECT id, arxiv_like_id, doi, title, abstract, authors, category,
           pdf_path, external_url,
           conductor_type, conductor_ai_model, conductor_ai_model_public,
           conductor_human, conductor_human_public, conductor_role,
           conductor_notes, agent_framework,
           has_auditor, auditor_name, auditor_affiliation, auditor_role,
           auditor_statement, auditor_orcid,
           view_count, score, comment_count,
           withdrawn, withdrawn_reason, withdrawn_at,
           created_at, updated_at
    FROM manuscripts WHERE submitter_id = ?
    ORDER BY created_at ASC
  `).all(userId);

  const comments = db.prepare(`
    SELECT id, manuscript_id, parent_id, content, score, created_at
    FROM comments WHERE author_id = ?
    ORDER BY created_at ASC
  `).all(userId);

  const votes = db.prepare(`
    SELECT target_type, target_id, value, created_at
    FROM votes WHERE user_id = ?
    ORDER BY created_at ASC
  `).all(userId);

  const flagsRows = db.prepare(`
    SELECT id, target_type, target_id, reason, resolved, resolved_at,
           resolution_note, created_at
    FROM flag_reports WHERE reporter_id = ?
    ORDER BY created_at ASC
  `).all(userId);

  const api_tokens = db.prepare(`
    SELECT id, name, last_used_at, created_at, expires_at
    FROM api_tokens WHERE user_id = ?
    ORDER BY created_at ASC
  `).all(userId);

  let webhooksRows = [];
  try {
    webhooksRows = db.prepare(`
      SELECT id, url, events, active, description, failure_count,
             last_attempt_at, last_status, created_at
      FROM webhooks WHERE user_id = ?
      ORDER BY created_at ASC
    `).all(userId);
  } catch (_e) { /* optional table */ }

  let notifications = [];
  try {
    notifications = db.prepare(`
      SELECT id, kind, actor_id, manuscript_id, comment_id, seen, created_at
      FROM notifications WHERE user_id = ?
      ORDER BY created_at ASC
    `).all(userId);
  } catch (_e) { /* optional table */ }

  let audit_log_rows = [];
  try {
    audit_log_rows = db.prepare(`
      SELECT id, actor_user_id, action, target_type, target_id, detail, created_at
      FROM audit_log
      WHERE actor_user_id = ?
         OR (target_type = 'user' AND target_id = ?)
      ORDER BY created_at ASC
    `).all(userId, userId);
  } catch (_e) { /* optional table */ }

  return {
    export_meta: {
      generated_at: new Date().toISOString(),
      schema:       'prexiv-user-export-v1',
      note:         'Password hash and TOTP secret are deliberately excluded. ' +
                    'API token plaintext is not retrievable (we hash on create). ' +
                    'Webhook secrets are not exported.',
    },
    user:          u,
    manuscripts,
    comments,
    votes,
    flags:         flagsRows,
    api_tokens,
    webhooks:      webhooksRows,
    notifications,
    audit_log:     audit_log_rows,
  };
}

/**
 * Build a download filename for a user's exported data.
 * @param {string|null|undefined} username
 * @returns {string}
 */
function exportFilename(username) {
  const date = new Date().toISOString().slice(0, 10);
  const safe = String(username || 'user').replace(/[^a-zA-Z0-9_-]/g, '_').slice(0, 64);
  return `prexiv-export-${safe}-${date}.json`;
}

/**
 * Anonymise a user account in a single transaction:
 *   - withdraw all live manuscripts
 *   - delete all api_tokens
 *   - delete all webhooks (if table exists)
 *   - rename user to deleted_<id>, blank email/display/bio/orcid, clear 2FA
 * @param {number} userId
 * @param {string} password
 * @param {string|null} ip
 * @returns {{ok:true}|{ok:false, error:string}}
 */
function anonymizeUser(userId, password, ip) {
  const u = /** @type {{id:number, username:string, password_hash:string}|undefined} */ (
    db.prepare('SELECT id, username, password_hash FROM users WHERE id = ?').get(userId)
  );
  if (!u) return { ok: false, error: 'User not found.' };
  if (!u.password_hash || !verifyPassword(password, u.password_hash)) {
    return { ok: false, error: 'Password does not match.' };
  }

  const tx = db.transaction((/** @type {number} */ id) => {
    db.prepare(`
      UPDATE manuscripts
         SET withdrawn = 1,
             withdrawn_reason = ?,
             withdrawn_at = CURRENT_TIMESTAMP
       WHERE submitter_id = ? AND withdrawn = 0
    `).run('User account deleted.', id);

    db.prepare('DELETE FROM api_tokens WHERE user_id = ?').run(id);
    try { db.prepare('DELETE FROM webhooks WHERE user_id = ?').run(id); }
    catch (_e) { /* optional */ }

    db.prepare(`
      UPDATE users SET
        username = ?,
        email = ?,
        display_name = NULL,
        affiliation = NULL,
        bio = NULL,
        password_hash = '',
        email_verify_token = NULL,
        email_verify_expires = NULL,
        password_reset_token = NULL,
        password_reset_expires = NULL,
        totp_secret = NULL,
        totp_enabled = 0,
        orcid = NULL
      WHERE id = ?
    `).run(`deleted_${id}`, `deleted_${id}@deleted.invalid`, id);
  });

  tx(userId);

  try {
    db.prepare(
      'INSERT INTO audit_log (actor_user_id, action, target_type, target_id, detail, ip) VALUES (?, ?, ?, ?, ?, ?)'
    ).run(userId, 'account_deleted', 'user', userId, JSON.stringify({ via: 'self' }), ip || null);
  } catch (_e) { /* ignore */ }

  return { ok: true };
}

/**
 * Register the /me/export, /me/delete-account, and /me/webhooks routes.
 * @param {import('express').Application} app
 * @param {MeAccountDeps} deps
 */
function register(app, deps) {
  const { flash } = deps;
  const audit = deps.auditLog || ((/** @type {any} */ _r, /** @type {any} */ _a, /** @type {any} */ _t, /** @type {any} */ _i, /** @type {any} */ _d) => {});

  app.locals.buildUserExport = buildUserExport;
  app.locals.exportFilename = exportFilename;
  app.locals.anonymizeUser = anonymizeUser;

  // ─── /me/export ───────────────────────────────────────────────────────────
  app.get('/me/export', requireAuth, (req, res) => {
    const data = buildUserExport(req.user.id);
    if (!data) return res.status(404).render('error', { code: 404, msg: 'No such user.' });
    const fname = exportFilename(req.user.username);
    res.setHeader('Content-Type', 'application/json; charset=utf-8');
    res.setHeader('Content-Disposition', `attachment; filename="${fname}"`);
    res.setHeader('Cache-Control', 'no-store');
    res.send(JSON.stringify(data, null, 2));
    audit(req, 'data_export', 'user', req.user.id, null);
  });

  // ─── /me/delete-account ───────────────────────────────────────────────────
  app.get('/me/delete-account', requireAuth, (req, res) => {
    const counts = {
      live_manuscripts:      /** @type {{n:number}} */ (db.prepare('SELECT COUNT(*) AS n FROM manuscripts WHERE submitter_id = ? AND withdrawn = 0').get(req.user.id)).n,
      withdrawn_manuscripts: /** @type {{n:number}} */ (db.prepare('SELECT COUNT(*) AS n FROM manuscripts WHERE submitter_id = ? AND withdrawn = 1').get(req.user.id)).n,
      comments:              /** @type {{n:number}} */ (db.prepare('SELECT COUNT(*) AS n FROM comments WHERE author_id = ?').get(req.user.id)).n,
      votes:                 /** @type {{n:number}} */ (db.prepare('SELECT COUNT(*) AS n FROM votes WHERE user_id = ?').get(req.user.id)).n,
    };
    res.render('me_delete_account', { counts, errors: [] });
  });

  app.post('/me/delete-account', requireAuth, (req, res) => {
    const password = req.body.password || '';
    const counts = {
      live_manuscripts:      /** @type {{n:number}} */ (db.prepare('SELECT COUNT(*) AS n FROM manuscripts WHERE submitter_id = ? AND withdrawn = 0').get(req.user.id)).n,
      withdrawn_manuscripts: /** @type {{n:number}} */ (db.prepare('SELECT COUNT(*) AS n FROM manuscripts WHERE submitter_id = ? AND withdrawn = 1').get(req.user.id)).n,
      comments:              /** @type {{n:number}} */ (db.prepare('SELECT COUNT(*) AS n FROM comments WHERE author_id = ?').get(req.user.id)).n,
      votes:                 /** @type {{n:number}} */ (db.prepare('SELECT COUNT(*) AS n FROM votes WHERE user_id = ?').get(req.user.id)).n,
    };
    if (!password) {
      return res.status(400).render('me_delete_account', { counts, errors: ['Password is required.'] });
    }
    if (!req.body.confirm) {
      return res.status(400).render('me_delete_account', { counts, errors: ['Please tick the confirmation box.'] });
    }
    let result;
    try { result = anonymizeUser(req.user.id, password, req.ip); }
    catch (e) {
      console.warn('[delete-account] failed:', (/** @type {Error} */ (e)).message || e);
      return res.status(500).render('me_delete_account', { counts, errors: ['Something went wrong. Try again.'] });
    }
    if (!result.ok) {
      // @ts-expect-error result.error exists in the !ok branch
      return res.status(400).render('me_delete_account', { counts, errors: [result.error] });
    }
    req.session.destroy(() => res.redirect('/'));
  });

  // ─── /me/webhooks (best-effort — silently absent if lib/webhooks missing) ─
  if (!webhooks) return;

  /**
   * @param {number} userId
   * @returns {any[]}
   */
  function fetchUserWebhooks(userId) {
    return db.prepare(
      'SELECT id, url, events, active, description, failure_count, last_attempt_at, last_status, created_at FROM webhooks WHERE user_id = ? ORDER BY created_at DESC'
    ).all(userId);
  }

  /**
   * @param {unknown} input
   * @returns {string[]}
   */
  function parseEventsList(input) {
    if (input == null) return [];
    let arr;
    if (Array.isArray(input)) arr = input;
    else if (typeof input === 'string') arr = [input];
    else arr = [];
    return arr
      .map((/** @type {any} */ s) => String(s || '').trim())
      .filter(Boolean)
      .filter((/** @type {string} */ s) => webhooks.SUPPORTED_EVENTS.includes(s));
  }
  app.locals.parseEventsList = parseEventsList;
  app.locals.fetchUserWebhooks = fetchUserWebhooks;

  app.get('/me/webhooks', requireAuth, (req, res) => {
    const list = fetchUserWebhooks(req.user.id);
    const justCreated = req.session.justCreatedWebhook || null;
    delete req.session.justCreatedWebhook;
    res.render('me_webhooks', {
      webhooks: list,
      justCreated,
      supportedEvents: webhooks.SUPPORTED_EVENTS,
      errors: [],
    });
  });

  app.post('/me/webhooks', requireAuth, async (req, res) => {
    const url = (req.body.url || '').trim().slice(0, 500);
    const description = (req.body.description || '').trim().slice(0, 200) || null;
    const events = parseEventsList(req.body.events);
    /** @type {string[]} */
    const errors = [];
    const urlErr = await webhooks.validateWebhookUrl(url, { requireHttps: deps.isProd });
    if (urlErr) errors.push(urlErr);
    if (!events.length) errors.push('Pick at least one event to subscribe to.');
    if (errors.length) {
      return res.status(400).render('me_webhooks', {
        webhooks: fetchUserWebhooks(req.user.id),
        justCreated: null,
        supportedEvents: webhooks.SUPPORTED_EVENTS,
        errors,
      });
    }
    const secret = webhooks.randomSecret();
    const r = db.prepare(`
      INSERT INTO webhooks (user_id, url, secret, events, active, description)
      VALUES (?, ?, ?, ?, 1, ?)
    `)
      .run(req.user.id, url, secret, JSON.stringify(events), description);
    req.session.justCreatedWebhook = { id: r.lastInsertRowid, url, events, secret };
    audit(req, 'webhook_create', 'webhook', /** @type {number} */ (r.lastInsertRowid), JSON.stringify({ url, events }));
    flash(req, 'ok', 'Webhook created. Copy the secret now — it will not be shown again.');
    res.redirect('/me/webhooks');
  });

  app.post('/me/webhooks/:id/delete', requireAuth, (req, res) => {
    const id = parseInt(req.params.id, 10);
    if (id) {
      const w = /** @type {{id:number, user_id:number}|undefined} */ (
        db.prepare('SELECT id, user_id FROM webhooks WHERE id = ?').get(id)
      );
      if (w && w.user_id === req.user.id) {
        db.prepare('DELETE FROM webhooks WHERE id = ?').run(id);
        audit(req, 'webhook_delete', 'webhook', id, null);
        flash(req, 'ok', 'Webhook deleted.');
      } else {
        flash(req, 'error', 'Webhook not found.');
      }
    }
    res.redirect('/me/webhooks');
  });

  app.post('/me/webhooks/:id/ping', requireAuth, async (req, res) => {
    const id = parseInt(req.params.id, 10);
    if (!id) { flash(req, 'error', 'Bad webhook id.'); return res.redirect('/me/webhooks'); }
    const w = /** @type {{id:number, user_id:number}|undefined} */ (
      db.prepare('SELECT id, user_id FROM webhooks WHERE id = ?').get(id)
    );
    if (!w || w.user_id !== req.user.id) { flash(req, 'error', 'Webhook not found.'); return res.redirect('/me/webhooks'); }
    let result;
    try { result = await webhooks.pingOne(id); }
    catch (_e) { result = { ok: false, status: 0 }; }
    if (result.ok) {
      flash(req, 'ok', 'Ping delivered (HTTP ' + (result.status || 'OK') + ').');
    } else {
      flash(req, 'error', 'Ping failed (HTTP ' + (result.status || 'no response') + '). Failure count incremented.');
    }
    res.redirect('/me/webhooks');
  });
}

module.exports = { register, buildUserExport, exportFilename, anonymizeUser };
