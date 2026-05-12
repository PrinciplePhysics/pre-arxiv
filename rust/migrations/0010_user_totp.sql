-- Time-based one-time passwords (RFC 6238) for two-factor auth.
--
-- One row per user. `secret` is the base32-encoded TOTP shared key.
-- `enabled_at` is NULL while the user is mid-enrollment (we've issued
-- the secret + QR but they haven't verified the first code yet); set
-- once they submit a valid code.
--
-- Disabling 2FA deletes the row entirely. Re-enabling rotates the
-- secret. Backup codes can be added later as a separate table.
--
-- Threat model note: `secret` is stored plaintext, which is the same
-- as the JS app did. The right fix is column-level encryption with a
-- server-side key (PREXIV_DATA_KEY), tracked as S-7 in SECURITY.md.

CREATE TABLE user_totp (
    user_id     INTEGER PRIMARY KEY REFERENCES users(id) ON DELETE CASCADE,
    secret      TEXT    NOT NULL,
    enabled_at  DATETIME,
    created_at  DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP
);
