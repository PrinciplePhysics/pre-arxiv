-- "Verified scholar" signal — two independent gates that grant the
-- badge:
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
-- Either one alone makes the user a "verified scholar". Both are
-- displayed independently on the profile page (an institutional-email
-- user without an ORCID still gets the badge, and vice-versa).
--
-- We don't drop or gate anything on these flags yet — they are used
-- in default listing filters (skip "general"-bucket categories +
-- gently de-emphasize unverified authors) and to render banners on
-- the manuscript page. Submission itself remains open.

ALTER TABLE users ADD COLUMN orcid_verified      INTEGER NOT NULL DEFAULT 0;
ALTER TABLE users ADD COLUMN institutional_email INTEGER NOT NULL DEFAULT 0;
