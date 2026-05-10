const Database = require('better-sqlite3');
const path = require('path');
const fs = require('fs');

const DB_DIR = process.env.DATA_DIR || path.join(__dirname, 'data');
if (!fs.existsSync(DB_DIR)) fs.mkdirSync(DB_DIR, { recursive: true });

const db = new Database(path.join(DB_DIR, 'prearxiv.db'));
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
`);

// ─── lightweight migrations for databases created on earlier schemas ────────
function safeAlter(stmt) {
  try { db.exec(stmt); } catch (e) { if (!/duplicate column/i.test(e.message)) throw e; }
}
safeAlter(`ALTER TABLE users ADD COLUMN email_verified         INTEGER NOT NULL DEFAULT 0`);
safeAlter(`ALTER TABLE users ADD COLUMN email_verify_token     TEXT`);
safeAlter(`ALTER TABLE users ADD COLUMN email_verify_expires   INTEGER`);
safeAlter(`ALTER TABLE users ADD COLUMN password_reset_token   TEXT`);
safeAlter(`ALTER TABLE users ADD COLUMN password_reset_expires INTEGER`);
safeAlter(`ALTER TABLE users ADD COLUMN is_admin INTEGER NOT NULL DEFAULT 0`);
safeAlter(`ALTER TABLE manuscripts ADD COLUMN doi              TEXT`);
safeAlter(`ALTER TABLE manuscripts ADD COLUMN pdf_text         TEXT`);
safeAlter(`ALTER TABLE manuscripts ADD COLUMN withdrawn        INTEGER NOT NULL DEFAULT 0`);
safeAlter(`ALTER TABLE manuscripts ADD COLUMN withdrawn_reason TEXT`);
safeAlter(`ALTER TABLE manuscripts ADD COLUMN withdrawn_at     DATETIME`);
safeAlter(`ALTER TABLE manuscripts ADD COLUMN conductor_type            TEXT NOT NULL DEFAULT 'human-ai'`);
safeAlter(`ALTER TABLE manuscripts ADD COLUMN agent_framework           TEXT`);
safeAlter(`ALTER TABLE manuscripts ADD COLUMN conductor_ai_model_public INTEGER NOT NULL DEFAULT 1`);
safeAlter(`ALTER TABLE manuscripts ADD COLUMN conductor_human_public    INTEGER NOT NULL DEFAULT 1`);

// ─── relax NOT NULL on conductor_human / conductor_role ──────────────────────
// Old schema had both as NOT NULL. AI-agent manuscripts have neither, so we
// rebuild the table without those constraints (idempotent — only fires when
// the running DB still has the legacy NOT NULL).
function relaxConductorNotNullIfNeeded() {
  const cols = db.prepare(`PRAGMA table_info(manuscripts)`).all();
  const ch = cols.find(c => c.name === 'conductor_human');
  const cr = cols.find(c => c.name === 'conductor_role');
  if (!ch || !cr) return;
  if (ch.notnull !== 1 && cr.notnull !== 1) return; // already relaxed

  console.log('[migrate] rebuilding manuscripts table to allow AI-agent (no human conductor) submissions…');

  const colNames = cols.map(c => c.name);
  const selectList = colNames.join(', ');

  db.exec(`
    PRAGMA foreign_keys = OFF;
    BEGIN;
    DROP TRIGGER IF EXISTS manuscripts_ai;
    DROP TRIGGER IF EXISTS manuscripts_ad;
    DROP TRIGGER IF EXISTS manuscripts_au;

    CREATE TABLE manuscripts_new (
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
      conductor_type     TEXT NOT NULL DEFAULT 'human-ai'
                         CHECK (conductor_type IN ('human-ai', 'ai-agent')),
      conductor_ai_model TEXT NOT NULL,
      conductor_human    TEXT,
      conductor_role     TEXT,
      conductor_notes    TEXT,
      agent_framework    TEXT,
      has_auditor        INTEGER NOT NULL DEFAULT 0,
      auditor_name       TEXT,
      auditor_affiliation TEXT,
      auditor_role       TEXT,
      auditor_statement  TEXT,
      view_count         INTEGER DEFAULT 0,
      score              INTEGER DEFAULT 0,
      comment_count      INTEGER DEFAULT 0,
      withdrawn          INTEGER NOT NULL DEFAULT 0,
      withdrawn_reason   TEXT,
      withdrawn_at       DATETIME,
      created_at         DATETIME DEFAULT CURRENT_TIMESTAMP,
      updated_at         DATETIME DEFAULT CURRENT_TIMESTAMP
    );

    INSERT INTO manuscripts_new (${selectList})
    SELECT ${selectList} FROM manuscripts;

    DROP TABLE manuscripts;
    ALTER TABLE manuscripts_new RENAME TO manuscripts;

    CREATE INDEX IF NOT EXISTS idx_manuscripts_created ON manuscripts(created_at DESC);
    CREATE INDEX IF NOT EXISTS idx_manuscripts_score   ON manuscripts(score DESC);
    CREATE INDEX IF NOT EXISTS idx_manuscripts_cat     ON manuscripts(category);

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

    COMMIT;
    PRAGMA foreign_keys = ON;
  `);

  // FTS now points at the new manuscripts table by row content; rebuild to be safe.
  db.exec(`INSERT INTO manuscripts_fts(manuscripts_fts) VALUES ('rebuild');`);
}
relaxConductorNotNullIfNeeded();

// ─── promote configured admins ──────────────────────────────────────────────
const ADMIN_USERNAMES = (process.env.ADMIN_USERNAMES || '')
  .split(',').map(s => s.trim()).filter(Boolean);
if (ADMIN_USERNAMES.length) {
  for (const u of ADMIN_USERNAMES) {
    db.prepare('UPDATE users SET is_admin = 1 WHERE username = ?').run(u);
  }
}

// ─── FTS5 over manuscripts (title + abstract + authors + pdf body) ──────────
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

// Backfill synthetic DOIs for legacy rows that were created before the DOI
// column existed. Safe to run repeatedly — only fills NULLs.
db.exec(`
  UPDATE manuscripts
  SET    doi = '10.99999/' || UPPER(arxiv_like_id)
  WHERE  doi IS NULL AND arxiv_like_id IS NOT NULL;
`);

// Rename legacy 'pa.' id prefix to 'prexiv.' to match the brand. Idempotent —
// only matches rows that still have the old prefix.
db.exec(`
  UPDATE manuscripts
  SET arxiv_like_id = 'prexiv.' || SUBSTR(arxiv_like_id, 4)
  WHERE arxiv_like_id LIKE 'pa.%';
  UPDATE manuscripts
  SET doi = '10.99999/PREXIV.' || SUBSTR(doi, LENGTH('10.99999/PA.') + 1)
  WHERE doi LIKE '10.99999/PA.%';
`);

// Backfill FTS for any manuscripts that pre-date the FTS table. We use
// FTS5's built-in 'rebuild' command so the doclist matches the live row
// values exactly — a manual delete-all + reinsert can leave the index in a
// state where subsequent UPDATE triggers fail with "database disk image is
// malformed" because the OLD values referenced by the 'delete' command
// don't match what the index thinks is there.
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
