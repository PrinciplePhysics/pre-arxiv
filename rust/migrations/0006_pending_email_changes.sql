-- Pending email-change requests.
--
-- A logged-in user requests an email change; we don't immediately mutate
-- users.email — instead we mint a token and email the NEW address a
-- confirmation link. Only when the user follows that link do we replace
-- users.email and set email_verified=1. This ensures the user actually
-- controls the address they typed (vs. a typo / fat-finger), and that
-- email-based account recovery continues to work afterwards.
--
-- One pending change per user (mint deletes prior). 24-hour TTL — same
-- as initial-verification tokens.

CREATE TABLE pending_email_changes (
    id           INTEGER PRIMARY KEY AUTOINCREMENT,
    user_id      INTEGER NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    new_email    TEXT NOT NULL,
    token_hash   TEXT NOT NULL UNIQUE,
    created_at   DATETIME DEFAULT CURRENT_TIMESTAMP,
    expires_at   DATETIME NOT NULL
);

CREATE INDEX idx_pec_user ON pending_email_changes(user_id);
