const Database = require('better-sqlite3');
const path = require('path');
const fs = require('fs');
const { runMigrations } = require('./lib/migrations');

const DB_DIR = process.env.DATA_DIR || path.join(__dirname, 'data');
if (!fs.existsSync(DB_DIR)) fs.mkdirSync(DB_DIR, { recursive: true });

// ─── personal-data isolation (opt-in) ───────────────────────────────────────
// Two physical files:
//   data/prearxiv.seed.db  — optional pristine snapshot (created by
//                            `npm run seed`)
//   data/prearxiv.db       — runtime DB; everything users add accrues here
//
// By default the runtime DB persists across restarts so accounts, sessions,
// submissions, votes, comments, etc. are remembered. To opt into the
// "every restart wipes everything that wasn't seeded" behaviour, run with
// PREXIV_WIPE_ON_RESTART=1; the runtime DB is then replaced with a copy
// of prearxiv.seed.db (and sessions.db is cleared) on every server start.
// PREXIV_SKIP_RESET=1 (used internally by seed.js) and PREXIV_PERSIST=1
// (legacy alias) both force-disable the wipe.
const SEED_PATH     = path.join(DB_DIR, 'prearxiv.seed.db');
const RUNTIME_PATH  = path.join(DB_DIR, 'prearxiv.db');
const SESSIONS_PATH = path.join(DB_DIR, 'sessions.db');

const FORCE_PERSIST = process.env.PREXIV_SKIP_RESET === '1' || process.env.PREXIV_PERSIST === '1';
const WIPE_REQUESTED = process.env.PREXIV_WIPE_ON_RESTART === '1';
if (WIPE_REQUESTED && !FORCE_PERSIST && fs.existsSync(SEED_PATH)) {
  for (const p of [
    RUNTIME_PATH, RUNTIME_PATH + '-shm', RUNTIME_PATH + '-wal',
    SESSIONS_PATH, SESSIONS_PATH + '-shm', SESSIONS_PATH + '-wal',
  ]) {
    try { fs.unlinkSync(p); } catch { /* may not exist */ }
  }
  fs.copyFileSync(SEED_PATH, RUNTIME_PATH);
  console.log('[db] runtime DB restored from prearxiv.seed.db (PREXIV_WIPE_ON_RESTART=1)');
}

const db = new Database(RUNTIME_PATH);
db.pragma('journal_mode = WAL');
db.pragma('foreign_keys = ON');

db.exec(`
CREATE TABLE IF NOT EXISTS users (
  id                     INTEGER PRIMARY KEY AUTOINCREMENT,
  username               TEXT UNIQUE NOT NULL,
  email                  TEXT UNIQUE NOT NULL,
  password_hash          TEXT NOT NULL,
  display_name           TEXT,
  affiliation            TEXT,
  bio                    TEXT,
  karma                  INTEGER DEFAULT 0,
  is_admin               INTEGER NOT NULL DEFAULT 0,
  email_verified         INTEGER NOT NULL DEFAULT 0,
  email_verify_token     TEXT,
  email_verify_expires   INTEGER,
  password_reset_token   TEXT,
  password_reset_expires INTEGER,
  totp_secret            TEXT,
  totp_enabled           INTEGER NOT NULL DEFAULT 0,
  orcid                  TEXT,
  created_at             DATETIME DEFAULT CURRENT_TIMESTAMP
);

CREATE TABLE IF NOT EXISTS manuscripts (
  id                 INTEGER PRIMARY KEY AUTOINCREMENT,
  arxiv_like_id      TEXT UNIQUE,
  doi                TEXT UNIQUE,
  submitter_id       INTEGER NOT NULL REFERENCES users(id) ON DELETE CASCADE,
  title              TEXT NOT NULL,
  abstract           TEXT NOT NULL,
  authors            TEXT NOT NULL,
  category           TEXT NOT NULL,
  pdf_path           TEXT,
  pdf_text           TEXT,
  external_url       TEXT,

  -- 'human-ai' (named human conductor + AI co-author) or 'ai-agent' (AI alone, autonomous)
  conductor_type     TEXT NOT NULL DEFAULT 'human-ai'
                     CHECK (conductor_type IN ('human-ai', 'ai-agent')),
  conductor_ai_model TEXT NOT NULL,
  conductor_ai_model_public INTEGER NOT NULL DEFAULT 1,
  conductor_human    TEXT,           -- required only when conductor_type='human-ai'
  conductor_human_public    INTEGER NOT NULL DEFAULT 1,
  conductor_role     TEXT,           -- required only when conductor_type='human-ai'
  conductor_notes    TEXT,
  agent_framework    TEXT,           -- optional; only meaningful for conductor_type='ai-agent'

  has_auditor        INTEGER NOT NULL DEFAULT 0,
  auditor_name       TEXT,
  auditor_affiliation TEXT,
  auditor_role       TEXT,
  auditor_statement  TEXT,
  auditor_orcid      TEXT,

  view_count         INTEGER DEFAULT 0,
  score              INTEGER DEFAULT 0,
  comment_count      INTEGER DEFAULT 0,

  withdrawn          INTEGER NOT NULL DEFAULT 0,
  withdrawn_reason   TEXT,
  withdrawn_at       DATETIME,

  created_at         DATETIME DEFAULT CURRENT_TIMESTAMP,
  updated_at         DATETIME DEFAULT CURRENT_TIMESTAMP
);
CREATE INDEX IF NOT EXISTS idx_manuscripts_created ON manuscripts(created_at DESC);
CREATE INDEX IF NOT EXISTS idx_manuscripts_score   ON manuscripts(score DESC);
CREATE INDEX IF NOT EXISTS idx_manuscripts_cat     ON manuscripts(category);

CREATE TABLE IF NOT EXISTS comments (
  id            INTEGER PRIMARY KEY AUTOINCREMENT,
  manuscript_id INTEGER NOT NULL REFERENCES manuscripts(id) ON DELETE CASCADE,
  author_id     INTEGER NOT NULL REFERENCES users(id) ON DELETE CASCADE,
  parent_id     INTEGER REFERENCES comments(id) ON DELETE CASCADE,
  content       TEXT NOT NULL,
  score         INTEGER DEFAULT 0,
  created_at    DATETIME DEFAULT CURRENT_TIMESTAMP
);
CREATE INDEX IF NOT EXISTS idx_comments_manuscript ON comments(manuscript_id);
CREATE INDEX IF NOT EXISTS idx_comments_parent     ON comments(parent_id);

CREATE TABLE IF NOT EXISTS votes (
  id            INTEGER PRIMARY KEY AUTOINCREMENT,
  user_id       INTEGER NOT NULL REFERENCES users(id) ON DELETE CASCADE,
  target_type   TEXT NOT NULL CHECK(target_type IN ('manuscript','comment')),
  target_id     INTEGER NOT NULL,
  value         INTEGER NOT NULL CHECK(value IN (-1, 1)),
  created_at    DATETIME DEFAULT CURRENT_TIMESTAMP,
  UNIQUE(user_id, target_type, target_id)
);
CREATE INDEX IF NOT EXISTS idx_votes_target ON votes(target_type, target_id);

CREATE TABLE IF NOT EXISTS audit_endorsements (
  id            INTEGER PRIMARY KEY AUTOINCREMENT,
  manuscript_id INTEGER NOT NULL REFERENCES manuscripts(id) ON DELETE CASCADE,
  user_id       INTEGER NOT NULL REFERENCES users(id) ON DELETE CASCADE,
  statement     TEXT NOT NULL,
  created_at    DATETIME DEFAULT CURRENT_TIMESTAMP,
  UNIQUE(manuscript_id, user_id)
);

CREATE TABLE IF NOT EXISTS flag_reports (
  id              INTEGER PRIMARY KEY AUTOINCREMENT,
  target_type     TEXT NOT NULL CHECK(target_type IN ('manuscript','comment')),
  target_id       INTEGER NOT NULL,
  reporter_id     INTEGER NOT NULL REFERENCES users(id) ON DELETE CASCADE,
  reason          TEXT NOT NULL,
  resolved        INTEGER NOT NULL DEFAULT 0,
  resolved_by_id  INTEGER REFERENCES users(id) ON DELETE SET NULL,
  resolved_at     DATETIME,
  resolution_note TEXT,
  created_at      DATETIME DEFAULT CURRENT_TIMESTAMP,
  UNIQUE(target_type, target_id, reporter_id)
);
CREATE INDEX IF NOT EXISTS idx_flags_unresolved ON flag_reports(resolved, created_at DESC);

CREATE TABLE IF NOT EXISTS api_tokens (
  id            INTEGER PRIMARY KEY AUTOINCREMENT,
  user_id       INTEGER NOT NULL REFERENCES users(id) ON DELETE CASCADE,
  token_hash    TEXT NOT NULL UNIQUE,
  name          TEXT,
  last_used_at  DATETIME,
  created_at    DATETIME DEFAULT CURRENT_TIMESTAMP,
  expires_at    DATETIME
);
CREATE INDEX IF NOT EXISTS idx_api_tokens_user ON api_tokens(user_id);

CREATE TABLE IF NOT EXISTS manuscript_versions (
  id            INTEGER PRIMARY KEY AUTOINCREMENT,
  manuscript_id INTEGER NOT NULL REFERENCES manuscripts(id) ON DELETE CASCADE,
  version       INTEGER NOT NULL,
  title         TEXT NOT NULL,
  abstract      TEXT NOT NULL,
  authors       TEXT NOT NULL,
  category      TEXT,
  pdf_path      TEXT,
  external_url  TEXT,
  conductor_type     TEXT,
  conductor_ai_model TEXT,
  conductor_human    TEXT,
  conductor_role     TEXT,
  conductor_notes    TEXT,
  agent_framework    TEXT,
  has_auditor        INTEGER,
  auditor_name       TEXT,
  auditor_affiliation TEXT,
  auditor_role       TEXT,
  auditor_statement  TEXT,
  diff_summary  TEXT,
  created_at    DATETIME DEFAULT CURRENT_TIMESTAMP,
  UNIQUE(manuscript_id, version)
);
CREATE INDEX IF NOT EXISTS idx_versions_manuscript ON manuscript_versions(manuscript_id, version DESC);

-- ─── audit log (moderator/admin actions) ───────────────────────────────────
CREATE TABLE IF NOT EXISTS audit_log (
  id              INTEGER PRIMARY KEY AUTOINCREMENT,
  actor_user_id   INTEGER REFERENCES users(id) ON DELETE SET NULL,
  action          TEXT NOT NULL,
  target_type     TEXT,
  target_id       INTEGER,
  detail          TEXT,
  ip              TEXT,
  created_at      DATETIME DEFAULT CURRENT_TIMESTAMP
);
CREATE INDEX IF NOT EXISTS idx_audit_log_created ON audit_log(created_at DESC);
CREATE INDEX IF NOT EXISTS idx_audit_log_actor   ON audit_log(actor_user_id);

-- ─── notifications (in-app, no email) ───────────────────────────────────────
CREATE TABLE IF NOT EXISTS notifications (
  id            INTEGER PRIMARY KEY AUTOINCREMENT,
  user_id       INTEGER NOT NULL REFERENCES users(id) ON DELETE CASCADE,
  kind          TEXT NOT NULL,
  actor_id      INTEGER REFERENCES users(id) ON DELETE SET NULL,
  manuscript_id INTEGER REFERENCES manuscripts(id) ON DELETE CASCADE,
  comment_id    INTEGER REFERENCES comments(id) ON DELETE CASCADE,
  seen          INTEGER NOT NULL DEFAULT 0,
  created_at    DATETIME DEFAULT CURRENT_TIMESTAMP
);
CREATE INDEX IF NOT EXISTS idx_notif_user ON notifications(user_id, seen, created_at DESC);

-- ─── follows (user → user) ──────────────────────────────────────────────────
CREATE TABLE IF NOT EXISTS follows (
  follower_id INTEGER NOT NULL REFERENCES users(id) ON DELETE CASCADE,
  followee_id INTEGER NOT NULL REFERENCES users(id) ON DELETE CASCADE,
  created_at  DATETIME DEFAULT CURRENT_TIMESTAMP,
  PRIMARY KEY (follower_id, followee_id),
  CHECK (follower_id != followee_id)
);
CREATE INDEX IF NOT EXISTS idx_follows_followee ON follows(followee_id);

-- ─── webhooks (per-user agent subscriptions) ───────────────────────────────
-- The dispatcher in lib/webhooks.js POSTs the JSON envelope
--   { event, ts, payload }
-- to the configured url whenever an event the user subscribed to fires,
-- signed with HMAC-SHA256(secret, body) sent in the X-PreXiv-Signature
-- header. After 5 consecutive failures the dispatcher sets active = 0.
CREATE TABLE IF NOT EXISTS webhooks (
  id               INTEGER PRIMARY KEY AUTOINCREMENT,
  user_id          INTEGER NOT NULL REFERENCES users(id) ON DELETE CASCADE,
  url              TEXT NOT NULL,
  secret           TEXT NOT NULL,
  events           TEXT NOT NULL,
  active           INTEGER NOT NULL DEFAULT 1,
  description      TEXT,
  failure_count    INTEGER NOT NULL DEFAULT 0,
  last_attempt_at  DATETIME,
  last_status      INTEGER,
  created_at       DATETIME DEFAULT CURRENT_TIMESTAMP
);
CREATE INDEX IF NOT EXISTS idx_webhooks_user ON webhooks(user_id);
`);

// Tiny in-place ALTER for legacy DBs that pre-date the `description` column
// on `webhooks`. Idempotent — silently swallows the "duplicate column" error
// that better-sqlite3 raises if the column already exists.
function _safeAlterWebhooks(stmt) {
  try { db.exec(stmt); } catch (e) { if (!/duplicate column/i.test(e.message)) throw e; }
}
_safeAlterWebhooks(`ALTER TABLE webhooks ADD COLUMN description TEXT`);

// ─── FTS5 over manuscripts (title + abstract + authors + pdf body) ──────────
// Created up-front so any later table-rebuild migration has the FTS table to
// rebuild against. Triggers are idempotent (CREATE TRIGGER IF NOT EXISTS) and
// re-installed below after migrations in case migration 2 dropped them as
// part of the manuscripts-table rebuild.
db.exec(`
CREATE VIRTUAL TABLE IF NOT EXISTS manuscripts_fts USING fts5(
  title, abstract, authors, pdf_text,
  content='manuscripts', content_rowid='id', tokenize='porter unicode61'
);

CREATE TRIGGER IF NOT EXISTS manuscripts_ai AFTER INSERT ON manuscripts BEGIN
  INSERT INTO manuscripts_fts(rowid, title, abstract, authors, pdf_text)
  VALUES (new.id, new.title, new.abstract, new.authors, COALESCE(new.pdf_text, ''));
END;
CREATE TRIGGER IF NOT EXISTS manuscripts_ad AFTER DELETE ON manuscripts BEGIN
  INSERT INTO manuscripts_fts(manuscripts_fts, rowid, title, abstract, authors, pdf_text)
  VALUES ('delete', old.id, old.title, old.abstract, old.authors, COALESCE(old.pdf_text, ''));
END;
CREATE TRIGGER IF NOT EXISTS manuscripts_au AFTER UPDATE ON manuscripts BEGIN
  INSERT INTO manuscripts_fts(manuscripts_fts, rowid, title, abstract, authors, pdf_text)
  VALUES ('delete', old.id, old.title, old.abstract, old.authors, COALESCE(old.pdf_text, ''));
  INSERT INTO manuscripts_fts(rowid, title, abstract, authors, pdf_text)
  VALUES (new.id, new.title, new.abstract, new.authors, COALESCE(new.pdf_text, ''));
END;
`);

// ─── numbered migrations ───────────────────────────────────────────────────
// All schema upgrades for legacy DBs (column adds, NOT-NULL relaxation on
// conductor_human/role, DOI backfill, prefix renames, 2FA columns, audit_log,
// ORCID columns) live in lib/migrations.js and are tracked in a tiny
// schema_version table. The runner detects a legacy DB whose schema is
// already at HEAD (e.g. the existing victoria DB, brought there by the
// historical safeAlter() blocks) and stamps it without re-running anything.
const migrationResult = runMigrations(db);
if (migrationResult.applied.length) {
  console.log(`[migrate] applied migrations ${migrationResult.applied.join(', ')} (schema_version=${migrationResult.stampedAt})`);
} else if (migrationResult.skippedReason === 'schema-already-current') {
  console.log(`[migrate] legacy DB already at current schema — stamped schema_version=${migrationResult.stampedAt} without re-running migrations`);
}

// Re-create FTS triggers idempotently in case migration 2 rebuilt the
// manuscripts table (which drops triggers as a side-effect).
db.exec(`
CREATE TRIGGER IF NOT EXISTS manuscripts_ai AFTER INSERT ON manuscripts BEGIN
  INSERT INTO manuscripts_fts(rowid, title, abstract, authors, pdf_text)
  VALUES (new.id, new.title, new.abstract, new.authors, COALESCE(new.pdf_text, ''));
END;
CREATE TRIGGER IF NOT EXISTS manuscripts_ad AFTER DELETE ON manuscripts BEGIN
  INSERT INTO manuscripts_fts(manuscripts_fts, rowid, title, abstract, authors, pdf_text)
  VALUES ('delete', old.id, old.title, old.abstract, old.authors, COALESCE(old.pdf_text, ''));
END;
CREATE TRIGGER IF NOT EXISTS manuscripts_au AFTER UPDATE ON manuscripts BEGIN
  INSERT INTO manuscripts_fts(manuscripts_fts, rowid, title, abstract, authors, pdf_text)
  VALUES ('delete', old.id, old.title, old.abstract, old.authors, COALESCE(old.pdf_text, ''));
  INSERT INTO manuscripts_fts(rowid, title, abstract, authors, pdf_text)
  VALUES (new.id, new.title, new.abstract, new.authors, COALESCE(new.pdf_text, ''));
END;
`);

// ─── promote configured admins ──────────────────────────────────────────────
const ADMIN_USERNAMES = (process.env.ADMIN_USERNAMES || '')
  .split(',').map(s => s.trim()).filter(Boolean);
if (ADMIN_USERNAMES.length) {
  for (const u of ADMIN_USERNAMES) {
    db.prepare('UPDATE users SET is_admin = 1 WHERE username = ?').run(u);
  }
}

// Backfill FTS for any manuscripts whose row count drifts from the FTS index.
// We use FTS5's built-in 'rebuild' command so the doclist matches the live
// row values exactly — a manual delete-all + reinsert can leave the index in
// a state where later UPDATE triggers fail with "database disk image is
// malformed" because the OLD values the 'delete' command references don't
// match what the index thinks is there.
const ftsCount = db.prepare('SELECT COUNT(*) AS n FROM manuscripts_fts').get().n;
const msCount  = db.prepare('SELECT COUNT(*) AS n FROM manuscripts').get().n;
if (ftsCount !== msCount) {
  db.exec(`INSERT INTO manuscripts_fts(manuscripts_fts) VALUES ('rebuild');`);
}

const CATEGORIES = [
  { id: 'cs.AI',      name: 'Artificial Intelligence' },
  { id: 'cs.LG',      name: 'Machine Learning' },
  { id: 'cs.CL',      name: 'Computation & Language' },
  { id: 'cs.CV',      name: 'Computer Vision' },
  { id: 'cs.SE',      name: 'Software Engineering' },
  { id: 'math.AG',    name: 'Algebraic Geometry' },
  { id: 'math.NT',    name: 'Number Theory' },
  { id: 'math.PR',    name: 'Probability' },
  { id: 'math.OC',    name: 'Optimization & Control' },
  { id: 'physics.gen-ph', name: 'General Physics' },
  { id: 'hep-th',     name: 'High Energy Physics — Theory' },
  { id: 'hep-ph',     name: 'High Energy Physics — Phenomenology' },
  { id: 'nucl-th',    name: 'Nuclear Theory' },
  { id: 'cond-mat',   name: 'Condensed Matter' },
  { id: 'astro-ph',   name: 'Astrophysics' },
  { id: 'q-bio',      name: 'Quantitative Biology' },
  { id: 'q-fin',      name: 'Quantitative Finance' },
  { id: 'stat.ML',    name: 'Statistics — Machine Learning' },
  { id: 'econ',       name: 'Economics' },
  { id: 'misc',       name: 'Miscellaneous' },
];

const ROLES = [
  'undergraduate',
  'graduate-student',
  'postdoc',
  'industry-researcher',
  'professor',
  'professional-expert',
  'independent-researcher',
  'hobbyist',
];

module.exports = { db, CATEGORIES, ROLES };
