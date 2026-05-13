-- Ownership-grade ORCID binding.
--
-- `orcid_verified` from migration 0013 is only a public-record name
-- match. It is useful as a profile signal but does not prove that the
-- PreXiv account controls the ORCID account. These fields are set only
-- after an OAuth authorization-code round trip through orcid.org.

ALTER TABLE users ADD COLUMN orcid_oauth_verified INTEGER NOT NULL DEFAULT 0;
ALTER TABLE users ADD COLUMN orcid_oauth_verified_at DATETIME;
ALTER TABLE users ADD COLUMN orcid_oauth_sub TEXT;

CREATE UNIQUE INDEX IF NOT EXISTS idx_users_orcid_oauth_sub_unique
    ON users(orcid_oauth_sub)
    WHERE orcid_oauth_sub IS NOT NULL AND orcid_oauth_sub <> '';
