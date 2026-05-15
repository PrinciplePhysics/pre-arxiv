-- Legacy identity-signal fields. This migration originally introduced
-- the older scholar-verification concept:
--
--   orcid_verified       — user pasted an ORCID iD and the public
--                          record on pub.orcid.org carries a name
--                          consistent with their display name. Set by
--                          POST /me/verify-orcid.
--
--   institutional_email  — the email-verified-flag was set against an
--                          address whose domain matches the .edu /
--                          .ac.<cc> / .edu.<cc> patterns or our
--                          hand-curated R&D-org allowlist. Set
--                          automatically at register / email change.
--
-- The current product uses ORCID OAuth for authenticated ORCID status;
-- pasted ORCID name matching is retained only for compatibility. A
-- verified institutional email remains a stronger public identity
-- signal.
--
-- Default-listing and public-write gates now use account verification
-- from GitHub OAuth, ORCID OAuth, or email verification. Do not use the
-- legacy orcid_verified field for new trust decisions.

ALTER TABLE users ADD COLUMN orcid_verified      INTEGER NOT NULL DEFAULT 0;
ALTER TABLE users ADD COLUMN institutional_email INTEGER NOT NULL DEFAULT 0;
