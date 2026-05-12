-- Cross-listing across categories.
--
-- A math-physics paper can be both `math.MP` and `math.AP`; without a
-- cross-list mechanism we either pick one (hides it from the other
-- audience) or break the primary-category invariant. arXiv format:
-- "Subjects: Mathematical Physics (math-ph); Analysis of PDEs (math.AP)".
--
-- Stored as a single TEXT column with whitespace-separated category
-- ids (e.g. "math.AP cs.LG q-bio.PE"). 0..N secondaries; the primary
-- stays in `manuscripts.category` alone. Listing queries that want
-- "everything in math.AP" check both the primary AND
--   (' ' || secondary_categories || ' ') LIKE '% math.AP %'
-- so a paper appears in both filters.

ALTER TABLE manuscripts
  ADD COLUMN secondary_categories TEXT DEFAULT NULL;
