-- Aliases for retired PreXiv ids.
--
-- We renumbered every existing manuscript when the id scheme switched
-- from `prexiv:YYMM.NNNNN` (random) to `prexiv:YYMMDD.SSSSSS` (Crockford
-- base-32 monotonic; lex-sort = chronological-sort). External links and
-- citations under the old scheme still need to resolve, so we record
-- the mapping here and 301-redirect old slugs at the request layer.
--
-- `old_slug` and `new_slug` are both stored as the full id including
-- the `prexiv:` prefix, exactly the way they appear in URLs. `new_slug`
-- is also indexed because admin tooling may want to do reverse lookups
-- ("what was X's previous id?").

CREATE TABLE IF NOT EXISTS prexiv_id_aliases (
    old_slug   TEXT PRIMARY KEY,
    new_slug   TEXT NOT NULL,
    aliased_at DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP
);

CREATE INDEX IF NOT EXISTS idx_prexiv_id_aliases_new
    ON prexiv_id_aliases(new_slug);
