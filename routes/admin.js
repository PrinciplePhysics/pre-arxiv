// Admin queue routes — lists open flags and lets an admin resolve them.
//
// Hard-delete of a flagged manuscript is in routes/manuscript.js (POST
// /m/:id/delete) so it lives next to the other manuscript-mutation routes.

const { db } = require('../db');
const { paginate } = require('../lib/util');

/**
 * @typedef {object} AdminDeps
 * @property {import('express').RequestHandler} requireAdmin
 * @property {(req:any, action:string, type:string|null, id:number|null, detail:string|null) => void} [auditLog]
 */

/**
 * Register the admin routes:
 *   GET  /admin                    — flag queue (unresolved only, last 200)
 *   POST /admin/flag/:id/resolve   — mark a flag as resolved with optional note
 *   GET  /admin/audit              — paginated audit log
 *
 * @param {import('express').Application} app
 * @param {AdminDeps} deps
 */
function register(app, deps) {
  const { requireAdmin } = deps;
  const audit = deps.auditLog || ((/** @type {any} */ _r, /** @type {any} */ _a, /** @type {any} */ _t, /** @type {any} */ _i, /** @type {any} */ _d) => {});

  app.get('/admin', requireAdmin, (_req, res) => {
    const flags = /** @type {{id:number, target_type:string, target_id:number}[]} */ (
      db.prepare(`
        SELECT f.*, u.username AS reporter_username
        FROM flag_reports f JOIN users u ON u.id = f.reporter_id
        WHERE f.resolved = 0
        ORDER BY f.created_at DESC LIMIT 200
      `).all()
    );
    const enriched = flags.map(f => {
      if (f.target_type === 'manuscript') {
        const m = /** @type {{id:number, arxiv_like_id:string, title:string, withdrawn:number}|undefined} */ (
          db.prepare('SELECT id, arxiv_like_id, title, withdrawn FROM manuscripts WHERE id = ?').get(f.target_id)
        );
        return { ...f, target: m, targetUrl: m ? '/m/' + m.arxiv_like_id : null };
      } else {
        const c = /** @type {{id:number, content:string, author_id:number, arxiv_like_id:string, author_username:string}|undefined} */ (
          db.prepare(`
            SELECT c.id, c.content, c.author_id, m.arxiv_like_id, u.username AS author_username
            FROM comments c JOIN manuscripts m ON m.id = c.manuscript_id
            JOIN users u ON u.id = c.author_id
            WHERE c.id = ?
          `).get(f.target_id)
        );
        return { ...f, target: c, targetUrl: c ? '/m/' + c.arxiv_like_id + '#c' + c.id : null };
      }
    });
    res.render('admin', { flags: enriched });
  });

  app.post('/admin/flag/:id/resolve', requireAdmin, (req, res) => {
    const id = parseInt(req.params.id, 10);
    const note = (req.body.note || '').trim().slice(0, 500);
    db.prepare(`UPDATE flag_reports SET resolved = 1, resolved_by_id = ?, resolved_at = CURRENT_TIMESTAMP, resolution_note = ? WHERE id = ?`)
      .run(req.user.id, note || null, id);
    audit(req, 'flag_resolve', 'flag', id, note || null);
    res.redirect('/admin');
  });

  app.get('/admin/audit', requireAdmin, (req, res) => {
    const { page, per, offset } = paginate(req, 50);
    const rows = db.prepare(`
      SELECT a.id, a.actor_user_id, a.action, a.target_type, a.target_id, a.detail, a.ip, a.created_at,
             u.username AS actor_username
      FROM audit_log a
      LEFT JOIN users u ON u.id = a.actor_user_id
      ORDER BY a.id DESC LIMIT ? OFFSET ?
    `).all(per, offset);
    const totalRow = /** @type {{n:number}} */ (
      db.prepare('SELECT COUNT(*) AS n FROM audit_log').get()
    );
    res.render('admin_audit', { entries: rows, page, per, total: totalRow.n });
  });
}

module.exports = { register };
