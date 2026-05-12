//! Manuscript versioning — read/write primitives.
//!
//! Model: the `manuscripts` row is always the LATEST version. Every
//! historical version (including v1) is also recorded in
//! `manuscript_versions` so the archive is complete; a reader can ask
//! for v2 of a piece even after the author has shipped v4.
//!
//! Revising a manuscript is one transaction:
//!   1. UPDATE manuscripts: set the editable fields + increment
//!      current_version + bump updated_at.
//!   2. INSERT manuscript_versions: a snapshot of the new values with
//!      the new version_number.
//!
//! v1 is inserted at original submission time so the version log has
//! a complete history.

use anyhow::{Context, Result};
use sqlx::SqlitePool;

use crate::models::ManuscriptVersion;

/// Fields you can change in a revision. Everything else on the
/// manuscript row stays put (conductor, audit, identity).
#[derive(Debug, Clone)]
pub struct VersionInput<'a> {
    pub title: &'a str,
    pub r#abstract: &'a str,
    pub authors: &'a str,
    pub category: &'a str,
    pub pdf_path: Option<&'a str>,
    pub external_url: Option<&'a str>,
    pub conductor_notes: Option<&'a str>,
    pub license: &'a str,
    pub ai_training: &'a str,
    pub revision_note: Option<&'a str>,
}

/// Insert the initial v1 row alongside a freshly-created manuscript.
/// Called from the two submit handlers (HTML + JSON API) after the
/// manuscripts INSERT, inside the same enclosing transaction.
pub async fn insert_initial<'c, E>(
    tx: E,
    manuscript_id: i64,
    v: &VersionInput<'_>,
) -> Result<i64>
where
    E: sqlx::Executor<'c, Database = sqlx::Sqlite>,
{
    let res = sqlx::query(
        r#"INSERT INTO manuscript_versions
              (manuscript_id, version_number,
               title, abstract, authors, category,
               pdf_path, external_url, conductor_notes,
               license, ai_training,
               revision_note)
           VALUES (?, 1, ?, ?, ?, ?, ?, ?, ?, ?, ?, NULL)"#,
    )
    .bind(manuscript_id)
    .bind(v.title)
    .bind(v.r#abstract)
    .bind(v.authors)
    .bind(v.category)
    .bind(v.pdf_path)
    .bind(v.external_url)
    .bind(v.conductor_notes)
    .bind(v.license)
    .bind(v.ai_training)
    .execute(tx)
    .await
    .context("inserting initial manuscript_versions row")?;
    Ok(res.last_insert_rowid())
}

/// Apply a revision in one transaction. Returns the new version_number.
/// Caller is responsible for permission checks + non-withdrawn check.
pub async fn mint_revision(
    pool: &SqlitePool,
    manuscript_id: i64,
    v: &VersionInput<'_>,
) -> Result<i64> {
    let mut tx = pool.begin().await?;

    let (current,): (i64,) =
        sqlx::query_as("SELECT current_version FROM manuscripts WHERE id = ?")
            .bind(manuscript_id)
            .fetch_one(&mut *tx)
            .await
            .context("fetching current_version")?;
    let next = current + 1;

    sqlx::query(
        r#"UPDATE manuscripts SET
              title = ?, abstract = ?, authors = ?, category = ?,
              pdf_path = ?, external_url = ?,
              conductor_notes = ?, license = ?, ai_training = ?,
              current_version = ?, updated_at = CURRENT_TIMESTAMP
           WHERE id = ?"#,
    )
    .bind(v.title)
    .bind(v.r#abstract)
    .bind(v.authors)
    .bind(v.category)
    .bind(v.pdf_path)
    .bind(v.external_url)
    .bind(v.conductor_notes)
    .bind(v.license)
    .bind(v.ai_training)
    .bind(next)
    .bind(manuscript_id)
    .execute(&mut *tx)
    .await
    .context("updating manuscripts with new version")?;

    sqlx::query(
        r#"INSERT INTO manuscript_versions
              (manuscript_id, version_number,
               title, abstract, authors, category,
               pdf_path, external_url, conductor_notes,
               license, ai_training,
               revision_note)
           VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)"#,
    )
    .bind(manuscript_id)
    .bind(next)
    .bind(v.title)
    .bind(v.r#abstract)
    .bind(v.authors)
    .bind(v.category)
    .bind(v.pdf_path)
    .bind(v.external_url)
    .bind(v.conductor_notes)
    .bind(v.license)
    .bind(v.ai_training)
    .bind(v.revision_note)
    .execute(&mut *tx)
    .await
    .context("inserting new manuscript_versions row")?;

    tx.commit().await?;
    Ok(next)
}

/// List all versions of a manuscript, newest first.
pub async fn list_versions(pool: &SqlitePool, manuscript_id: i64) -> Result<Vec<ManuscriptVersion>> {
    let rows = sqlx::query_as::<_, ManuscriptVersion>(
        r#"SELECT id, manuscript_id, version_number,
                  title, abstract, authors, category,
                  pdf_path, external_url, conductor_notes,
                  license, ai_training,
                  revision_note, revised_at
           FROM manuscript_versions
           WHERE manuscript_id = ?
           ORDER BY version_number DESC"#,
    )
    .bind(manuscript_id)
    .fetch_all(pool)
    .await
    .context("listing manuscript_versions")?;
    Ok(rows)
}

/// Look up one specific version. Returns None if version_number doesn't exist.
pub async fn get_version(
    pool: &SqlitePool,
    manuscript_id: i64,
    version_number: i64,
) -> Result<Option<ManuscriptVersion>> {
    let row = sqlx::query_as::<_, ManuscriptVersion>(
        r#"SELECT id, manuscript_id, version_number,
                  title, abstract, authors, category,
                  pdf_path, external_url, conductor_notes,
                  license, ai_training,
                  revision_note, revised_at
           FROM manuscript_versions
           WHERE manuscript_id = ? AND version_number = ?"#,
    )
    .bind(manuscript_id)
    .bind(version_number)
    .fetch_optional(pool)
    .await
    .context("fetching specific manuscript_versions row")?;
    Ok(row)
}
