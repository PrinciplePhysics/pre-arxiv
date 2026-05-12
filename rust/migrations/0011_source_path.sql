-- arXiv-style LaTeX-source submissions: we now compile pdflatex
-- server-side from an uploaded archive. The original source is kept
-- alongside the compiled PDF so reviewers can re-build it.

ALTER TABLE manuscripts ADD COLUMN source_path TEXT DEFAULT NULL;
