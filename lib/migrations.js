// Numbered DB migrations.
//
// Each migration is a function that takes the better-sqlite3 database handle
// and applies one discrete, idempotent-ish schema upgrade. We track the
// highest-applied number in a tiny `schema_version` table; runMigrations()
// applies any unapplied steps in order and bumps the version.
//
// Critical invariant: this must NOT corrupt or re-touch a DB that was already
// brought up to the current schema by the old ad-hoc safeAlter() blocks in
// db.js. That includes the existing victoria DB, where every column, every
// rebuilt table, every prefix rename has already been applied informally.
// To handle that case, runMigrations() inspects the live schema first; if all
// features the migrations would add are already present, it just stamps the
// version row at the head and returns without running anything.

const HEAD = 7;

const migrations = {};

// 1. Lightweight column adds on the original users + manuscripts tables.
// Equivalent to the historical ad-hoc safeAlter() block — every ALTER is
// guarded so the migration is idempotent against partially-old DBs.
migrations[1] = (db) => {
  const adds = [
    ['users',       'email_verified',         "INTEGER NOT NULL DEFAULT 0"],
    ['users',       'email_verify_token',     "TEXT"],
    ['users',       'email_verify_expires',   "INTEGER"],
    ['users',       'password_reset_token',   "TEXT"],
    ['users',       'password_reset_expires', "INTEGER"],
    ['users',       'is_admin',               "INTEGER NOT NULL DEFAULT 0"],
    ['manuscripts', 'doi',                    "TEXT"],
    ['manuscripts', 'pdf_text',               "TEXT"],
    ['manuscripts', 'withdrawn',              "INTEGER NOT NULL DEFAULT 0"],
    ['manuscripts', 'withdrawn_reason',       "TEXT"],
    ['manuscripts', 'withdrawn_at',           "DATETIME"],
    ['manuscripts', 'conductor_type',         "TEXT NOT NULL DEFAULT 'human-ai'"],
    ['manuscripts', 'agent_framework',        "TEXT"],
    ['manuscripts', 'conductor_ai_model_public', "INTEGER NOT NULL DEFAULT 1"],
    ['manuscripts', 'conductor_human_public',    "INTEGER NOT NULL DEFAULT 1"],
  ];
  for (const [tbl, col, type] of adds) {
    if (!hasColumn(db, tbl, col)) {
      db.exec(`ALTER TABLE ${tbl} ADD COLUMN ${col} ${type}`);
    }
  }
};

// 2. Relax NOT NULL on conductor_human / conductor_role so AI-agent rows
// (no human conductor) are allowed. Original schema declared both NOT NULL.
// Safe to skip if they're already nullable.
migrations[2] = (db) => {
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
      conductor_ai_model_public INTEGER NOT NULL DEFAULT 1,
      conductor_human    TEXT,
      conductor_human_public    INTEGER NOT NULL DEFAULT 1,
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
    COMMIT;
    PRAGMA foreign_keys = ON;
  `);
  // Triggers + FTS rebuild are taken care of by db.js's CREATE TRIGGER block
  // and the FTS reconciler that runs after runMigrations().
};

// 3. Backfill synthetic DOIs for legacy rows that pre-date the doi column.
// Idempotent — only fills NULLs.
migrations[3] = (db) => {
  db.exec(`
    UPDATE manuscripts
    SET    doi = '10.99999/' || UPPER(arxiv_like_id)
    WHERE  doi IS NULL AND arxiv_like_id IS NOT NULL;
  `);
};

// 4. Brand prefix renames: pa.* → prexiv.* → prexiv:* (and matching DOI
// PA. → PREXIV. → PREXIV: ). Each step matches only the older form so this
// is safe to run repeatedly.
migrations[4] = (db) => {
  db.exec(`
    UPDATE manuscripts
    SET arxiv_like_id = 'prexiv.' || SUBSTR(arxiv_like_id, 4)
    WHERE arxiv_like_id LIKE 'pa.%';
    UPDATE manuscripts
    SET doi = '10.99999/PREXIV.' || SUBSTR(doi, LENGTH('10.99999/PA.') + 1)
    WHERE doi LIKE '10.99999/PA.%';
    UPDATE manuscripts
    SET arxiv_like_id = 'prexiv:' || SUBSTR(arxiv_like_id, LENGTH('prexiv.') + 1)
    WHERE arxiv_like_id LIKE 'prexiv.%';
    UPDATE manuscripts
    SET doi = '10.99999/PREXIV:' || SUBSTR(doi, LENGTH('10.99999/PREXIV.') + 1)
    WHERE doi LIKE '10.99999/PREXIV.%';
  `);
};

// 5. 2FA columns on users (TOTP).
migrations[5] = (db) => {
  if (!hasColumn(db, 'users', 'totp_secret'))  db.exec(`ALTER TABLE users ADD COLUMN totp_secret  TEXT`);
  if (!hasColumn(db, 'users', 'totp_enabled')) db.exec(`ALTER TABLE users ADD COLUMN totp_enabled INTEGER NOT NULL DEFAULT 0`);
};

// 6. Audit log table for moderator/admin actions.
migrations[6] = (db) => {
  db.exec(`
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
  `);
};

// 7. ORCID identifiers on users and on auditor metadata.
migrations[7] = (db) => {
  if (!hasColumn(db, 'users',       'orcid'))         db.exec(`ALTER TABLE users       ADD COLUMN orcid         TEXT`);
  if (!hasColumn(db, 'manuscripts', 'auditor_orcid')) db.exec(`ALTER TABLE manuscripts ADD COLUMN auditor_orcid TEXT`);
};

// ─── helpers ────────────────────────────────────────────────────────────────
function hasColumn(db, table, column) {
  const rows = db.prepare(`PRAGMA table_info(${table})`).all();
  return rows.some(r => r.name === column);
}

function tableExists(db, name) {
  const r = db.prepare(`SELECT name FROM sqlite_master WHERE type='table' AND name=?`).get(name);
  return !!r;
}

// True iff every schema feature each migration adds is already present.
// Used to recognise a legacy DB that was brought up to current state by the
// old ad-hoc safeAlter() blocks: in that case we just stamp the version at
// HEAD and return without re-running anything.
function isFullyCurrent(db) {
  if (!tableExists(db, 'manuscripts') || !tableExists(db, 'users')) return false;
  if (!tableExists(db, 'audit_log')) return false;
  const requiredCols = [
    ['users',       'email_verified'],
    ['users',       'is_admin'],
    ['users',       'password_reset_token'],
    ['users',       'totp_secret'],
    ['users',       'totp_enabled'],
    ['users',       'orcid'],
    ['manuscripts', 'doi'],
    ['manuscripts', 'pdf_text'],
    ['manuscripts', 'withdrawn'],
    ['manuscripts', 'conductor_type'],
    ['manuscripts', 'agent_framework'],
    ['manuscripts', 'conductor_ai_model_public'],
    ['manuscripts', 'conductor_human_public'],
    ['manuscripts', 'auditor_orcid'],
  ];
  for (const [t, c] of requiredCols) if (!hasColumn(db, t, c)) return false;
  // conductor_human / conductor_role must be nullable (migration 2)
  const cols = db.prepare(`PRAGMA table_info(manuscripts)`).all();
  const ch = cols.find(c => c.name === 'conductor_human');
  const cr = cols.find(c => c.name === 'conductor_role');
  if (!ch || !cr) return false;
  if (ch.notnull === 1 || cr.notnull === 1) return false;
  // No rows still using the legacy 'pa.' / 'prexiv.' prefixes (migration 4).
  const legacyArx = db.prepare(
    `SELECT COUNT(*) AS n FROM manuscripts WHERE arxiv_like_id LIKE 'pa.%' OR arxiv_like_id LIKE 'prexiv.%'`
  ).get().n;
  if (legacyArx > 0) return false;
  const legacyDoi = db.prepare(
    `SELECT COUNT(*) AS n FROM manuscripts WHERE doi LIKE '10.99999/PA.%' OR doi LIKE '10.99999/PREXIV.%'`
  ).get().n;
  if (legacyDoi > 0) return false;
  // No rows with a NULL doi where they shouldn't (migration 3 backfill).
  const nullDoi = db.prepare(
    `SELECT COUNT(*) AS n FROM manuscripts WHERE doi IS NULL AND arxiv_like_id IS NOT NULL`
  ).get().n;
  if (nullDoi > 0) return false;
  return true;
}

function getCurrentVersion(db) {
  db.exec(`CREATE TABLE IF NOT EXISTS schema_version (version INTEGER PRIMARY KEY)`);
  const row = db.prepare(`SELECT MAX(version) AS v FROM schema_version`).get();
  return (row && row.v) || 0;
}

function setVersion(db, v) {
  db.prepare(`INSERT OR IGNORE INTO schema_version (version) VALUES (?)`).run(v);
}

function runMigrations(db) {
  const current = getCurrentVersion(db);

  // Fast-path: a legacy DB whose schema is already at HEAD (because the old
  // ad-hoc safeAlter() blocks brought it there) — just stamp and return.
  // We detect this by inspecting the live schema; if every migration's effect
  // is already present, replaying them is at best wasted work and at worst
  // (for table-rebuild migrations) destructive.
  if (current === 0 && isFullyCurrent(db)) {
    setVersion(db, HEAD);
    return { applied: [], stampedAt: HEAD, skippedReason: 'schema-already-current' };
  }

  const applied = [];
  for (let v = current + 1; v <= HEAD; v++) {
    const fn = migrations[v];
    if (!fn) continue;
    try {
      fn(db);
    } catch (e) {
      throw new Error(`Migration ${v} failed: ${e.message}`);
    }
    setVersion(db, v);
    applied.push(v);
  }
  return { applied, stampedAt: HEAD, skippedReason: null };
}

module.exports = { runMigrations, HEAD };
