// Social-graph routes: follow / unfollow, feed, notifications, theme cookie.
//
// All of these require an authenticated user (except the theme cookie set,
// which is permitted for unauthenticated visitors so the toggle works on the
// public landing page too).

const { db } = require('../db');
const { requireAuth } = require('../lib/auth');
const { paginate } = require('../lib/util');

/**
 * @typedef {object} SocialDeps
 * @property {(req:any, type:'ok'|'error', msg:string) => void} flash
 * @property {(userId:number|undefined, type:'manuscript'|'comment', ids:number[]) => Record<number, number>} buildVoteMap
 */

/**
 * Fetch the most recent notifications for a user.
 * @param {number} userId
 * @param {{limit?:number, offset?:number}} [opts]
 * @returns {any[]}
 */
function fetchNotifications(userId, opts = {}) {
  const limit = Math.min(200, Math.max(1, opts.limit || 50));
  const offset = Math.max(0, opts.offset || 0);
  return db.prepare(`
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
  `).all(userId, limit, offset);
}

/**
 * Register the social-graph routes:
 *   POST /u/:username/follow
 *   POST /u/:username/unfollow
 *   GET  /feed                          — manuscripts from people I follow
 *   GET  /me/notifications              — my notifications inbox
 *   POST /me/notifications/mark-read    — mark some/all notifications as read
 *   POST /me/theme                      — set the theme cookie
 *
 * @param {import('express').Application} app
 * @param {SocialDeps} deps
 */
function register(app, deps) {
  const { flash, buildVoteMap } = deps;

  // expose for the API router
  app.locals.fetchNotifications = fetchNotifications;

  app.post('/u/:username/follow', requireAuth, (req, res) => {
    const target = /** @type {{id:number, username:string}|undefined} */ (
      db.prepare('SELECT id, username FROM users WHERE username = ?').get(req.params.username)
    );
    if (!target) return res.status(404).render('error', { code: 404, msg: 'No such user.' });
    if (target.id === req.user.id) {
      flash(req, 'error', 'You cannot follow yourself.');
      return res.redirect('/u/' + target.username);
    }
    try {
      db.prepare('INSERT OR IGNORE INTO follows (follower_id, followee_id) VALUES (?, ?)')
        .run(req.user.id, target.id);
      flash(req, 'ok', 'Following @' + target.username + '.');
    } catch (e) {
      flash(req, 'error', 'Could not follow: ' + ((/** @type {Error} */ (e)).message || 'unknown error'));
    }
    res.redirect('/u/' + target.username);
  });

  app.post('/u/:username/unfollow', requireAuth, (req, res) => {
    const target = /** @type {{id:number, username:string}|undefined} */ (
      db.prepare('SELECT id, username FROM users WHERE username = ?').get(req.params.username)
    );
    if (!target) return res.status(404).render('error', { code: 404, msg: 'No such user.' });
    db.prepare('DELETE FROM follows WHERE follower_id = ? AND followee_id = ?')
      .run(req.user.id, target.id);
    flash(req, 'ok', 'Unfollowed @' + target.username + '.');
    res.redirect('/u/' + target.username);
  });

  app.get('/feed', requireAuth, (req, res) => {
    const { page, per, offset } = paginate(req, 30);
    const rows = /** @type {{id:number}[]} */ (
      db.prepare(`
        SELECT m.*, u.username AS submitter_username, u.display_name AS submitter_display
        FROM manuscripts m
        JOIN users u ON u.id = m.submitter_id
        JOIN follows f ON f.followee_id = m.submitter_id
        WHERE f.follower_id = ?
        ORDER BY m.created_at DESC LIMIT ? OFFSET ?
      `)
        .all(req.user.id, per, offset)
    );
    const voteMap = req.user ? buildVoteMap(req.user.id, 'manuscript', rows.map(r => r.id)) : {};
    res.render('feed', { manuscripts: rows, voteMap, page, per });
  });

  app.get('/me/notifications', requireAuth, (req, res) => {
    const { page, per, offset } = paginate(req, 50);
    const items = fetchNotifications(req.user.id, { limit: per, offset });
    res.render('notifications', { items, page, per });
  });

  app.post('/me/notifications/mark-read', requireAuth, (req, res) => {
    const all = req.body && (req.body.all === '1' || req.body.all === 'true' || req.body.all === true || req.body.all === 'on');
    if (all) {
      db.prepare('UPDATE notifications SET seen = 1 WHERE user_id = ? AND seen = 0')
        .run(req.user.id);
    } else {
      let ids = req.body && req.body.ids;
      if (typeof ids === 'string') ids = ids.split(',').map((/** @type {string} */ s) => parseInt(s, 10)).filter(Number.isFinite);
      if (!Array.isArray(ids)) ids = [];
      ids = ids.map((/** @type {any} */ x) => parseInt(x, 10)).filter(Number.isFinite);
      if (ids.length) {
        const placeholders = ids.map(() => '?').join(',');
        db.prepare(`UPDATE notifications SET seen = 1 WHERE user_id = ? AND id IN (${placeholders})`)
          .run(req.user.id, ...ids);
      }
    }
    if (req.headers.accept && req.headers.accept.includes('application/json')) {
      return res.json({ ok: true });
    }
    res.redirect('/me/notifications');
  });

  app.post('/me/theme', (req, res) => {
    const t = (req.body && req.body.theme || '').toString();
    const valid = (t === 'light' || t === 'dark' || t === 'auto') ? t : 'auto';
    const maxAge = 60 * 60 * 24 * 365;
    res.setHeader('Set-Cookie',
      `prexiv_theme=${encodeURIComponent(valid)}; Max-Age=${maxAge}; Path=/; SameSite=Lax`);
    let dest = '/';
    const ref = req.get('Referer');
    if (ref) {
      try {
        const u = new URL(ref);
        if (u.host === req.get('host')) dest = u.pathname + u.search + u.hash;
      } catch { /* ignore */ }
    }
    if (req.headers.accept && req.headers.accept.includes('application/json')) {
      return res.json({ theme: valid });
    }
    res.redirect(dest);
  });
}

module.exports = { register, fetchNotifications };
