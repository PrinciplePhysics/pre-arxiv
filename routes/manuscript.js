// Manuscript routes: submit / edit / view / withdraw / delete / cite.
//
// The submit + edit handlers are the largest in the app: multer parses an
// optional PDF upload, then the parsed-body CSRF check runs, then we validate
// the manuscript values, optionally extract PDF text, and finally insert
// (or update) the row. Zenodo deposition is best-effort and asynchronous —
// if the token is set we mint a real DOI in the background and patch the row.

const fs = require('fs');
const path = require('path');
const { db } = require('../db');
const { requireAuth } = require('../lib/auth');
const { makeArxivLikeId } = require('../lib/util');
const { renderBibtex, renderRis, renderPlain } = require('../lib/citation');
const zenodo = require('../lib/zenodo');

/**
 * @typedef {object} ManuscriptDeps
 * @property {import('multer').Multer} upload
 * @property {import('express').RequestHandler} submitLimiter
 * @property {import('express').RequestHandler} commentLimiter
 * @property {import('express').RequestHandler} requireVerified
 * @property {import('express').RequestHandler} requireAdmin
 * @property {import('express').RequestHandler} csrfCheckParsed
 * @property {(user:any) => boolean} isAdmin
 * @property {(req:any, type:'ok'|'error', msg:string) => void} flash
 * @property {(userId:number|undefined, type:'manuscript'|'comment', ids:number[]) => Record<number, number>} buildVoteMap
 * @property {(req:any) => Record<string, any>} parseManuscriptValues
 * @property {(v:Record<string, any>, opts?:{editing?:boolean}) => string[]} validateManuscriptValues
 * @property {(filepath:string) => Promise<string|null>} extractPdfText
 * @property {(filePath:string) => string|null} verifyUploadedPdf
 * @property {(publicPath:string|null|undefined) => string|null} uploadedFileFsPath
 * @property {(arxivLikeId:string) => string} makeSyntheticDoi
 * @property {(req:any, res:any, isAdminFn:(u:any)=>boolean) => any} fetchEditableManuscript
 * @property {(manuscriptId:number, diffSummary?:string|null) => void} [snapshotManuscriptVersion]
 * @property {(manuscriptId:number) => number} [currentManuscriptVersionNumber]
 * @property {(manuscriptId:number) => any[]} [listVersions]
 * @property {(a:string, b:string, fromLabel:string, toLabel:string) => string} [unifiedDiff]
 * @property {(event:string, payload:any) => void} [safeEmit]
 * @property {(m:any) => any} [manuscriptWebhookPayload]
 * @property {(req:any, action:string, type:string|null, id:number|null, detail:string|null) => void} [auditLog]
 */

/**
 * Register manuscript-detail routes.
 *
 * Routes registered:
 *   GET  /submit             — submission form (verified email required)
 *   POST /submit             — multipart, validates + inserts + Zenodo deposit
 *   GET  /m/:id              — manuscript detail page (with comment tree)
 *   GET  /m/:id/edit         — edit form (owner or admin only)
 *   POST /m/:id/edit         — multipart, validates + updates
 *   POST /m/:id/withdraw     — owner or admin: mark withdrawn (tombstoned)
 *   POST /m/:id/delete       — admin: hard delete + remove PDF
 *   GET  /m/:id/cite         — citation page
 *   GET  /m/:id/cite.bib     — raw BibTeX
 *   GET  /m/:id/cite.ris     — raw RIS
 *
 * @param {import('express').Application} app
 * @param {ManuscriptDeps} deps
 */
function register(app, deps) {
  const {
    upload, submitLimiter, requireVerified, csrfCheckParsed,
    isAdmin, flash, buildVoteMap,
    parseManuscriptValues, validateManuscriptValues,
    extractPdfText, verifyUploadedPdf, uploadedFileFsPath,
    makeSyntheticDoi, fetchEditableManuscript,
    snapshotManuscriptVersion, listVersions, currentManuscriptVersionNumber, unifiedDiff,
    safeEmit, manuscriptWebhookPayload, auditLog,
  } = deps;
  const noopSnapshot = (/** @type {any} */ _id, /** @type {any} */ _summary) => {};
  const snapshot = snapshotManuscriptVersion || noopSnapshot;
  const emit = safeEmit || ((/** @type {string} */ _e, /** @type {any} */ _p) => {});
  const wbpayload = manuscriptWebhookPayload || ((/** @type {any} */ m) => m);
  const audit = auditLog || ((/** @type {any} */ _r, /** @type {any} */ _a, /** @type {any} */ _t, /** @type {any} */ _i, /** @type {any} */ _d) => {});

  // ─── /submit ──────────────────────────────────────────────────────────────
  app.get('/submit', requireVerified, (_req, res) => {
    res.render('submit', { values: {}, errors: [] });
  });

  app.post('/submit', submitLimiter, requireVerified, (req, res, next) => {
    upload.single('pdf')(req, res, (/** @type {Error|undefined} */ err) => {
      if (err) {
        flash(req, 'error', err.message || 'Upload failed.');
        return res.redirect('/submit');
      }
      next();
    });
  }, csrfCheckParsed, async (req, res) => {
    const v = parseManuscriptValues(req);
    const errors = validateManuscriptValues(v);
    if (!req.file && !v.external_url) {
      errors.push('Provide either a PDF upload or an external URL to the manuscript.');
    }
    if (req.file) {
      const pdfErr = verifyUploadedPdf(req.file.path);
      if (pdfErr) errors.push(pdfErr);
    }
    if (errors.length) {
      if (req.file) fs.unlink(req.file.path, () => {});
      return res.render('submit', { values: v, errors });
    }

    const arxivId = makeArxivLikeId();
    let   doi     = makeSyntheticDoi(arxivId); // may be replaced by Zenodo below
    const pdf_path = req.file ? '/uploads/' + path.basename(req.file.path) : null;

    /** @type {string|null} */
    let pdf_text = null;
    if (req.file) {
      pdf_text = await extractPdfText(req.file.path);
    }

    const r = db.prepare(`
      INSERT INTO manuscripts (
        arxiv_like_id, doi, submitter_id, title, abstract, authors, category, pdf_path, pdf_text, external_url,
        conductor_type, conductor_ai_model, conductor_ai_model_public,
        conductor_human, conductor_human_public, conductor_role, conductor_notes, agent_framework,
        has_auditor, auditor_name, auditor_affiliation, auditor_role, auditor_statement, auditor_orcid,
        score
      ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, 1)
    `).run(
      arxivId, doi, req.user.id, v.title, v.abstract, v.authors, v.category, pdf_path, pdf_text, v.external_url,
      v.conductor_type,
      v.conductor_ai_model,
      v.conductor_ai_model_private ? 0 : 1,
      v.conductor_type === 'human-ai' ? v.conductor_human : null,
      v.conductor_human_private ? 0 : 1,
      v.conductor_type === 'human-ai' ? v.conductor_role  : null,
      v.conductor_notes,
      v.conductor_type === 'ai-agent' ? v.agent_framework : null,
      v.has_auditor ? 1 : 0,
      v.has_auditor ? v.auditor_name : null,
      v.has_auditor ? (v.auditor_affiliation || null) : null,
      v.has_auditor ? v.auditor_role : null,
      v.has_auditor ? v.auditor_statement : null,
      v.has_auditor ? (v.auditor_orcid || null) : null,
    );
    // self-upvote
    db.prepare("INSERT INTO votes (user_id, target_type, target_id, value) VALUES (?, 'manuscript', ?, 1)")
      .run(req.user.id, r.lastInsertRowid);

    // Initial version snapshot — every successful submission writes version=1
    // into manuscript_versions so the version history is well-defined from the
    // start. Edits append further snapshots (see /m/:id/edit).
    snapshot(/** @type {number} */ (r.lastInsertRowid), 'Initial submission.');

    // Webhook fan-out (fire-and-forget).
    {
      const fresh = db.prepare(`
        SELECT m.*, u.username AS submitter_username
        FROM manuscripts m JOIN users u ON u.id = m.submitter_id
        WHERE m.id = ?
      `).get(r.lastInsertRowid);
      emit('manuscript.created', wbpayload(fresh));
    }

    // Best-effort Zenodo deposition.
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
          console.log(`[zenodo] minted ${zr.doi} (${zr.sandbox ? 'sandbox' : 'production'}) for ${arxivId}`);
        }
      }).catch(() => {});
    }

    flash(req, 'ok', 'Manuscript posted as ' + arxivId + '.');
    res.redirect('/m/' + arxivId);
  });

  // ─── /m/:id/edit ──────────────────────────────────────────────────────────
  app.get('/m/:id/edit', requireAuth, (req, res) => {
    const m = fetchEditableManuscript(req, res, isAdmin);
    if (!m) return;
    const values = {
      title: m.title, abstract: m.abstract, authors: m.authors, category: m.category,
      external_url: m.external_url || '',
      conductor_type: m.conductor_type,
      conductor_ai_model: m.conductor_ai_model,
      conductor_human: m.conductor_human || '',
      conductor_role: m.conductor_role || '',
      conductor_notes: m.conductor_notes || '',
      agent_framework: m.agent_framework || '',
      conductor_ai_model_private: m.conductor_ai_model_public === 0,
      conductor_human_private:    m.conductor_human_public    === 0,
      has_auditor: !!m.has_auditor,
      auditor_name: m.auditor_name || '',
      auditor_affiliation: m.auditor_affiliation || '',
      auditor_role: m.auditor_role || '',
      auditor_statement: m.auditor_statement || '',
      auditor_orcid: m.auditor_orcid || '',
    };
    res.render('submit', { values, errors: [], editing: true, m });
  });

  app.post('/m/:id/edit', submitLimiter, requireAuth, (req, res, next) => {
    upload.single('pdf')(req, res, (/** @type {Error|undefined} */ err) => {
      if (err) {
        flash(req, 'error', err.message || 'Upload failed.');
        return res.redirect('/m/' + req.params.id + '/edit');
      }
      next();
    });
  }, csrfCheckParsed, async (req, res) => {
    const m = fetchEditableManuscript(req, res, isAdmin);
    if (!m) {
      if (req.file) fs.unlink(req.file.path, () => {});
      return;
    }
    const v = parseManuscriptValues(req);
    const errors = validateManuscriptValues(v, { editing: true });
    if (!req.file && !v.external_url && !m.pdf_path && !m.external_url) {
      errors.push('Provide either a PDF upload or an external URL to the manuscript.');
    }
    if (req.file) {
      const pdfErr = verifyUploadedPdf(req.file.path);
      if (pdfErr) errors.push(pdfErr);
    }
    if (errors.length) {
      if (req.file) fs.unlink(req.file.path, () => {});
      return res.render('submit', { values: v, errors, editing: true, m });
    }

    let pdf_path = m.pdf_path;
    let pdf_text = m.pdf_text;
    if (req.file) {
      pdf_path = '/uploads/' + path.basename(req.file.path);
      pdf_text = await extractPdfText(req.file.path);
      if (m.pdf_path) {
        const old = uploadedFileFsPath(m.pdf_path);
        if (old) fs.unlink(old, () => {});
      }
    }

    // Snapshot the CURRENT (pre-edit) row into manuscript_versions BEFORE
    // applying the update.
    const diffSummary = (req.body.diff_summary == null ? '' : String(req.body.diff_summary)).trim().slice(0, 500) || null;
    snapshot(m.id, diffSummary);

    db.prepare(`
      UPDATE manuscripts SET
        title = ?, abstract = ?, authors = ?, category = ?,
        pdf_path = ?, pdf_text = ?, external_url = ?,
        conductor_type = ?, conductor_ai_model = ?, conductor_ai_model_public = ?,
        conductor_human = ?, conductor_human_public = ?, conductor_role = ?,
        conductor_notes = ?, agent_framework = ?,
        has_auditor = ?, auditor_name = ?, auditor_affiliation = ?, auditor_role = ?, auditor_statement = ?,
        auditor_orcid = ?,
        updated_at = CURRENT_TIMESTAMP
      WHERE id = ?
    `).run(
      v.title, v.abstract, v.authors, v.category,
      pdf_path, pdf_text, v.external_url,
      v.conductor_type, v.conductor_ai_model, v.conductor_ai_model_private ? 0 : 1,
      v.conductor_type === 'human-ai' ? v.conductor_human : null,
      v.conductor_human_private ? 0 : 1,
      v.conductor_type === 'human-ai' ? v.conductor_role : null,
      v.conductor_notes, v.conductor_type === 'ai-agent' ? v.agent_framework : null,
      v.has_auditor ? 1 : 0,
      v.has_auditor ? v.auditor_name : null,
      v.has_auditor ? (v.auditor_affiliation || null) : null,
      v.has_auditor ? v.auditor_role : null,
      v.has_auditor ? v.auditor_statement : null,
      v.has_auditor ? (v.auditor_orcid || null) : null,
      m.id
    );

    // Webhook fan-out (fire-and-forget).
    {
      const fresh = db.prepare(`
        SELECT m.*, u.username AS submitter_username
        FROM manuscripts m JOIN users u ON u.id = m.submitter_id
        WHERE m.id = ?
      `).get(m.id);
      emit('manuscript.updated', wbpayload(fresh));
    }

    flash(req, 'ok', 'Manuscript updated.');
    res.redirect('/m/' + m.arxiv_like_id);
  });

  // ─── /m/:id ───────────────────────────────────────────────────────────────
  app.get('/m/:id', (req, res) => {
    const m = /** @type {any} */ (
      db.prepare(`
        SELECT m.*, u.username AS submitter_username, u.display_name AS submitter_display
        FROM manuscripts m JOIN users u ON u.id = m.submitter_id
        WHERE m.arxiv_like_id = ? OR m.id = ?
      `).get(req.params.id, req.params.id)
    );
    if (!m) return res.status(404).render('error', { code: 404, msg: 'Manuscript not found.' });

    db.prepare('UPDATE manuscripts SET view_count = view_count + 1 WHERE id = ?').run(m.id);

    const comments = /** @type {{id:number, parent_id:number|null, children?:any[]}[]} */ (
      db.prepare(`
        SELECT c.*, u.username, u.display_name FROM comments c
        JOIN users u ON u.id = c.author_id
        WHERE c.manuscript_id = ?
        ORDER BY c.created_at ASC
      `).all(m.id)
    );

    /** @type {Record<number, any>} */
    const byId = {};
    for (const c of comments) { c.children = []; byId[c.id] = c; }
    /** @type {any[]} */
    const top = [];
    for (const c of comments) {
      if (c.parent_id && byId[c.parent_id]) byId[c.parent_id].children.push(c);
      else top.push(c);
    }

    const myMsVote = /** @type {any} */ (req).user
      ? /** @type {{value:number}|undefined} */ (
          db.prepare("SELECT value FROM votes WHERE user_id = ? AND target_type = 'manuscript' AND target_id = ?")
            .get(req.user.id, m.id)
        )?.value ?? null
      : null;
    const cVoteMap   = req.user ? buildVoteMap(req.user.id, 'comment', comments.map(c => c.id)) : {};

    res.render('manuscript', { m, comments: top, allComments: comments, myMsVote, cVoteMap });
  });

  // ─── /m/:id/withdraw and /delete ──────────────────────────────────────────
  app.post('/m/:id/withdraw', requireAuth, (req, res) => {
    const m = /** @type {{id:number, submitter_id:number, arxiv_like_id:string}|undefined} */ (
      db.prepare('SELECT id, submitter_id, arxiv_like_id FROM manuscripts WHERE arxiv_like_id = ? OR id = ?').get(req.params.id, req.params.id)
    );
    if (!m) return res.status(404).render('error', { code: 404, msg: 'Manuscript not found.' });
    const allowed = (m.submitter_id === req.user.id) || isAdmin(req.user);
    if (!allowed) return res.status(403).render('error', { code: 403, msg: 'You can only withdraw your own manuscripts.' });
    const reason = (req.body.reason || '').trim().slice(0, 500) || 'No reason given.';
    db.prepare(`UPDATE manuscripts SET withdrawn = 1, withdrawn_reason = ?, withdrawn_at = CURRENT_TIMESTAMP WHERE id = ?`)
      .run(reason, m.id);
    // Audit only admin-driven withdrawals.
    if (m.submitter_id !== req.user.id && isAdmin(req.user)) {
      audit(req, 'manuscript_withdraw_admin', 'manuscript', m.id, m.arxiv_like_id + ' :: ' + reason);
    }
    // Webhook fan-out.
    {
      const fresh = db.prepare(`
        SELECT m.*, u.username AS submitter_username
        FROM manuscripts m JOIN users u ON u.id = m.submitter_id
        WHERE m.id = ?
      `).get(m.id);
      emit('manuscript.withdrawn', { ...wbpayload(fresh), withdrawn_reason: reason });
    }
    flash(req, 'ok', 'Manuscript withdrawn. The page now shows a tombstone.');
    res.redirect('/m/' + req.params.id);
  });

  app.post('/m/:id/delete', deps.requireAdmin, (req, res) => {
    const m = /** @type {{id:number, pdf_path:string|null, arxiv_like_id:string, withdrawn:number, created_at:string, title:string}|undefined} */ (
      db.prepare(`
        SELECT id, pdf_path, arxiv_like_id, withdrawn, created_at, title
        FROM manuscripts WHERE arxiv_like_id = ? OR id = ?
      `).get(req.params.id, req.params.id)
    );
    if (!m) return res.status(404).render('error', { code: 404, msg: 'Manuscript not found.' });

    // Withdrawal-first protection. If the manuscript is older than 24 h or
    // PREXIV_PRETEND_CITATIONS=1, refuse hard-delete and convert to a withdraw.
    // Admins can override with ?force=1 (also _force=1 in body).
    const force = String(req.query.force || req.body.force || req.body._force || '') === '1';
    const ageMs = Date.now() - new Date(m.created_at + (String(m.created_at).endsWith('Z') ? '' : 'Z')).getTime();
    const olderThan24h = ageMs > 24 * 60 * 60 * 1000;
    const pretendCitations = process.env.PREXIV_PRETEND_CITATIONS === '1';

    if (!force && (olderThan24h || pretendCitations)) {
      const reason = 'Withdrawn at admin request (placeholder).';
      db.prepare(`UPDATE manuscripts SET withdrawn = 1, withdrawn_reason = ?, withdrawn_at = CURRENT_TIMESTAMP WHERE id = ?`)
        .run(reason, m.id);
      audit(req, 'admin_delete_converted_to_withdraw', 'manuscript', m.id,
        JSON.stringify({ arxiv_like_id: m.arxiv_like_id, age_ms: ageMs, pretendCitations }));
      flash(req, 'ok', 'Hard-delete refused (older than 24 h or has citations). Converted to a withdrawal instead. Append ?force=1 to bypass.');
      return res.redirect('/m/' + m.arxiv_like_id);
    }

    audit(req, 'admin_force_delete_manuscript', 'manuscript', m.id,
      JSON.stringify({ arxiv_like_id: m.arxiv_like_id, title: m.title, age_ms: ageMs }));
    if (m.pdf_path) {
      const p = uploadedFileFsPath(m.pdf_path);
      if (p) fs.unlink(p, () => {});
    }
    db.prepare('DELETE FROM manuscripts WHERE id = ?').run(m.id);
    flash(req, 'ok', 'Manuscript permanently deleted (force=1).');
    res.redirect('/');
  });

  // ─── /m/:id/versions — public version history with diffs ─────────────────
  app.get('/m/:id/versions', (req, res) => {
    const m = /** @type {any} */ (
      db.prepare(`
        SELECT m.*, u.username AS submitter_username, u.display_name AS submitter_display
        FROM manuscripts m JOIN users u ON u.id = m.submitter_id
        WHERE m.arxiv_like_id = ? OR m.id = ?
      `).get(req.params.id, req.params.id)
    );
    if (!m) return res.status(404).render('error', { code: 404, msg: 'Manuscript not found.' });

    if (!listVersions || !unifiedDiff || !currentManuscriptVersionNumber) {
      // Versions library unavailable — render an empty history rather than crash.
      return res.render('versions', { m, versions: [], diffs: {}, currentVersion: 1 });
    }
    const versions = listVersions(m.id);
    /** @type {Record<number, string>} */
    const diffs = {};
    for (let i = 0; i < versions.length - 1; i++) {
      const cur = versions[i];
      const prev = versions[i + 1];
      const a = (prev.title || '') + '\n\n' + (prev.abstract || '');
      const b = (cur.title  || '') + '\n\n' + (cur.abstract  || '');
      diffs[cur.version] = unifiedDiff(a, b, 'v' + prev.version, 'v' + cur.version);
    }
    res.render('versions', {
      m,
      versions,
      diffs,
      currentVersion: currentManuscriptVersionNumber(m.id),
    });
  });

  // ─── /m/:id/cite[.bib|.ris] ───────────────────────────────────────────────
  /**
   * @param {string|number} idOrSlug
   * @returns {any}
   */
  function getManuscriptForCite(idOrSlug) {
    return db.prepare(`
      SELECT m.*, u.username AS submitter_username, u.display_name AS submitter_display
      FROM manuscripts m JOIN users u ON u.id = m.submitter_id
      WHERE m.arxiv_like_id = ? OR m.id = ?
    `).get(idOrSlug, idOrSlug);
  }
  /**
   * @param {import('express').Request} req
   * @returns {string}
   */
  function citeBaseUrl(req) {
    if (process.env.APP_URL) return process.env.APP_URL.replace(/\/+$/, '');
    const proto = req.get('x-forwarded-proto') || (req.secure ? 'https' : 'http');
    return proto + '://' + req.get('host');
  }
  app.get('/m/:id/cite', (req, res) => {
    const m = getManuscriptForCite(req.params.id);
    if (!m) return res.status(404).render('error', { code: 404, msg: 'Manuscript not found.' });
    const base = citeBaseUrl(req);
    res.render('cite', {
      m,
      bib:   renderBibtex(m, base),
      ris:   renderRis(m, base),
      plain: renderPlain(m, base),
    });
  });
  app.get('/m/:id/cite.bib', (req, res) => {
    const m = getManuscriptForCite(req.params.id);
    if (!m) return res.status(404).type('text/plain').send('not found');
    res.type('application/x-bibtex').send(renderBibtex(m, citeBaseUrl(req)));
  });
  app.get('/m/:id/cite.ris', (req, res) => {
    const m = getManuscriptForCite(req.params.id);
    if (!m) return res.status(404).type('text/plain').send('not found');
    res.type('application/x-research-info-systems').send(renderRis(m, citeBaseUrl(req)));
  });
}

module.exports = { register };
