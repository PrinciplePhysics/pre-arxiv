// Manuscript-version snapshots and a tiny inline unified-diff implementation.
//
// Why inline a diff: the design constraint is "no new npm dependencies", and
// a basic line-level unified diff is small enough to write by hand. We use
// the standard Wagner-Fischer / Myers-style longest-common-subsequence on
// LINES (not characters) and then format the LCS into a unified-diff hunk.
// This is sufficient for human-readable change review of title+abstract; the
// output is not a perfect git/`diff -u` byte-for-byte match but it parses
// fine and is unambiguous.

const { db } = require('../db');

// Snapshot the CURRENT manuscript row (as it stands in DB) into the
// manuscript_versions table. We compute the next version number atomically
// in the same write so concurrent edits don't collide on UNIQUE(manuscript_id, version).
function snapshotManuscriptVersion(manuscriptId, diffSummary) {
  const m = db.prepare(`SELECT * FROM manuscripts WHERE id = ?`).get(manuscriptId);
  if (!m) return null;
  const row = db.prepare(`
    SELECT COALESCE(MAX(version), 0) AS v FROM manuscript_versions WHERE manuscript_id = ?
  `).get(manuscriptId);
  const nextVersion = (row.v || 0) + 1;
  db.prepare(`
    INSERT INTO manuscript_versions (
      manuscript_id, version,
      title, abstract, authors, category, pdf_path, external_url,
      conductor_type, conductor_ai_model, conductor_human, conductor_role,
      conductor_notes, agent_framework,
      has_auditor, auditor_name, auditor_affiliation, auditor_role, auditor_statement,
      diff_summary
    ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
  `).run(
    manuscriptId, nextVersion,
    m.title, m.abstract, m.authors, m.category, m.pdf_path, m.external_url,
    m.conductor_type, m.conductor_ai_model, m.conductor_human, m.conductor_role,
    m.conductor_notes, m.agent_framework,
    m.has_auditor, m.auditor_name, m.auditor_affiliation, m.auditor_role, m.auditor_statement,
    diffSummary || null
  );
  return nextVersion;
}

// The CURRENT manuscript's "logical version" is the count of stored versions
// plus 1 (each successful edit snapshots the CURRENT row before applying;
// the new live state therefore has version-number = N_snapshots + 1).
// Brand-new manuscripts have N_snapshots = 1 (the initial snapshot taken at
// submit time) and so the displayed "live" version is 1, matching the head
// snapshot.
function currentManuscriptVersionNumber(manuscriptId) {
  const row = db.prepare(`
    SELECT COALESCE(MAX(version), 0) AS v FROM manuscript_versions WHERE manuscript_id = ?
  `).get(manuscriptId);
  return row.v || 0;
}

function listVersions(manuscriptId, { full = false } = {}) {
  if (full) {
    return db.prepare(`
      SELECT * FROM manuscript_versions
      WHERE manuscript_id = ?
      ORDER BY version DESC
    `).all(manuscriptId);
  }
  return db.prepare(`
    SELECT id, manuscript_id, version, title, diff_summary, created_at
    FROM manuscript_versions
    WHERE manuscript_id = ?
    ORDER BY version DESC
  `).all(manuscriptId);
}

function getVersion(manuscriptId, version) {
  return db.prepare(`
    SELECT * FROM manuscript_versions WHERE manuscript_id = ? AND version = ?
  `).get(manuscriptId, version);
}

// ─── inline unified-diff ────────────────────────────────────────────────────
// Implements line-level LCS via dynamic programming. For abstract-sized
// inputs (a few hundred lines max) this is plenty fast. Returns a string in
// approximately git-style unified-diff format.
function diffLines(a, b) {
  const A = (a == null ? '' : String(a)).split(/\r?\n/);
  const B = (b == null ? '' : String(b)).split(/\r?\n/);
  const n = A.length, m = B.length;
  // dp[i][j] = LCS length of A[i..] and B[j..]
  // Allocate (n+1) x (m+1)
  const dp = Array.from({ length: n + 1 }, () => new Uint32Array(m + 1));
  for (let i = n - 1; i >= 0; i--) {
    for (let j = m - 1; j >= 0; j--) {
      if (A[i] === B[j]) dp[i][j] = dp[i + 1][j + 1] + 1;
      else                dp[i][j] = Math.max(dp[i + 1][j], dp[i][j + 1]);
    }
  }
  // Walk forward producing edit ops: '=', '-', '+'
  const ops = [];
  let i = 0, j = 0;
  while (i < n && j < m) {
    if (A[i] === B[j]) { ops.push(['=', A[i]]); i++; j++; }
    else if (dp[i + 1][j] >= dp[i][j + 1]) { ops.push(['-', A[i]]); i++; }
    else { ops.push(['+', B[j]]); j++; }
  }
  while (i < n) { ops.push(['-', A[i++]]); }
  while (j < m) { ops.push(['+', B[j++]]); }
  return ops;
}

// Render an array of ops into a unified-diff. We emit hunks of changed
// regions plus 3 lines of context around them, mimicking `diff -U 3`.
function unifiedDiff(a, b, labelA, labelB, context = 3) {
  const ops = diffLines(a, b);
  if (!ops.length) return '';
  const allEqual = ops.every(o => o[0] === '=');
  if (allEqual) return ''; // identical

  const out = [];
  out.push('--- ' + (labelA || 'a'));
  out.push('+++ ' + (labelB || 'b'));

  // Walk ops, emitting hunks. Track absolute line numbers in A (oldLine) and
  // B (newLine), 1-based for unified-diff @@ headers.
  let oldLine = 1, newLine = 1;
  let i = 0;
  const N = ops.length;

  while (i < N) {
    // Skip leading equal runs that aren't context for any change.
    if (ops[i][0] === '=') {
      // Look ahead — is there any change within reach?
      let k = i;
      while (k < N && ops[k][0] === '=') k++;
      if (k === N) break; // tail of all '=' — no more hunks
      const eqRun = k - i;
      if (eqRun > context) {
        // Skip leading '=' lines beyond the context window.
        const skip = eqRun - context;
        for (let p = 0; p < skip; p++) { oldLine++; newLine++; }
        i += skip;
      }
    }
    // Begin a hunk at i.
    const hunkStartOps = i;
    const hunkOldStart = oldLine;
    const hunkNewStart = newLine;
    let hunkLines = [];

    // Consume ops, but greedy-merge if an equal run is short (≤ 2*context),
    // otherwise close the hunk after `context` lines of trailing '='.
    let trailingEq = 0;
    while (i < N) {
      const [op, line] = ops[i];
      if (op === '=') {
        // Look ahead to see how long this equal run is.
        let k = i;
        while (k < N && ops[k][0] === '=') k++;
        const runLen = k - i;
        const lastBeforeEnd = (k === N);
        if (lastBeforeEnd) {
          // emit up to `context` trailing context lines, then stop
          const take = Math.min(context, runLen);
          for (let p = 0; p < take; p++) {
            hunkLines.push([' ', ops[i + p][1]]);
            oldLine++; newLine++;
          }
          i += runLen; // consume the rest silently
          trailingEq = take;
          break;
        }
        if (runLen > 2 * context) {
          // emit `context` trailing context lines, close hunk, continue.
          for (let p = 0; p < context; p++) {
            hunkLines.push([' ', ops[i + p][1]]);
            oldLine++; newLine++;
          }
          // skip the middle equal lines
          const skip = runLen - 2 * context;
          for (let p = 0; p < skip; p++) { oldLine++; newLine++; }
          i += context + skip;
          trailingEq = context;
          break;
        }
        // Short equal run inside a hunk — emit them all as context.
        for (let p = 0; p < runLen; p++) {
          hunkLines.push([' ', ops[i + p][1]]);
          oldLine++; newLine++;
        }
        i += runLen;
        continue;
      }
      if (op === '-') { hunkLines.push(['-', line]); oldLine++; }
      else if (op === '+') { hunkLines.push(['+', line]); newLine++; }
      i++;
      trailingEq = 0;
    }

    if (!hunkLines.length) break;

    // Compute hunk header counts.
    let oldCount = 0, newCount = 0;
    for (const [t] of hunkLines) {
      if (t === ' ') { oldCount++; newCount++; }
      else if (t === '-') { oldCount++; }
      else if (t === '+') { newCount++; }
    }
    out.push('@@ -' + hunkOldStart + ',' + oldCount + ' +' + hunkNewStart + ',' + newCount + ' @@');
    for (const [t, l] of hunkLines) out.push(t + l);
  }
  return out.join('\n');
}

module.exports = {
  snapshotManuscriptVersion,
  currentManuscriptVersionNumber,
  listVersions,
  getVersion,
  unifiedDiff,
  diffLines,
};
