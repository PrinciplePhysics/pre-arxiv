-- Email verification tokens.
--
-- On register, mint a token, store SHA-256(plaintext) here, and email the
-- plaintext to the user as a /verify/{token} link. When the link is
-- followed, look up sha256(token), check expires_at, set
-- users.email_verified = 1, and delete the row.
--
-- Notes on shape:
--   - token_hash is UNIQUE so a stolen DB doesn't allow a collision attack
--     by re-inserting a hash; also gives us O(log n) lookup.
--   - ON DELETE CASCADE: removing a user removes their pending tokens.
--   - expires_at is required, not nullable; the verify handler must check it.

CREATE TABLE email_verification_tokens (
    id          INTEGER PRIMARY KEY AUTOINCREMENT,
    user_id     INTEGER NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    token_hash  TEXT    NOT NULL UNIQUE,
    created_at  DATETIME DEFAULT CURRENT_TIMESTAMP,
    expires_at  DATETIME NOT NULL
);

CREATE INDEX idx_evt_user ON email_verification_tokens(user_id);
