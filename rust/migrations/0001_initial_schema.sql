-- Initial schema, mirrored from the JS app's db.js so the same SQLite file
-- can be served by both apps during the transition. Every statement is
-- idempotent (IF NOT EXISTS) — on a DB already created by the JS app this
-- migration is a no-op.

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

CREATE TABLE IF NOT EXISTS follows (
  follower_id INTEGER NOT NULL REFERENCES users(id) ON DELETE CASCADE,
  followee_id INTEGER NOT NULL REFERENCES users(id) ON DELETE CASCADE,
  created_at  DATETIME DEFAULT CURRENT_TIMESTAMP,
  PRIMARY KEY (follower_id, followee_id),
  CHECK (follower_id != followee_id)
);
CREATE INDEX IF NOT EXISTS idx_follows_followee ON follows(followee_id);

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
