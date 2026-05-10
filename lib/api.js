// PreXiv JSON API mounted at /api/v1.
//
// Every operation a logged-in human can do via the website has a JSON twin
// here. Read endpoints are public; write endpoints require a Bearer token.
// Validation reuses parseManuscriptValues / validateManuscriptValues from
// the caller (we receive them via the factory's `deps` argument).
//
// Errors follow the shape `{ error: "<msg>", details?: [...] }`.

const express = require('express');
const crypto = require('crypto');
const { db, CATEGORIES, ROLES } = require('../db');
const { hashPassword, verifyPassword, validateUsername } = require('./auth');
const { generateToken, hashToken, extractBearer } = require('./api-auth');
const { makeArxivLikeId, paginate, rankScore, ageHours } = require('./util');
const { buildOpenApi } = require('./openapi');
const zenodo = require('./zenodo');

function makeSyntheticDoi(arxivLikeId) {
  return '10.99999/' + (arxivLikeId || '').toUpperCase();
}

function isAdmin(user) {
  if (!user) return false;
  const r = db.prepare('SELECT is_admin FROM users WHERE id = ?').get(user.id);
  return !!(r && r.is_admin);
}

function publicUser(u) {
  if (!u) return null;
  return {
    id: u.id,
    username: u.username,
    display_name: u.display_name || null,
    affiliation: u.affiliation || null,
    karma: u.karma || 0,
    is_admin: !!u.is_admin,
    email: u.email || null,
    email_verified: !!u.email_verified,
    created_at: u.created_at || null,
  };
}

function fetchUserFull(id) {
  return db.prepare(`
    SELECT id, username, email, display_name, affiliation, bio, karma, is_admin, email_verified, created_at
    FROM users WHERE id = ?
  `).get(id);
}

function fetchManuscript(idOrSlug) {
  return db.prepare(`
    SELECT m.*, u.username AS submitter_username, u.display_name AS submitter_display
    FROM manuscripts m JOIN users u ON u.id = m.submitter_id
    WHERE m.arxiv_like_id = ? OR m.id = ?
  `).get(idOrSlug, idOrSlug);
}

// Redact private conductor fields (`conductor_human`, `conductor_ai_model`)
// from the API response when the viewer is neither the submitter nor an
// admin. The `_public` flags themselves remain so clients can render an
// "(undisclosed)" label. Submitter/admin always see the real values.
function redactManuscript(m, viewer, viewerIsAdmin) {
  if (!m) return m;
  const isOwner = !!(viewer && m.submitter_id === viewer.id);
  if (isOwner || viewerIsAdmin) return m;
  const out = { ...m };
  if (m.conductor_ai_model_public === 0) out.conductor_ai_model = null;
  if (m.conductor_human_public    === 0) out.conductor_human    = null;
  return out;
}

function err(res, code, message, details) {
  const body = { error: message };
  if (details) body.details = details;
  return res.status(code).json(body);
}

// API-level requireAuth — always JSON 401, never an HTML redirect. We only
// accept Bearer (not session) because the API surface is for non-browser
// clients; a logged-in browser session that calls /api/v1/me without a
// Bearer header should see a clean 401, not get treated as authenticated.
function requireApiAuth(req, res, next) {
  if (!extractBearer(req)) return err(res, 401, 'Bearer token required.');
  if (!req.user) return err(res, 401, 'Invalid or expired Bearer token.');
  next();
}
function requireApiAdmin(req, res, next) {
  if (!extractBearer(req)) return err(res, 401, 'Bearer token required.');
  if (!req.user) return err(res, 401, 'Invalid or expired Bearer token.');
  if (!isAdmin(req.user)) return err(res, 403, 'Admin only.');
  next();
}

function buildVoteForUser(userId, type, id) {
  if (!userId) return 0;
  const row = db.prepare('SELECT value FROM votes WHERE user_id = ? AND target_type = ? AND target_id = ?')
    .get(userId, type, id);
  return row ? row.value : 0;
}

function applyVote(userId, type, targetId, value) {
  const table = type === 'manuscript' ? 'manuscripts' : 'comments';
  const existing = db.prepare('SELECT value FROM votes WHERE user_id = ? AND target_type = ? AND target_id = ?')
    .get(userId, type, targetId);
  let delta = 0;
  if (!existing) {
    db.prepare('INSERT INTO votes (user_id, target_type, target_id, value) VALUES (?, ?, ?, ?)')
      .run(userId, type, targetId, value);
    delta = value;
  } else if (existing.value === value) {
    db.prepare('DELETE FROM votes WHERE user_id = ? AND target_type = ? AND target_id = ?')
      .run(userId, type, targetId);
    delta = -value;
  } else {
    db.prepare('UPDATE votes SET value = ? WHERE user_id = ? AND target_type = ? AND target_id = ?')
      .run(value, userId, type, targetId);
    delta = value - existing.value;
  }
  db.prepare(`UPDATE ${table} SET score = score + ? WHERE id = ?`).run(delta, targetId);
  const authorCol = type === 'manuscript' ? 'submitter_id' : 'author_id';
  const author = db.prepare(`SELECT ${authorCol} AS aid FROM ${table} WHERE id = ?`).get(targetId);
  if (author && author.aid !== userId) {
    db.prepare('UPDATE users SET karma = karma + ? WHERE id = ?').run(delta, author.aid);
  }
  return db.prepare(`SELECT score FROM ${table} WHERE id = ?`).get(targetId).score;
}

// `deps` carries: { parseManuscriptValues, validateManuscriptValues,
//                   authLimiter, submitLimiter, commentLimiter, voteLimiter,
//                   escapeFtsQuery }
function buildApiRouter(deps) {
  const router = express.Router();
  const { parseManuscriptValues, validateManuscriptValues,
          authLimiter, submitLimiter, commentLimiter, voteLimiter,
          escapeFtsQuery } = deps;

  // Make sure JSON errors bubble up as JSON, not as the HTML error page.
  router.use((req, res, next) => {
    res.setHeader('Cache-Control', 'no-store');
    next();
  });

  // ─── auth + identity ─────────────────────────────────────────────────────
  router.post('/register', authLimiter, (req, res) => {
    const username     = (req.body.username || '').trim();
    const email        = (req.body.email || '').trim().toLowerCase();
    const password     = req.body.password || '';
    const display_name = (req.body.display_name || '').trim() || null;
    const affiliation  = (req.body.affiliation || '').trim() || null;

    const errors = [];
    const uErr = validateUsername(username);
    if (uErr) errors.push(uErr);
    if (!email || !/^[^@\s]+@[^@\s]+\.[^@\s]+$/.test(email)) errors.push('A valid email is required.');
    if (!password || password.length < 8) errors.push('Password must be ≥ 8 characters.');
    if (!errors.length) {
      const dup = db.prepare('SELECT 1 FROM users WHERE username = ? OR email = ?').get(username, email);
      if (dup) errors.push('That username or email is already in use.');
    }
    if (errors.length) return err(res, 422, 'Validation failed.', errors);

    // API path skips both the math CAPTCHA and the email-verify gate.
    const r = db.prepare(`
      INSERT INTO users (username, email, password_hash, display_name, affiliation, email_verified)
      VALUES (?, ?, ?, ?, ?, 1)
    `).run(username, email, hashPassword(password), display_name, affiliation);

    const plain = generateToken();
    const tokRow = db.prepare(
      'INSERT INTO api_tokens (user_id, token_hash, name) VALUES (?, ?, ?)'
    ).run(r.lastInsertRowid, hashToken(plain), 'register');

    const u = fetchUserFull(r.lastInsertRowid);
    return res.json({
      user: publicUser(u),
      token: plain,
      verify_url: null,
      token_id: tokRow.lastInsertRowid,
    });
  });

  router.post('/login', authLimiter, (req, res) => {
    const id  = (req.body.username_or_email || req.body.username || '').trim();
    const pw  = req.body.password || '';
    if (!id || !pw) return err(res, 400, 'username_or_email and password are required.');
    const u = db.prepare('SELECT id, password_hash FROM users WHERE username = ? OR email = ?').get(id, id);
    if (!u || !verifyPassword(pw, u.password_hash)) return err(res, 401, 'Invalid username or password.');

    const plain = generateToken();
    db.prepare('INSERT INTO api_tokens (user_id, token_hash, name) VALUES (?, ?, ?)')
      .run(u.id, hashToken(plain), 'login');
    return res.json({ user: publicUser(fetchUserFull(u.id)), token: plain });
  });

  router.post('/logout', requireApiAuth, (req, res) => {
    if (req.user._api_token_id) {
      db.prepare('DELETE FROM api_tokens WHERE id = ? AND user_id = ?').run(req.user._api_token_id, req.user.id);
    }
    res.json({ ok: true });
  });

  router.get('/me', requireApiAuth, (req, res) => {
    res.json(publicUser(fetchUserFull(req.user.id)));
  });

  router.get('/me/tokens', requireApiAuth, (req, res) => {
    const rows = db.prepare(
      'SELECT id, name, last_used_at, created_at, expires_at FROM api_tokens WHERE user_id = ? ORDER BY created_at DESC'
    ).all(req.user.id);
    res.json(rows);
  });

  router.post('/me/tokens', requireApiAuth, (req, res) => {
    const name = (req.body && typeof req.body.name === 'string') ? req.body.name.trim().slice(0, 200) : null;
    const plain = generateToken();
    const r = db.prepare('INSERT INTO api_tokens (user_id, token_hash, name) VALUES (?, ?, ?)')
      .run(req.user.id, hashToken(plain), name || null);
    const row = db.prepare('SELECT id, name, created_at FROM api_tokens WHERE id = ?').get(r.lastInsertRowid);
    res.json({ id: row.id, name: row.name, token: plain, created_at: row.created_at });
  });

  router.delete('/me/tokens/:id', requireApiAuth, (req, res) => {
    const id = parseInt(req.params.id, 10);
    if (!id) return err(res, 400, 'Bad token id.');
    const t = db.prepare('SELECT id, user_id FROM api_tokens WHERE id = ?').get(id);
    if (!t || t.user_id !== req.user.id) return err(res, 404, 'Token not found.');
    db.prepare('DELETE FROM api_tokens WHERE id = ?').run(id);
    res.json({ ok: true });
  });

  // ─── manuscripts: list / get / create / update / withdraw / delete ───────
  router.get('/manuscripts', (req, res) => {
    const mode = (req.query.mode || 'ranked').toString();
    const cat  = req.query.category ? String(req.query.category) : null;
    const { page, per, offset } = paginate(req, 30);

    let items;
    if (cat) {
      items = db.prepare(`
        SELECT m.*, u.username AS submitter_username, u.display_name AS submitter_display
        FROM manuscripts m JOIN users u ON u.id = m.submitter_id
        WHERE m.category = ?
        ORDER BY m.created_at DESC LIMIT ? OFFSET ?
      `).all(cat, per, offset);
    } else if (mode === 'new') {
      items = db.prepare(`
        SELECT m.*, u.username AS submitter_username, u.display_name AS submitter_display
        FROM manuscripts m JOIN users u ON u.id = m.submitter_id
        ORDER BY m.created_at DESC LIMIT ? OFFSET ?
      `).all(per, offset);
    } else if (mode === 'top') {
      items = db.prepare(`
        SELECT m.*, u.username AS submitter_username, u.display_name AS submitter_display
        FROM manuscripts m JOIN users u ON u.id = m.submitter_id
        ORDER BY m.score DESC, m.created_at DESC LIMIT ? OFFSET ?
      `).all(per, offset);
    } else if (mode === 'audited') {
      items = db.prepare(`
        SELECT m.*, u.username AS submitter_username, u.display_name AS submitter_display
        FROM manuscripts m JOIN users u ON u.id = m.submitter_id
        WHERE m.has_auditor = 1
        ORDER BY m.created_at DESC LIMIT ? OFFSET ?
      `).all(per, offset);
    } else {
      // ranked — replicate the home-page behaviour (sample window, then rank)
      const window = db.prepare(`
        SELECT m.*, u.username AS submitter_username, u.display_name AS submitter_display
        FROM manuscripts m JOIN users u ON u.id = m.submitter_id
        ORDER BY m.created_at DESC LIMIT 300
      `).all();
      const ranked = window
        .map(r => ({ ...r, _rank: rankScore(r.score, ageHours(r.created_at)) }))
        .sort((a, b) => b._rank - a._rank);
      items = ranked.slice(offset, offset + per).map(r => { delete r._rank; return r; });
    }
    const adminFlag = isAdmin(req.user);
    res.json({ items: items.map(m => redactManuscript(m, req.user, adminFlag)), page, per, mode, category: cat });
  });

  router.get('/manuscripts/:id', (req, res) => {
    const m = fetchManuscript(req.params.id);
    if (!m) return err(res, 404, 'Manuscript not found.');
    db.prepare('UPDATE manuscripts SET view_count = view_count + 1 WHERE id = ?').run(m.id);
    res.json(redactManuscript(m, req.user, isAdmin(req.user)));
  });

  router.post('/manuscripts', submitLimiter, requireApiAuth, async (req, res) => {
    const v = parseManuscriptValues(req);
    const errors = validateManuscriptValues(v);
    if (!v.external_url) errors.push('external_url is required (PDF upload not supported via JSON API).');
    if (errors.length) return err(res, 422, 'Validation failed.', errors);

    const arxivId = makeArxivLikeId();
    const doi = makeSyntheticDoi(arxivId);

    const r = db.prepare(`
      INSERT INTO manuscripts (
        arxiv_like_id, doi, submitter_id, title, abstract, authors, category, pdf_path, pdf_text, external_url,
        conductor_type, conductor_ai_model, conductor_ai_model_public,
        conductor_human, conductor_human_public, conductor_role, conductor_notes, agent_framework,
        has_auditor, auditor_name, auditor_affiliation, auditor_role, auditor_statement,
        score
      ) VALUES (?, ?, ?, ?, ?, ?, ?, NULL, NULL, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, 1)
    `).run(
      arxivId, doi, req.user.id, v.title, v.abstract, v.authors, v.category, v.external_url,
      v.conductor_type,
      v.conductor_ai_model,
      v.conductor_ai_model_private ? 0 : 1,
      v.conductor_type === 'human-ai' ? v.conductor_human : null,
      v.conductor_human_private ? 0 : 1,
      v.conductor_type === 'human-ai' ? v.conductor_role : null,
      v.conductor_notes,
      v.conductor_type === 'ai-agent' ? v.agent_framework : null,
      v.has_auditor ? 1 : 0,
      v.has_auditor ? v.auditor_name : null,
      v.has_auditor ? (v.auditor_affiliation || null) : null,
      v.has_auditor ? v.auditor_role : null,
      v.has_auditor ? v.auditor_statement : null,
    );
    db.prepare("INSERT INTO votes (user_id, target_type, target_id, value) VALUES (?, 'manuscript', ?, 1)")
      .run(req.user.id, r.lastInsertRowid);

    // Best-effort Zenodo deposition (mirrors the web flow). Failures are
    // tolerated; the manuscript stays posted with the synthetic DOI.
    if (zenodo.enabled) {
      const base = (process.env.APP_URL || '').replace(/\/+$/, '') ||
        ((req.get('x-forwarded-proto') || (req.secure ? 'https' : 'http')) + '://' + req.get('host'));
      const mForZenodo = {
        arxiv_like_id: arxivId, title: v.title, abstract: v.abstract,
        authors: v.authors, category: v.category,
        conductor_type: v.conductor_type,
        conductor_human: v.conductor_human, conductor_ai_model: v.conductor_ai_model,
        agent_framework: v.agent_framework,
        has_auditor: v.has_auditor, auditor_name: v.auditor_name,
      };
      zenodo.depositAndPublish(mForZenodo, base).then(zr => {
        if (zr.ok && zr.doi) {
          db.prepare('UPDATE manuscripts SET doi = ? WHERE id = ?').run(zr.doi, r.lastInsertRowid);
        }
      }).catch(() => {});
    }

    const m = fetchManuscript(arxivId);
    res.status(200).json(redactManuscript(m, req.user, isAdmin(req.user)));
  });

  router.patch('/manuscripts/:id', submitLimiter, requireApiAuth, (req, res) => {
    const m = fetchManuscript(req.params.id);
    if (!m) return err(res, 404, 'Manuscript not found.');
    if (m.submitter_id !== req.user.id && !isAdmin(req.user)) {
      return err(res, 403, 'You can only edit your own manuscripts.');
    }
    // Build a merged body: take existing values, overlay any provided
    // fields. Then run parse + validate against the merged shape.
    const body = req.body || {};
    const merged = {
      title:         'title'         in body ? body.title         : m.title,
      abstract:      'abstract'      in body ? body.abstract      : m.abstract,
      authors:       'authors'       in body ? body.authors       : m.authors,
      category:      'category'      in body ? body.category      : m.category,
      external_url:  'external_url'  in body ? body.external_url  : (m.external_url || ''),
      conductor_type: 'conductor_type' in body ? body.conductor_type : m.conductor_type,
      conductor_ai_model: 'conductor_ai_model' in body ? body.conductor_ai_model : m.conductor_ai_model,
      conductor_human: 'conductor_human' in body ? body.conductor_human : (m.conductor_human || ''),
      conductor_role: 'conductor_role' in body ? body.conductor_role : (m.conductor_role || ''),
      conductor_notes: 'conductor_notes' in body ? body.conductor_notes : (m.conductor_notes || ''),
      agent_framework: 'agent_framework' in body ? body.agent_framework : (m.agent_framework || ''),
      conductor_ai_model_private:
        'conductor_ai_model_private' in body ? body.conductor_ai_model_private : (m.conductor_ai_model_public === 0 ? '1' : ''),
      conductor_human_private:
        'conductor_human_private' in body ? body.conductor_human_private : (m.conductor_human_public === 0 ? '1' : ''),
      has_auditor:   'has_auditor'   in body ? body.has_auditor   : (m.has_auditor ? '1' : ''),
      auditor_name:  'auditor_name'  in body ? body.auditor_name  : (m.auditor_name || ''),
      auditor_affiliation: 'auditor_affiliation' in body ? body.auditor_affiliation : (m.auditor_affiliation || ''),
      auditor_role:  'auditor_role'  in body ? body.auditor_role  : (m.auditor_role || ''),
      auditor_statement: 'auditor_statement' in body ? body.auditor_statement : (m.auditor_statement || ''),
      no_auditor_ack: body.no_auditor_ack || '',
      ai_agent_ack:   body.ai_agent_ack   || '',
    };
    const v = parseManuscriptValues({ body: merged });
    const errors = validateManuscriptValues(v, { editing: true });
    if (!v.external_url && !m.pdf_path && !m.external_url) {
      errors.push('A manuscript must have an external_url or an existing PDF.');
    }
    if (errors.length) return err(res, 422, 'Validation failed.', errors);

    db.prepare(`
      UPDATE manuscripts SET
        title = ?, abstract = ?, authors = ?, category = ?,
        external_url = ?,
        conductor_type = ?, conductor_ai_model = ?, conductor_ai_model_public = ?,
        conductor_human = ?, conductor_human_public = ?, conductor_role = ?,
        conductor_notes = ?, agent_framework = ?,
        has_auditor = ?, auditor_name = ?, auditor_affiliation = ?, auditor_role = ?, auditor_statement = ?,
        updated_at = CURRENT_TIMESTAMP
      WHERE id = ?
    `).run(
      v.title, v.abstract, v.authors, v.category,
      v.external_url,
      v.conductor_type, v.conductor_ai_model, v.conductor_ai_model_private ? 0 : 1,
      v.conductor_type === 'human-ai' ? v.conductor_human : null,
      v.conductor_human_private ? 0 : 1,
      v.conductor_type === 'human-ai' ? v.conductor_role : null,
      v.conductor_notes,
      v.conductor_type === 'ai-agent' ? v.agent_framework : null,
      v.has_auditor ? 1 : 0,
      v.has_auditor ? v.auditor_name : null,
      v.has_auditor ? (v.auditor_affiliation || null) : null,
      v.has_auditor ? v.auditor_role : null,
      v.has_auditor ? v.auditor_statement : null,
      m.id
    );
    res.json(redactManuscript(fetchManuscript(m.arxiv_like_id), req.user, isAdmin(req.user)));
  });

  router.post('/manuscripts/:id/withdraw', requireApiAuth, (req, res) => {
    const m = fetchManuscript(req.params.id);
    if (!m) return err(res, 404, 'Manuscript not found.');
    if (m.submitter_id !== req.user.id && !isAdmin(req.user)) {
      return err(res, 403, 'You can only withdraw your own manuscripts.');
    }
    const reason = (req.body && req.body.reason ? String(req.body.reason) : '').trim().slice(0, 500) || 'No reason given.';
    db.prepare('UPDATE manuscripts SET withdrawn = 1, withdrawn_reason = ?, withdrawn_at = CURRENT_TIMESTAMP WHERE id = ?')
      .run(reason, m.id);
    res.json(redactManuscript(fetchManuscript(m.arxiv_like_id), req.user, isAdmin(req.user)));
  });

  router.delete('/manuscripts/:id', requireApiAdmin, (req, res) => {
    const m = fetchManuscript(req.params.id);
    if (!m) return err(res, 404, 'Manuscript not found.');
    db.prepare('DELETE FROM manuscripts WHERE id = ?').run(m.id);
    res.json({ ok: true });
  });

  // ─── comments ────────────────────────────────────────────────────────────
  router.get('/manuscripts/:id/comments', (req, res) => {
    const m = fetchManuscript(req.params.id);
    if (!m) return err(res, 404, 'Manuscript not found.');
    const rows = db.prepare(`
      SELECT c.id, c.manuscript_id, c.author_id, c.parent_id, c.content, c.score, c.created_at,
             u.username, u.display_name
      FROM comments c JOIN users u ON u.id = c.author_id
      WHERE c.manuscript_id = ?
      ORDER BY c.created_at ASC
    `).all(m.id);
    res.json(rows);
  });

  router.post('/manuscripts/:id/comments', commentLimiter, requireApiAuth, (req, res) => {
    const m = fetchManuscript(req.params.id);
    if (!m) return err(res, 404, 'Manuscript not found.');
    const content = (req.body && req.body.content ? String(req.body.content) : '').trim();
    const parentId = req.body && req.body.parent_id ? parseInt(req.body.parent_id, 10) : null;
    if (!content || content.length < 2) return err(res, 422, 'Comment content is required (≥ 2 characters).');
    if (content.length > 8000) return err(res, 422, 'Comment is too long (≤ 8000 characters).');
    if (parentId) {
      const p = db.prepare('SELECT id FROM comments WHERE id = ? AND manuscript_id = ?').get(parentId, m.id);
      if (!p) return err(res, 422, 'parent_id does not refer to a comment on this manuscript.');
    }
    const r = db.prepare('INSERT INTO comments (manuscript_id, author_id, parent_id, content, score) VALUES (?, ?, ?, ?, 1)')
      .run(m.id, req.user.id, parentId, content);
    db.prepare("INSERT INTO votes (user_id, target_type, target_id, value) VALUES (?, 'comment', ?, 1)")
      .run(req.user.id, r.lastInsertRowid);
    db.prepare('UPDATE manuscripts SET comment_count = (SELECT COUNT(*) FROM comments WHERE manuscript_id = ?) WHERE id = ?').run(m.id, m.id);
    const row = db.prepare(`
      SELECT c.id, c.manuscript_id, c.author_id, c.parent_id, c.content, c.score, c.created_at,
             u.username, u.display_name
      FROM comments c JOIN users u ON u.id = c.author_id WHERE c.id = ?
    `).get(r.lastInsertRowid);
    res.json(row);
  });

  router.delete('/comments/:id', requireApiAuth, (req, res) => {
    const id = parseInt(req.params.id, 10);
    if (!id) return err(res, 400, 'Bad comment id.');
    const c = db.prepare('SELECT id, author_id, manuscript_id FROM comments WHERE id = ?').get(id);
    if (!c) return err(res, 404, 'Comment not found.');
    if (c.author_id !== req.user.id && !isAdmin(req.user)) return err(res, 403, 'You can only delete your own comments.');
    db.prepare('DELETE FROM comments WHERE id = ?').run(c.id);
    db.prepare('UPDATE manuscripts SET comment_count = (SELECT COUNT(*) FROM comments WHERE manuscript_id = ?) WHERE id = ?').run(c.manuscript_id, c.manuscript_id);
    res.json({ ok: true });
  });

  // ─── votes / flags ───────────────────────────────────────────────────────
  router.post('/votes/:type/:id', voteLimiter, requireApiAuth, (req, res) => {
    const type = req.params.type;
    if (type !== 'manuscript' && type !== 'comment') return err(res, 400, 'type must be manuscript or comment.');
    const id = parseInt(req.params.id, 10);
    if (!id) return err(res, 400, 'Bad target id.');
    const value = parseInt(req.body && req.body.value, 10);
    if (![1, -1].includes(value)) return err(res, 400, 'value must be 1 or -1.');
    const table = type === 'manuscript' ? 'manuscripts' : 'comments';
    const exists = db.prepare(`SELECT 1 FROM ${table} WHERE id = ?`).get(id);
    if (!exists) return err(res, 404, type + ' not found.');
    const newScore = applyVote(req.user.id, type, id, value);
    res.json({ score: newScore, my_vote: buildVoteForUser(req.user.id, type, id) });
  });

  router.post('/flags/:type/:id', requireApiAuth, (req, res) => {
    const type = req.params.type;
    if (type !== 'manuscript' && type !== 'comment') return err(res, 400, 'type must be manuscript or comment.');
    const targetId = parseInt(req.params.id, 10);
    if (!targetId) return err(res, 400, 'Bad target id.');
    const reason = (req.body && req.body.reason ? String(req.body.reason) : '').trim().slice(0, 1000);
    if (!reason || reason.length < 5) return err(res, 422, 'reason is required (≥ 5 characters).');
    const table = type === 'manuscript' ? 'manuscripts' : 'comments';
    const exists = db.prepare(`SELECT 1 FROM ${table} WHERE id = ?`).get(targetId);
    if (!exists) return err(res, 404, type + ' not found.');
    try {
      db.prepare('INSERT INTO flag_reports (target_type, target_id, reporter_id, reason) VALUES (?, ?, ?, ?)')
        .run(type, targetId, req.user.id, reason);
    } catch (e) {
      if (/UNIQUE/.test(e.message)) {
        // idempotent — caller's flag is already on the queue
        return res.json({ ok: true, already_flagged: true });
      }
      throw e;
    }
    res.json({ ok: true });
  });

  router.get('/admin/flags', requireApiAdmin, (req, res) => {
    const flags = db.prepare(`
      SELECT f.*, u.username AS reporter_username
      FROM flag_reports f JOIN users u ON u.id = f.reporter_id
      WHERE f.resolved = 0
      ORDER BY f.created_at DESC LIMIT 200
    `).all();
    res.json(flags);
  });

  router.post('/admin/flags/:id/resolve', requireApiAdmin, (req, res) => {
    const id = parseInt(req.params.id, 10);
    if (!id) return err(res, 400, 'Bad flag id.');
    const note = (req.body && req.body.note ? String(req.body.note) : '').trim().slice(0, 500);
    const f = db.prepare('SELECT id FROM flag_reports WHERE id = ?').get(id);
    if (!f) return err(res, 404, 'Flag not found.');
    db.prepare(`
      UPDATE flag_reports SET resolved = 1, resolved_by_id = ?, resolved_at = CURRENT_TIMESTAMP, resolution_note = ?
      WHERE id = ?
    `).run(req.user.id, note || null, id);
    res.json({ ok: true });
  });

  // ─── discovery ────────────────────────────────────────────────────────────
  router.get('/categories', (_req, res) => res.json(CATEGORIES));

  router.get('/search', (req, res) => {
    const q = (req.query.q || '').trim();
    const items = [];
    const seen = new Set();
    if (q) {
      const idMatches = db.prepare(`
        SELECT m.*, u.username AS submitter_username, u.display_name AS submitter_display
        FROM manuscripts m JOIN users u ON u.id = m.submitter_id
        WHERE m.arxiv_like_id = ? OR m.doi = ? OR m.arxiv_like_id LIKE ? OR m.doi LIKE ?
        LIMIT 20
      `).all(q, q, q + '%', q + '%');
      for (const r of idMatches) if (!seen.has(r.id)) { seen.add(r.id); items.push(r); }

      const ftsQ = escapeFtsQuery(q);
      if (ftsQ) {
        try {
          const ftsRows = db.prepare(`
            SELECT m.*, u.username AS submitter_username, u.display_name AS submitter_display
            FROM manuscripts m
            JOIN users u ON u.id = m.submitter_id
            JOIN manuscripts_fts fts ON fts.rowid = m.id
            WHERE manuscripts_fts MATCH ?
            ORDER BY rank
            LIMIT 100
          `).all(ftsQ);
          for (const r of ftsRows) if (!seen.has(r.id)) { seen.add(r.id); items.push(r); }
        } catch (_e) { /* ignore bad fts query */ }
      }
    }
    const adminFlag = isAdmin(req.user);
    res.json({ q, items: items.map(m => redactManuscript(m, req.user, adminFlag)) });
  });

  router.get('/openapi.json', (req, res) => {
    const proto = req.get('x-forwarded-proto') || (req.secure ? 'https' : 'http');
    const host = req.get('host');
    const base = (process.env.APP_URL || '').replace(/\/+$/, '') || (proto + '://' + host);
    res.type('application/json').send(JSON.stringify(buildOpenApi(base), null, 2));
  });

  // ─── final API-level error handler ────────────────────────────────────────
  router.use((req, res) => err(res, 404, 'No such API endpoint.'));
  // eslint-disable-next-line no-unused-vars
  router.use((e, req, res, _next) => {
    console.error('[api]', e);
    err(res, 500, 'Internal server error.');
  });

  return router;
}

module.exports = { buildApiRouter };
