-- Submitter-chosen reader license + AI-training flag.
--
-- These are orthogonal to the existing platform-grant (which is implicit
-- in "you submitted to PreXiv") and to the conductor/auditor identity
-- metadata. See rust/src/templates/pages_content/licenses.html and the
-- full design note in routes/licenses.rs.
--
-- Defaults are chosen so legacy rows (which pre-date this migration)
-- get the most permissive sensible reading: CC BY 4.0 for the reader
-- license (matches what most academic preprints would have picked
-- anyway) and 'allow' for AI training (matches the pre-flag default).

ALTER TABLE manuscripts ADD COLUMN license TEXT NOT NULL DEFAULT 'CC-BY-4.0';
ALTER TABLE manuscripts ADD COLUMN ai_training TEXT NOT NULL DEFAULT 'allow';

CREATE INDEX IF NOT EXISTS idx_manuscripts_license     ON manuscripts(license);
CREATE INDEX IF NOT EXISTS idx_manuscripts_ai_training ON manuscripts(ai_training);
