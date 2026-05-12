-- Versioned manuscripts (arXiv-style).
--
-- Model: the `manuscripts` row is always the LATEST version. Every
-- historical version (including v1) is also recorded in
-- `manuscript_versions` so the archive is complete; a reader can ask
-- for v2 of a piece even after the author has shipped v4.
--
-- What's versioned (per row in manuscript_versions): title, abstract,
-- authors, category, pdf_path, external_url, conductor_notes, license,
-- ai_training, and a required revision_note ("Fixed typo in Thm 2.1",
-- "Added Section 3" ...).
--
-- What stays on the manuscripts row only (immutable across versions):
--   - submitter_id, arxiv_like_id, doi
--   - conductor_type, conductor_human/ai/role/public flags
--   - agent_framework
--   - has_auditor + all auditor_* fields
--   - withdrawal status
-- Changing any of these means a new submission, not a revision.

-- An older `manuscript_versions` table existed in the JS-app schema with
-- a different shape (and is empty on every PreXiv deployment we know of
-- — it was an unfinished experiment). Drop it so our CREATE TABLE below
-- works cleanly. Guarded by IF EXISTS so fresh installs don't fail.
DROP TABLE IF EXISTS manuscript_versions;
DROP INDEX IF EXISTS idx_versions_manuscript;

ALTER TABLE manuscripts ADD COLUMN current_version INTEGER NOT NULL DEFAULT 1;

CREATE TABLE manuscript_versions (
    id              INTEGER PRIMARY KEY AUTOINCREMENT,
    manuscript_id   INTEGER NOT NULL REFERENCES manuscripts(id) ON DELETE CASCADE,
    version_number  INTEGER NOT NULL,

    title           TEXT NOT NULL,
    abstract        TEXT NOT NULL,
    authors         TEXT NOT NULL,
    category        TEXT NOT NULL,
    pdf_path        TEXT,
    external_url    TEXT,
    conductor_notes TEXT,
    license         TEXT,
    ai_training     TEXT,

    revision_note   TEXT,                              -- v1: NULL; vN>1: required at API/HTML layer
    revised_at      DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP,

    UNIQUE (manuscript_id, version_number)
);

CREATE INDEX idx_mv_manuscript ON manuscript_versions(manuscript_id);

-- Backfill: every existing manuscript becomes its own v1 with the
-- current row's data. created_at carries through as the v1 revised_at
-- so the version log starts at the moment of original submission.
INSERT INTO manuscript_versions (
    manuscript_id, version_number,
    title, abstract, authors, category,
    pdf_path, external_url, conductor_notes, license, ai_training,
    revision_note, revised_at
)
SELECT
    id, 1,
    title, abstract, authors, category,
    pdf_path, external_url, conductor_notes, license, ai_training,
    NULL, COALESCE(created_at, CURRENT_TIMESTAMP)
FROM manuscripts;
