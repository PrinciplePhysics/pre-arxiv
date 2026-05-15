use axum::extract::{Path, State};
use axum::response::{Html, IntoResponse, Redirect, Response};
use tower_sessions::Session;

use crate::auth::MaybeUser;
use crate::error::{AppError, AppResult};
use crate::helpers::build_ctx;
use crate::markdown;
use crate::models::comment::CommentWithAuthor;
use crate::models::Manuscript;
use crate::state::AppState;
use crate::templates;
use crate::templates::layout::OgMeta;

pub async fn legacy_view_redirect(Path(id): Path<String>) -> AppResult<Redirect> {
    Ok(Redirect::permanent(&format!("/abs/{}", bare_slug(&id))))
}

pub async fn view_abs(
    State(state): State<AppState>,
    session: Session,
    maybe_user: MaybeUser,
    Path(id): Path<String>,
) -> AppResult<Response> {
    render_view(state, session, maybe_user, id).await
}

pub async fn pdf(State(state): State<AppState>, Path(id): Path<String>) -> AppResult<Response> {
    match load_public_artifact(&state, &id, ArtifactKind::Pdf).await? {
        ArtifactLookup::Found(path) => {
            Ok(Redirect::temporary(&format!("/static/uploads/{path}")).into_response())
        }
        ArtifactLookup::Alias(new_slug) => {
            Ok(Redirect::permanent(&format!("/pdf/{new_slug}")).into_response())
        }
        ArtifactLookup::Missing => Err(AppError::NotFound),
    }
}

pub async fn source(State(state): State<AppState>, Path(id): Path<String>) -> AppResult<Response> {
    match load_public_artifact(&state, &id, ArtifactKind::Source).await? {
        ArtifactLookup::Found(path) => {
            Ok(Redirect::temporary(&format!("/static/uploads/{path}")).into_response())
        }
        ArtifactLookup::Alias(new_slug) => {
            Ok(Redirect::permanent(&format!("/src/{new_slug}")).into_response())
        }
        ArtifactLookup::Missing => Err(AppError::NotFound),
    }
}

async fn render_view(
    state: AppState,
    session: Session,
    maybe_user: MaybeUser,
    id: String,
) -> AppResult<Response> {
    let lookup_id = lookup_prexiv_id(&id);
    let m: Option<Manuscript> = sqlx::query_as::<_, Manuscript>(crate::db::pg(
        r#"
        SELECT id, arxiv_like_id, doi, submitter_id, title, abstract, authors, category,
               pdf_path, external_url, source_path,
               conductor_type, conductor_ai_model, conductor_ai_model_public,
               conductor_human, conductor_human_public, conductor_role, conductor_notes,
               agent_framework,
               has_auditor, auditor_name, auditor_affiliation, auditor_role,
               auditor_statement, auditor_orcid,
               view_count, score, comment_count,
               withdrawn, withdrawn_reason, withdrawn_at,
               created_at, updated_at,
               license, ai_training, current_version, secondary_categories
        FROM manuscripts
        WHERE arxiv_like_id = ? OR CAST(id AS TEXT) = ?
        LIMIT 1
        "#,
    ))
    .bind(&lookup_id)
    .bind(&id)
    .fetch_optional(&state.pool)
    .await?;

    // If no row matched, check the retired-id alias table. A retired
    // slug 301-redirects to its current `prexiv:YYMMDD.xxxxxx`
    // counterpart so external links and citations keep working.
    let m = match m {
        Some(m) => m,
        None => {
            let alias: Option<(String,)> = sqlx::query_as(crate::db::pg(
                "SELECT new_slug FROM prexiv_id_aliases WHERE old_slug = ? OR old_slug = ? LIMIT 1",
            ))
            .bind(&id)
            .bind(&lookup_id)
            .fetch_optional(&state.pool)
            .await?;
            if let Some((new_slug,)) = alias {
                return Ok(
                    Redirect::permanent(&format!("/abs/{}", bare_slug(&new_slug))).into_response(),
                );
            }
            return Err(AppError::NotFound);
        }
    };
    let canonical_public_slug = m
        .arxiv_like_id
        .as_deref()
        .map(bare_slug)
        .unwrap_or_else(|| bare_slug(&id))
        .to_string();
    if id != canonical_public_slug {
        return Ok(Redirect::permanent(&format!("/abs/{canonical_public_slug}")).into_response());
    }

    let comments: Vec<CommentWithAuthor> = sqlx::query_as::<_, CommentWithAuthor>(crate::db::pg(
        r#"
        SELECT c.id, c.manuscript_id, c.author_id,
               u.username AS author_username,
               c.parent_id, c.content, c.score, c.created_at
        FROM comments c
        JOIN users u ON u.id = c.author_id
        WHERE c.manuscript_id = ?
        ORDER BY c.created_at ASC
        "#,
    ))
    .bind(m.id)
    .fetch_all(&state.pool)
    .await?;

    let submitter: Option<(String, Option<String>, i64, i64, i64, i64)> = sqlx::query_as(crate::db::pg(
        "SELECT username, display_name, email_verified, institutional_email, orcid_oauth_verified, github_oauth_verified
           FROM users WHERE id = ?",
    ))
    .bind(m.submitter_id)
    .fetch_optional(&state.pool)
    .await?;

    // Viewer's current vote on this manuscript: -1, 0, or +1.
    let my_vote: i64 = match &maybe_user.0 {
        Some(u) => sqlx::query_as::<_, (i64,)>(
            crate::db::pg("SELECT value FROM votes WHERE user_id = ? AND target_type = 'manuscript' AND target_id = ?"),
        )
        .bind(u.id)
        .bind(m.id)
        .fetch_optional(&state.pool)
        .await?
        .map(|(v,)| v)
        .unwrap_or(0),
        None => 0,
    };

    // Category counts for the sidebar "Subject Areas" index — same shape
    // as bioRxiv's category sidebar.
    let cats: Vec<(String, i64)> = sqlx::query_as::<_, (String, i64)>(
        crate::db::pg("SELECT category, COUNT(*) FROM manuscripts WHERE withdrawn = 0 GROUP BY category ORDER BY category")
    )
    .fetch_all(&state.pool)
    .await?;

    sqlx::query(crate::db::pg(
        "UPDATE manuscripts SET view_count = COALESCE(view_count, 0) + 1 WHERE id = ?",
    ))
    .bind(m.id)
    .execute(&state.pool)
    .await
    .ok();

    // Sharing metadata. abs-truncated description for OG/Twitter card.
    let slug = canonical_public_slug;
    let abs_excerpt = excerpt_plain(&m.r#abstract, 280);
    let base = state.app_url.as_deref().unwrap_or("");
    let canon = if base.is_empty() {
        format!("/abs/{}", slug)
    } else {
        format!("{}/abs/{}", base.trim_end_matches('/'), slug)
    };
    let og = OgMeta {
        title: strip_inline_md(&m.title),
        description: abs_excerpt.clone(),
        url: canon.clone(),
        kind: "article",
        published_time: m
            .created_at
            .map(|t| t.and_utc().format("%Y-%m-%dT%H:%M:%SZ").to_string()),
        modified_time: m
            .updated_at
            .map(|t| t.and_utc().format("%Y-%m-%dT%H:%M:%SZ").to_string()),
        author: Some(m.authors.clone()),
    };
    let jsonld = build_scholarly_article_jsonld(&m, &abs_excerpt, &canon);

    let mut ctx = build_ctx(&session, maybe_user, "/m").await;
    ctx.og = Some(og);
    ctx.jsonld = Some(jsonld);
    ctx.canonical_url = Some(canon);
    Ok(Html(
        templates::manuscript::render(&ctx, &m, &comments, submitter.as_ref(), &cats, my_vote)
            .into_string(),
    )
    .into_response())
}

enum ArtifactKind {
    Pdf,
    Source,
}

enum ArtifactLookup {
    Found(String),
    Alias(String),
    Missing,
}

async fn load_public_artifact(
    state: &AppState,
    id: &str,
    kind: ArtifactKind,
) -> AppResult<ArtifactLookup> {
    let lookup_id = lookup_prexiv_id(id);
    let row: Option<(Option<String>,)> = match kind {
        ArtifactKind::Pdf => sqlx::query_as(
            crate::db::pg("SELECT pdf_path FROM manuscripts WHERE arxiv_like_id = ? OR CAST(id AS TEXT) = ? LIMIT 1"),
        )
        .bind(&lookup_id)
        .bind(id)
        .fetch_optional(&state.pool)
        .await?,
        ArtifactKind::Source => sqlx::query_as(
            crate::db::pg("SELECT source_path FROM manuscripts WHERE arxiv_like_id = ? OR CAST(id AS TEXT) = ? LIMIT 1"),
        )
        .bind(&lookup_id)
        .bind(id)
        .fetch_optional(&state.pool)
        .await?,
    };
    if let Some((Some(path),)) = row {
        return Ok(ArtifactLookup::Found(path));
    }
    if row.is_some() {
        return Ok(ArtifactLookup::Missing);
    }
    let alias: Option<(String,)> = sqlx::query_as(crate::db::pg(
        "SELECT new_slug FROM prexiv_id_aliases WHERE old_slug = ? OR old_slug = ? LIMIT 1",
    ))
    .bind(id)
    .bind(&lookup_id)
    .fetch_optional(&state.pool)
    .await?;
    Ok(alias
        .map(|(new_slug,)| ArtifactLookup::Alias(bare_slug(&new_slug).to_string()))
        .unwrap_or(ArtifactLookup::Missing))
}

fn bare_slug(id: &str) -> &str {
    id.strip_prefix("prexiv:").unwrap_or(id)
}

fn lookup_prexiv_id(id: &str) -> String {
    if id.starts_with("prexiv:") {
        id.to_string()
    } else {
        format!("prexiv:{id}")
    }
}

/// First N chars of `s` with markdown + LaTeX stripped, suitable for an
/// OG description or schema.org `description` field. Doesn't try to
/// preserve formatting — readers seeing this expect plain prose.
fn excerpt_plain(s: &str, max: usize) -> String {
    // Pull out anything between $…$ or $$…$$, drop common markdown
    // markers, collapse whitespace.
    let s = markdown::strip_latex_text_commands(s);
    let mut out = String::with_capacity(s.len());
    let mut in_math = false;
    let mut in_double = false;
    let mut chars = s.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '\\' {
            let _ = chars.next();
            continue;
        }
        if c == '$' {
            if chars.peek() == Some(&'$') {
                chars.next();
                in_double = !in_double;
                if !in_double {
                    out.push(' ');
                }
                continue;
            }
            in_math = !in_math;
            if !in_math {
                out.push(' ');
            }
            continue;
        }
        if in_math || in_double {
            continue;
        }
        match c {
            '*' | '_' | '`' | '#' | '>' | '|' => {} // strip markdown markers
            '\n' | '\t' => out.push(' '),
            _ => out.push(c),
        }
    }
    // Collapse runs of whitespace.
    let collapsed: String = out.split_whitespace().collect::<Vec<_>>().join(" ");
    if collapsed.chars().count() <= max {
        collapsed
    } else {
        let truncated: String = collapsed.chars().take(max - 1).collect();
        format!("{}…", truncated.trim_end())
    }
}

fn strip_inline_md(s: &str) -> String {
    // Titles can have $…$ inline math. For OG the math should appear as
    // its source-form (good enough for sharing) so we just strip leading
    // markdown markers.
    s.replace(['*', '`', '_'], "")
}

fn build_scholarly_article_jsonld(m: &Manuscript, description: &str, url: &str) -> String {
    use serde_json::json;
    let authors: Vec<serde_json::Value> = m
        .authors
        .split(',')
        .map(|a| a.trim())
        .filter(|a| !a.is_empty())
        .map(|a| json!({"@type": "Person", "name": a}))
        .collect();
    let mut obj = serde_json::Map::new();
    obj.insert("@context".into(), json!("https://schema.org"));
    obj.insert("@type".into(), json!("ScholarlyArticle"));
    obj.insert("headline".into(), json!(strip_inline_md(&m.title)));
    obj.insert("name".into(), json!(strip_inline_md(&m.title)));
    obj.insert("description".into(), json!(description));
    obj.insert("author".into(), json!(authors));
    if let Some(ts) = m.created_at {
        obj.insert(
            "datePublished".into(),
            json!(ts.and_utc().format("%Y-%m-%dT%H:%M:%SZ").to_string()),
        );
    }
    if let Some(ts) = m.updated_at {
        obj.insert(
            "dateModified".into(),
            json!(ts.and_utc().format("%Y-%m-%dT%H:%M:%SZ").to_string()),
        );
    }
    obj.insert("url".into(), json!(url));
    obj.insert("inLanguage".into(), json!("en"));
    obj.insert(
        "publisher".into(),
        json!({
            "@type": "Organization",
            "name":  "PreXiv",
            "url":   "https://victoria.tail921ea4.ts.net/",
        }),
    );
    if let Some(doi) = &m.doi {
        obj.insert(
            "identifier".into(),
            json!({
                "@type":      "PropertyValue",
                "propertyID": "DOI",
                "value":      doi,
            }),
        );
    }
    if let Some(lic) = &m.license {
        obj.insert("license".into(), json!(lic));
    }
    obj.insert("about".into(), json!({"@type":"Thing","name":m.category}));
    serde_json::to_string(&obj).unwrap_or_else(|_| "{}".to_string())
}
