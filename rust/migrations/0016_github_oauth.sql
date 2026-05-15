ALTER TABLE users ADD COLUMN github_oauth_verified INTEGER NOT NULL DEFAULT 0;
ALTER TABLE users ADD COLUMN github_oauth_verified_at TEXT;
ALTER TABLE users ADD COLUMN github_id TEXT;
ALTER TABLE users ADD COLUMN github_login TEXT;

CREATE UNIQUE INDEX IF NOT EXISTS idx_users_github_id_unique
  ON users(github_id)
  WHERE github_id IS NOT NULL AND github_id <> '';
