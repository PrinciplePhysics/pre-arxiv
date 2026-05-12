-- S-7: PII-at-rest encryption for user email addresses.
--
-- We add two new columns alongside the existing plaintext `email`:
--
--   email_hash  BLOB  — 32-byte HMAC-SHA256 of the lowercased, trimmed
--                       email under the master key (PREXIV_DATA_KEY).
--                       Deterministic, so we can index on it for
--                       login / "is this email taken" lookups without
--                       decrypting every row.
--
--   email_enc   BLOB  — AES-256-GCM ciphertext (12-byte nonce ‖ tag).
--                       The actual value used for sending mail and for
--                       displaying back to the user in /me/edit.
--
-- The app backfills these columns at startup for any row where
-- `email_hash IS NULL` (i.e., legacy rows from before this migration).
-- Once that's done, the plaintext `email` column becomes a vestigial
-- bootstrap input and can be cleared by an operator via:
--
--     UPDATE users SET email = '' WHERE email_hash IS NOT NULL;
--
-- We don't drop the plaintext column here because losing PREXIV_DATA_KEY
-- with no fallback would brick every account. Keep the plaintext until
-- you're sure encryption works end-to-end, then clear it manually.

ALTER TABLE users ADD COLUMN email_hash BLOB DEFAULT NULL;
ALTER TABLE users ADD COLUMN email_enc  BLOB DEFAULT NULL;

-- A unique index lets us do constant-time existence checks on signup
-- and exact-match lookups on login. Partial index (`WHERE … IS NOT NULL`)
-- avoids collisions during the backfill window where some rows still
-- have NULL columns.
CREATE UNIQUE INDEX IF NOT EXISTS idx_users_email_hash
    ON users(email_hash)
    WHERE email_hash IS NOT NULL;
