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

  conductor_ai_model TEXT NOT NULL,
  conductor_human    TEXT NOT NULL,
  conductor_role     TEXT NOT NULL,
  conductor_notes    TEXT,

  has_auditor        INTEGER NOT NULL DEFAULT 0,
  auditor_name       TEXT,
  auditor_affiliation TEXT,
  auditor_role       TEXT,
  auditor_statement  TEXT,

  view_count         INTEGER DEFAULT 0,
  score              INTEGER DEFAULT 0,
  comment_count      INTEGER DEFAULT 0,

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
safeAlter(`ALTER TABLE manuscripts ADD COLUMN doi      TEXT`);
safeAlter(`ALTER TABLE manuscripts ADD COLUMN pdf_text TEXT`);

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
