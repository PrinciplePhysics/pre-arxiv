-- FTS5 virtual table over manuscripts (title + abstract + authors + pdf body)
-- plus AI/AD/AU triggers that keep it in sync. All idempotent.

CREATE VIRTUAL TABLE IF NOT EXISTS manuscripts_fts USING fts5(
  title, abstract, authors, pdf_text,
  content='manuscripts', content_rowid='id', tokenize='porter unicode61'
);

CREATE TRIGGER IF NOT EXISTS manuscripts_ai AFTER INSERT ON manuscripts BEGIN
  INSERT INTO manuscripts_fts(rowid, title, abstract, authors, pdf_text)
  VALUES (new.id, new.title, new.abstract, new.authors, COALESCE(new.pdf_text, ''));
END;

CREATE TRIGGER IF NOT EXISTS manuscripts_ad AFTER DELETE ON manuscripts BEGIN
  INSERT INTO manuscripts_fts(manuscripts_fts, rowid, title, abstract, authors, pdf_text)
  VALUES ('delete', old.id, old.title, old.abstract, old.authors, COALESCE(old.pdf_text, ''));
END;

CREATE TRIGGER IF NOT EXISTS manuscripts_au AFTER UPDATE ON manuscripts BEGIN
  INSERT INTO manuscripts_fts(manuscripts_fts, rowid, title, abstract, authors, pdf_text)
  VALUES ('delete', old.id, old.title, old.abstract, old.authors, COALESCE(old.pdf_text, ''));
  INSERT INTO manuscripts_fts(rowid, title, abstract, authors, pdf_text)
  VALUES (new.id, new.title, new.abstract, new.authors, COALESCE(new.pdf_text, ''));
END;
