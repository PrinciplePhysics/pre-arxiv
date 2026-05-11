// Voting and flagging routes (web flow only — the JSON twins live in
// lib/api.js under /api/v1/votes/* and /api/v1/flags/*).
//
// Both routes return JSON when the client asks for it via Accept and HTML
// otherwise; this lets the progressive-enhancement vote button on the home
// page (public/js/app.js) update the score in-place without a redirect.

const { db } = require('../db');
const { requireAuth } = require('../lib/auth');

/**
 * @typedef {object} VotesDeps
 * @property {import('express').RequestHandler} voteLimiter
 * @property {(req:any, type:'ok'|'error', msg:string) => void} flash
 * @property {(userId:number, type:'manuscript'|'comment', targetId:number, value:1|-1) => {score:number}|{withdrawn:true}|null} applyVote
 * @property {(event:string, payload:any) => void} [safeEmit]
 */

/**
 * Register the vote / flag routes:
 *   POST /vote/:type/:id     — toggle a vote on a manuscript or comment
 *   POST /flag/:type/:id     — file a flag report (with reason, ≥ 5 chars)
 *
 * @param {import('express').Application} app
 * @param {VotesDeps} deps
 */
function register(app, deps) {
  const { voteLimiter, flash, applyVote } = deps;
  const emit = deps.safeEmit || ((/** @type {string} */ _e, /** @type {any} */ _p) => {});

  /**
   * Render a vote error in the right format (JSON for fetch clients, HTML otherwise).
   * @param {import('express').Request} req
   * @param {import('express').Response} res
   * @param {number} code
   * @param {string} msg
   */
  function voteError(req, res, code, msg) {
    if (req.headers.accept && req.headers.accept.includes('application/json')) {
      return res.status(code).json({ error: msg });
    }
    return res.status(code).render('error', { code, msg });
  }

  app.post('/vote/:type/:id', voteLimiter, requireAuth, (req, res) => {
    const type = /** @type {'manuscript'|'comment'} */ (req.params.type);
    if (type !== 'manuscript' && type !== 'comment') return res.status(400).json({ error: 'bad type' });
    const id = parseInt(req.params.id, 10);
    if (!Number.isInteger(id) || id <= 0) return voteError(req, res, 400, 'Bad vote target.');
    const value = /** @type {1|-1} */ (parseInt(req.body.value, 10));
    if (![1, -1].includes(value)) return res.status(400).json({ error: 'bad value' });
    const result = applyVote(req.user.id, type, id, value);
    if (!result) return voteError(req, res, 404, 'Vote target not found.');
    if ('withdrawn' in result && result.withdrawn) return voteError(req, res, 409, 'Withdrawn manuscripts cannot be voted on.');
    const myVote = (
      /** @type {{value:number}|undefined} */ (
        db.prepare('SELECT value FROM votes WHERE user_id = ? AND target_type = ? AND target_id = ?')
          .get(req.user.id, type, id)
      ) || {}
    ).value || 0;
    // Webhook fan-out.
    emit('vote.cast', {
      target_type: type,
      target_id: id,
      voter_id: req.user.id,
      voter_username: req.user.username || null,
      value: myVote,
      // @ts-expect-error result has .score in this branch
      score: result.score,
      ts: new Date().toISOString(),
    });
    if (req.headers.accept && req.headers.accept.includes('application/json')) {
      // @ts-expect-error result has .score branch here
      return res.json({ score: result.score, myVote });
    }
    // Compute the right destination from the target itself — Referer is
    // unreliable (Referrer-Policy can strip it).
    let dest = '/';
    if (type === 'manuscript') {
      const m = /** @type {{arxiv_like_id:string}|undefined} */ (
        db.prepare('SELECT arxiv_like_id FROM manuscripts WHERE id = ?').get(id)
      );
      if (m) dest = '/m/' + encodeURIComponent(m.arxiv_like_id);
    } else {
      const row = /** @type {{arxiv_like_id:string}|undefined} */ (
        db.prepare(`
          SELECT m.arxiv_like_id FROM comments c
          JOIN manuscripts m ON m.id = c.manuscript_id
          WHERE c.id = ?
        `).get(id)
      );
      if (row) dest = '/m/' + encodeURIComponent(row.arxiv_like_id) + '#c' + id;
    }
    // Honor Referer ONLY if it's same-origin and points at the same manuscript.
    const ref = req.get('Referer');
    if (ref) {
      try {
        const u = new URL(ref);
        const sameOrigin = u.host === req.get('host');
        if (sameOrigin && u.pathname.startsWith('/m/')) dest = u.pathname + u.search + u.hash;
      } catch { /* fall through to computed dest */ }
    }
    res.redirect(dest);
  });

  app.post('/flag/:type/:id', requireAuth, (req, res) => {
    const type = req.params.type;
    if (type !== 'manuscript' && type !== 'comment') return res.status(400).render('error', { code: 400, msg: 'Bad flag target.' });
    const targetId = parseInt(req.params.id, 10);
    if (!targetId) return res.status(400).render('error', { code: 400, msg: 'Bad target id.' });
    let dest = '/';
    if (type === 'manuscript') {
      const m = /** @type {{arxiv_like_id:string}|undefined} */ (
        db.prepare('SELECT arxiv_like_id FROM manuscripts WHERE id = ?').get(targetId)
      );
      if (!m) return res.status(404).render('error', { code: 404, msg: 'Flag target not found.' });
      dest = '/m/' + encodeURIComponent(m.arxiv_like_id);
    } else {
      const row = /** @type {{arxiv_like_id:string}|undefined} */ (
        db.prepare(`
          SELECT m.arxiv_like_id FROM comments c
          JOIN manuscripts m ON m.id = c.manuscript_id
          WHERE c.id = ?
        `).get(targetId)
      );
      if (!row) return res.status(404).render('error', { code: 404, msg: 'Flag target not found.' });
      dest = '/m/' + encodeURIComponent(row.arxiv_like_id) + '#c' + targetId;
    }
    const reason = (req.body.reason || '').trim().slice(0, 1000);
    if (!reason || reason.length < 5) {
      flash(req, 'error', 'Please give a brief reason for the flag (≥ 5 characters).');
      return res.redirect(dest);
    }
    let didCreate = false;
    try {
      db.prepare('INSERT INTO flag_reports (target_type, target_id, reporter_id, reason) VALUES (?, ?, ?, ?)')
        .run(type, targetId, req.user.id, reason);
      flash(req, 'ok', 'Thanks — flagged for review.');
      didCreate = true;
    } catch (e) {
      const msg = (/** @type {Error} */ (e)).message;
      if (/UNIQUE/.test(msg)) {
        const existing = /** @type {{id:number, resolved:number}|undefined} */ (
          db.prepare(`
            SELECT id, resolved FROM flag_reports
            WHERE target_type = ? AND target_id = ? AND reporter_id = ?
          `)
            .get(type, targetId, req.user.id)
        );
        if (existing && existing.resolved) {
          db.prepare(`
            UPDATE flag_reports
            SET reason = ?, resolved = 0, resolved_by_id = NULL, resolved_at = NULL,
                resolution_note = NULL, created_at = CURRENT_TIMESTAMP
            WHERE id = ?
          `).run(reason, existing.id);
          flash(req, 'ok', 'Thanks — reopened for review.');
          didCreate = true;
        } else {
          flash(req, 'ok', 'You have already flagged this. The moderators will see it.');
        }
      } else throw e;
    }
    // Webhook fan-out — only on a new or reopened flag.
    if (didCreate) {
      emit('flag.created', {
        target_type: type, target_id: targetId,
        reporter_id: req.user.id,
        reporter_username: req.user.username || null,
        reason,
        ts: new Date().toISOString(),
      });
    }
    res.redirect(dest);
  });
}

module.exports = { register };
