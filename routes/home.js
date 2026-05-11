// Home / browse / search routes.
//
// Read-only listing pages. None of these require auth; any per-user data
// (like the current user's vote map) is folded in via `req.user` if present.

const { db, CATEGORIES } = require('../db');
const { paginate } = require('../lib/util');

/**
 * @typedef {object} HomeDeps
 * @property {(userId:number|undefined, type:'manuscript'|'comment', ids:number[]) => Record<number, number>} buildVoteMap
 * @property {<R extends {score:number, created_at:string}>(rows:R[]) => (R & {rankValue:number})[]} rankManuscripts
 * @property {(q:string) => string} escapeFtsQuery
 * @property {(value:unknown) => string} firstQueryString
 * @property {(qq:Record<string, any>) => {category:string, mode:string, dateFrom:string, dateTo:string, scoreMin:number|null, sql:string, params:any[]}} parseSearchFilters
 */

/**
 * Register the home / browse / search routes.
 *
 * Routes registered:
 *   GET /                — HN-style ranked manuscript list
 *   GET /new             — newest first
 *   GET /top             — by score, then created_at
 *   GET /audited         — has_auditor=1 only
 *   GET /browse          — category counts grid
 *   GET /browse/:cat     — manuscripts in a single category
 *   GET /search          — FTS5 over title/abstract/authors/pdf body, plus exact id/DOI matches
 *
 * @param {import('express').Application} app
 * @param {HomeDeps} deps
 */
function register(app, deps) {
  const { buildVoteMap, rankManuscripts, escapeFtsQuery, firstQueryString, parseSearchFilters } = deps;

  app.get('/', (req, res) => {
    const { page, per, offset } = paginate(req, 30);
    // pull a wider window then rank
    const window = /** @type {{id:number, score:number, created_at:string}[]} */ (
      db.prepare(`
        SELECT m.*, u.username AS submitter_username, u.display_name AS submitter_display
        FROM manuscripts m JOIN users u ON u.id = m.submitter_id
        ORDER BY m.created_at DESC
        LIMIT 300
      `).all()
    );
    const ranked = rankManuscripts(window).slice(offset, offset + per);
    const voteMap = req.user ? buildVoteMap(req.user.id, 'manuscript', ranked.map(r => r.id)) : {};
    res.render('index', { manuscripts: ranked, voteMap, mode: 'ranked', page, per });
  });

  app.get('/new', (req, res) => {
    const { page, per, offset } = paginate(req, 30);
    const rows = /** @type {{id:number}[]} */ (
      db.prepare(`
        SELECT m.*, u.username AS submitter_username, u.display_name AS submitter_display
        FROM manuscripts m JOIN users u ON u.id = m.submitter_id
        ORDER BY m.created_at DESC LIMIT ? OFFSET ?
      `).all(per, offset)
    );
    const voteMap = req.user ? buildVoteMap(req.user.id, 'manuscript', rows.map(r => r.id)) : {};
    res.render('index', { manuscripts: rows, voteMap, mode: 'new', page, per });
  });

  app.get('/top', (req, res) => {
    const { page, per, offset } = paginate(req, 30);
    const rows = /** @type {{id:number}[]} */ (
      db.prepare(`
        SELECT m.*, u.username AS submitter_username, u.display_name AS submitter_display
        FROM manuscripts m JOIN users u ON u.id = m.submitter_id
        ORDER BY m.score DESC, m.created_at DESC LIMIT ? OFFSET ?
      `).all(per, offset)
    );
    const voteMap = req.user ? buildVoteMap(req.user.id, 'manuscript', rows.map(r => r.id)) : {};
    res.render('index', { manuscripts: rows, voteMap, mode: 'top', page, per });
  });

  app.get('/audited', (req, res) => {
    const { page, per, offset } = paginate(req, 30);
    const rows = /** @type {{id:number}[]} */ (
      db.prepare(`
        SELECT m.*, u.username AS submitter_username, u.display_name AS submitter_display
        FROM manuscripts m JOIN users u ON u.id = m.submitter_id
        WHERE m.has_auditor = 1
        ORDER BY m.created_at DESC LIMIT ? OFFSET ?
      `).all(per, offset)
    );
    const voteMap = req.user ? buildVoteMap(req.user.id, 'manuscript', rows.map(r => r.id)) : {};
    res.render('index', { manuscripts: rows, voteMap, mode: 'audited', page, per });
  });

  app.get('/browse', (_req, res) => {
    /** @type {Record<string, number>} */
    const counts = {};
    const rows = /** @type {{category:string, n:number}[]} */ (
      db.prepare('SELECT category, COUNT(*) AS n FROM manuscripts GROUP BY category').all()
    );
    for (const r of rows) {
      counts[r.category] = r.n;
    }
    res.render('browse', { counts });
  });

  app.get('/browse/:cat', (req, res) => {
    const cat = req.params.cat;
    const meta = CATEGORIES.find((/** @type {{id:string}} */ c) => c.id === cat);
    if (!meta) return res.status(404).render('error', { code: 404, msg: 'Unknown category.' });
    const { page, per, offset } = paginate(req, 30);
    const rows = /** @type {{id:number}[]} */ (
      db.prepare(`
        SELECT m.*, u.username AS submitter_username, u.display_name AS submitter_display
        FROM manuscripts m JOIN users u ON u.id = m.submitter_id
        WHERE m.category = ?
        ORDER BY m.created_at DESC LIMIT ? OFFSET ?
      `).all(cat, per, offset)
    );
    const voteMap = req.user ? buildVoteMap(req.user.id, 'manuscript', rows.map(r => r.id)) : {};
    res.render('index', { manuscripts: rows, voteMap, mode: 'category', categoryMeta: meta, page, per });
  });

  app.get('/search', (req, res) => {
    const q = firstQueryString(req.query.q).trim();
    const filters = parseSearchFilters(req.query);
    /** @type {{id:number}[]} */
    const rows = [];
    const seen = new Set();
    const filterClause = filters.sql ? ' AND ' + filters.sql : '';

    if (q) {
      const idMatches = /** @type {{id:number}[]} */ (
        db.prepare(`
          SELECT m.*, u.username AS submitter_username, u.display_name AS submitter_display
          FROM manuscripts m JOIN users u ON u.id = m.submitter_id
          WHERE (m.arxiv_like_id = ? OR m.doi = ? OR m.arxiv_like_id LIKE ? OR m.doi LIKE ?)
            ${filterClause}
          LIMIT 20
        `).all(q, q, q + '%', q + '%', ...filters.params)
      );
      for (const r of idMatches) if (!seen.has(r.id)) { seen.add(r.id); rows.push(r); }

      const ftsQ = escapeFtsQuery(q);
      if (ftsQ) {
        try {
          const ftsRows = /** @type {{id:number}[]} */ (
            db.prepare(`
              SELECT m.*, u.username AS submitter_username, u.display_name AS submitter_display
              FROM manuscripts m
              JOIN users u ON u.id = m.submitter_id
              JOIN manuscripts_fts fts ON fts.rowid = m.id
              WHERE manuscripts_fts MATCH ?
                ${filterClause}
              ORDER BY rank
              LIMIT 100
            `).all(ftsQ, ...filters.params)
          );
          for (const r of ftsRows) if (!seen.has(r.id)) { seen.add(r.id); rows.push(r); }
        } catch (_e) {
          // bad query (rare) — fall through silently
        }
      }
    } else if (filters.sql) {
      const onlyRows = /** @type {{id:number}[]} */ (
        db.prepare(`
          SELECT m.*, u.username AS submitter_username, u.display_name AS submitter_display
          FROM manuscripts m JOIN users u ON u.id = m.submitter_id
          WHERE ${filters.sql}
          ORDER BY m.created_at DESC LIMIT 100
        `).all(...filters.params)
      );
      for (const r of onlyRows) if (!seen.has(r.id)) { seen.add(r.id); rows.push(r); }
    }
    const voteMap = req.user ? buildVoteMap(req.user.id, 'manuscript', rows.map(r => r.id)) : {};
    res.render('search', { manuscripts: rows, voteMap, q, filters });
  });
}

module.exports = { register };
