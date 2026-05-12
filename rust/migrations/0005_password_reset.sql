-- Password reset tokens.
--
-- A user who forgot their password requests a reset; we mint a token,
-- email them a /reset-password/{token} link, and they set a new password
-- by following it. Same shape as email_verification_tokens, but with a
-- much shorter TTL (1 hour, vs 24 for email-verify) — password reset is
-- the higher-value attack surface, so we narrow the redemption window.
--
-- Single-use: the redeem handler deletes the row after a successful set.
-- Pre-existing rows for the same user are also wiped at mint time so a
-- leaked older link can't beat a fresh one.

CREATE TABLE password_reset_tokens (
    id          INTEGER PRIMARY KEY AUTOINCREMENT,
    user_id     INTEGER NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    token_hash  TEXT    NOT NULL UNIQUE,
    created_at  DATETIME DEFAULT CURRENT_TIMESTAMP,
    expires_at  DATETIME NOT NULL
);

CREATE INDEX idx_prt_user ON password_reset_tokens(user_id);
