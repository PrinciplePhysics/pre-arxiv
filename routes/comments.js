// Comment routes — adding a comment to a manuscript and deleting one.
//
// Comments are flat-stored but can reference a parent_id within the same
// manuscript to render a threaded view. The detail-page tree-build is in
// routes/manuscript.js (since it lives in the manuscript page render).

const { db } = require('../db');
const { requireAuth } = require('../lib/auth');

/**
 * @typedef {object} CommentsDeps
 * @property {import('express').RequestHandler} commentLimiter
 * @property {(user:any) => boolean} isAdmin
 * @property {(req:any, type:'ok'|'error', msg:string) => void} flash
 * @property {(userId:number, kind:string, opts?:{actor_id?:number, manuscript_id?:number, comment_id?:number}) => void} [createNotification]
 * @property {(event:string, payload:any) => void} [safeEmit]
 * @property {(req:any, action:string, type:string|null, id:number|null, detail:string|null) => void} [auditLog]
 */

/**
 * Register the comment routes:
 *   POST /m/:id/comment        — create a comment on a manuscript (text or threaded reply)
 *   POST /comment/:id/delete   — delete one of your comments (or admin can delete any)
 *
 * @param {import('express').Application} app
 * @param {CommentsDeps} deps
 */
function register(app, deps) {
  const { commentLimiter, isAdmin, flash } = deps;
  const notify = deps.createNotification || ((/** @type {any} */ _u, /** @type {any} */ _k, /** @type {any} */ _o) => {});
  const emit = deps.safeEmit || ((/** @type {string} */ _e, /** @type {any} */ _p) => {});
  const audit = deps.auditLog || ((/** @type {any} */ _r, /** @type {any} */ _a, /** @type {any} */ _t, /** @type {any} */ _i, /** @type {any} */ _d) => {});

  app.post('/m/:id/comment', commentLimiter, requireAuth, (req, res) => {
    const m = /** @type {{id:number, submitter_id:number}|undefined} */ (
      db.prepare('SELECT id, submitter_id FROM manuscripts WHERE arxiv_like_id = ? OR id = ?').get(req.params.id, req.params.id)
    );
    if (!m) return res.status(404).render('error', { code: 404, msg: 'Manuscript not found.' });
    const content = (req.body.content || '').trim();
    /** @type {number|null} */
    let parentId = null;
    if (!content || content.length < 2) {
      flash(req, 'error', 'Comment cannot be empty.');
      return res.redirect('/m/' + req.params.id);
    }
    if (content.length > 8000) {
      flash(req, 'error', 'Comment is too long.');
      return res.redirect('/m/' + req.params.id);
    }
    /** @type {number|null} */
    let parentAuthorId = null;
    if (req.body.parent_id) {
      parentId = parseInt(req.body.parent_id, 10);
      if (!Number.isInteger(parentId) || /** @type {number} */ (parentId) <= 0) {
        flash(req, 'error', 'That reply target is invalid.');
        return res.redirect('/m/' + req.params.id);
      }
      const parent = /** @type {{id:number, author_id:number}|undefined} */ (
        db.prepare('SELECT id, author_id FROM comments WHERE id = ? AND manuscript_id = ?').get(parentId, m.id)
      );
      if (!parent) {
        flash(req, 'error', 'That reply target is no longer available.');
        return res.redirect('/m/' + req.params.id);
      }
      parentAuthorId = parent.author_id;
    }
    const r = db.prepare('INSERT INTO comments (manuscript_id, author_id, parent_id, content, score) VALUES (?, ?, ?, ?, 1)')
      .run(m.id, req.user.id, parentId, content);
    db.prepare("INSERT INTO votes (user_id, target_type, target_id, value) VALUES (?, 'comment', ?, 1)")
      .run(req.user.id, r.lastInsertRowid);
    db.prepare('UPDATE manuscripts SET comment_count = (SELECT COUNT(*) FROM comments WHERE manuscript_id = ?) WHERE id = ?').run(m.id, m.id);
    // Notifications:
    if (parentAuthorId) {
      notify(parentAuthorId, 'reply_to_my_comment', {
        actor_id: req.user.id, manuscript_id: m.id, comment_id: /** @type {number} */ (r.lastInsertRowid),
      });
    } else {
      notify(m.submitter_id, 'comment_on_my_manuscript', {
        actor_id: req.user.id, manuscript_id: m.id, comment_id: /** @type {number} */ (r.lastInsertRowid),
      });
    }
    // Webhook fan-out.
    emit('comment.created', {
      id: r.lastInsertRowid,
      manuscript_id: m.id,
      parent_id: parentId,
      author_id: req.user.id,
      author_username: req.user.username || null,
      content,
      created_at: new Date().toISOString(),
    });
    res.redirect('/m/' + req.params.id + '#c' + r.lastInsertRowid);
  });

  app.post('/comment/:id/delete', requireAuth, (req, res) => {
    const c = /** @type {{id:number, author_id:number, manuscript_id:number, arxiv_like_id:string}|undefined} */ (
      db.prepare('SELECT c.id, c.author_id, c.manuscript_id, m.arxiv_like_id FROM comments c JOIN manuscripts m ON m.id = c.manuscript_id WHERE c.id = ?').get(req.params.id)
    );
    if (!c) return res.status(404).render('error', { code: 404, msg: 'Comment not found.' });
    const allowed = (c.author_id === req.user.id) || isAdmin(req.user);
    if (!allowed) return res.status(403).render('error', { code: 403, msg: 'You can only delete your own comments.' });
    db.prepare('DELETE FROM comments WHERE id = ?').run(c.id);
    db.prepare('UPDATE manuscripts SET comment_count = (SELECT COUNT(*) FROM comments WHERE manuscript_id = ?) WHERE id = ?').run(c.manuscript_id, c.manuscript_id);
    if (c.author_id !== req.user.id && isAdmin(req.user)) {
      audit(req, 'comment_delete_admin', 'comment', c.id, 'on ' + c.arxiv_like_id);
    }
    // Webhook fan-out.
    emit('comment.deleted', {
      id: c.id, manuscript_id: c.manuscript_id, author_id: c.author_id,
      deleted_by_id: req.user.id,
      ts: new Date().toISOString(),
    });
    res.redirect('/m/' + c.arxiv_like_id);
  });
}

module.exports = { register };
