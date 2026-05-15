ALTER TABLE users
  ADD COLUMN IF NOT EXISTS github_oauth_verified BIGINT NOT NULL DEFAULT 0;

ALTER TABLE users
  ADD COLUMN IF NOT EXISTS github_oauth_verified_at TIMESTAMP;

ALTER TABLE users
  ADD COLUMN IF NOT EXISTS github_id TEXT;

ALTER TABLE users
  ADD COLUMN IF NOT EXISTS github_login TEXT;

CREATE UNIQUE INDEX IF NOT EXISTS idx_users_github_id_unique
  ON users(github_id)
  WHERE github_id IS NOT NULL AND github_id <> '';
