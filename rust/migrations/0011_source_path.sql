-- arXiv-style LaTeX-source submissions: we now compile pdflatex
-- server-side from an uploaded archive. The served source is kept
-- alongside the compiled PDF so reviewers can re-build it; when
-- conductor/model privacy is requested, this is the blacked-out source.

ALTER TABLE manuscripts ADD COLUMN source_path TEXT DEFAULT NULL;
