//! /admin — flag queue + audit log. Gated by RequireAdmin.

use axum::extract::{Form, Path, Query, State};
use axum::response::{Html, IntoResponse, Redirect, Response};
use chrono::NaiveDateTime;
use serde::Deserialize;
use tower_sessions::Session;

use crate::auth::{verify_csrf, MaybeUser, RequireAdmin};
use crate::error::AppResult;
use crate::helpers::{build_ctx, set_flash};
use crate::state::AppState;
use crate::templates;

pub struct FlagRow {
    pub id: i64,
    pub target_type: String,
    pub target_id: i64,
    pub reason: String,
    pub reporter_username: String,
    pub created_at: Option<NaiveDateTime>,
    /// Resolved target info (manuscript title or comment snippet).
    pub target_label: Option<String>,
    pub target_url: Option<String>,
    pub target_withdrawn: bool,
}

pub async fn queue(
    State(state): State<AppState>,
    session: Session,
    maybe_user: MaybeUser,
    RequireAdmin(_admin): RequireAdmin,
) -> AppResult<Html<String>> {
    let raw: Vec<(i64, String, i64, String, String, Option<NaiveDateTime>)> = sqlx::query_as(
        r#"SELECT f.id, f.target_type, f.target_id, f.reason,
                  u.username AS reporter_username, f.created_at
           FROM flag_reports f JOIN users u ON u.id = f.reporter_id
           WHERE f.resolved = 0
           ORDER BY f.created_at DESC LIMIT 200"#,
    )
    .fetch_all(&state.pool)
    .await?;

    let mut flags: Vec<FlagRow> = Vec::with_capacity(raw.len());
    for (id, target_type, target_id, reason, reporter_username, created_at) in raw {
        let (target_label, target_url, target_withdrawn) = match target_type.as_str() {
            "manuscript" => {
                let m: Option<(Option<String>, String, i64)> = sqlx::query_as(
                    "SELECT arxiv_like_id, title, withdrawn FROM manuscripts WHERE id = ?",
                )
                .bind(target_id)
                .fetch_optional(&state.pool)
                .await?;
                match m {
                    Some((Some(slug), title, w)) => (
                        Some(format!("{title} [{slug}]")),
                        Some(format!("/m/{slug}")),
                        w != 0,
                    ),
                    _ => (None, None, false),
                }
            }
            "comment" => {
                let c: Option<(String, String, Option<String>)> = sqlx::query_as(
                    r#"SELECT u.username, SUBSTR(c.content, 1, 200), m.arxiv_like_id
                       FROM comments c
                       JOIN manuscripts m ON m.id = c.manuscript_id
                       JOIN users u ON u.id = c.author_id
                       WHERE c.id = ?"#,
                )
                .bind(target_id)
                .fetch_optional(&state.pool)
                .await?;
                match c {
                    Some((author, snippet, Some(slug))) => (
                        Some(format!("comment by {author}: {snippet}")),
                        Some(format!("/m/{slug}#comment-{target_id}")),
                        false,
                    ),
                    _ => (None, None, false),
                }
            }
            _ => (None, None, false),
        };
        flags.push(FlagRow {
            id, target_type, target_id, reason, reporter_username, created_at,
            target_label, target_url, target_withdrawn,
        });
    }

    let mut ctx = build_ctx(&session, maybe_user, "/admin").await;
    ctx.no_index = true;
    Ok(Html(templates::admin::render_queue(&ctx, &flags).into_string()))
}

#[derive(Deserialize)]
pub struct ResolveForm {
    pub csrf_token: String,
    #[serde(default)]
    pub note: String,
}

pub async fn resolve(
    State(state): State<AppState>,
    session: Session,
    RequireAdmin(admin): RequireAdmin,
    Path(id): Path<i64>,
    Form(form): Form<ResolveForm>,
) -> AppResult<Response> {
    if !verify_csrf(&session, &form.csrf_token).await {
        set_flash(&session, "Form expired — please try again.").await;
        return Ok(Redirect::to("/admin").into_response());
    }
    let note = form.note.trim();
    let note_opt = if note.is_empty() { None } else { Some(note.to_string()) };
    sqlx::query(
        r#"UPDATE flag_reports
           SET resolved = 1, resolved_by_id = ?, resolved_at = CURRENT_TIMESTAMP, resolution_note = ?
           WHERE id = ?"#,
    )
    .bind(admin.id)
    .bind(note_opt.as_deref())
    .bind(id)
    .execute(&state.pool)
    .await?;

    // Audit log entry.
    let _ = sqlx::query(
        "INSERT INTO audit_log (actor_user_id, action, target_type, target_id, detail) VALUES (?, 'flag_resolve', 'flag', ?, ?)",
    )
    .bind(admin.id)
    .bind(id)
    .bind(note_opt.as_deref())
    .execute(&state.pool)
    .await;

    set_flash(&session, "Flag resolved.").await;
    Ok(Redirect::to("/admin").into_response())
}

#[derive(Deserialize)]
pub struct AuditQuery {
    #[serde(default)] pub page: Option<i64>,
}

pub struct AuditRow {
    pub id: i64,
    pub actor_username: Option<String>,
    pub action: String,
    pub target_type: Option<String>,
    pub target_id: Option<i64>,
    pub detail: Option<String>,
    pub ip: Option<String>,
    pub created_at: Option<NaiveDateTime>,
}

pub async fn audit(
    State(state): State<AppState>,
    session: Session,
    maybe_user: MaybeUser,
    RequireAdmin(_admin): RequireAdmin,
    Query(q): Query<AuditQuery>,
) -> AppResult<Html<String>> {
    let per: i64 = 50;
    let page = q.page.unwrap_or(1).max(1);
    let offset = (page - 1) * per;

    let raw: Vec<(i64, Option<i64>, String, Option<String>, Option<i64>, Option<String>, Option<String>, Option<NaiveDateTime>, Option<String>)> =
        sqlx::query_as(
            r#"SELECT a.id, a.actor_user_id, a.action, a.target_type, a.target_id,
                      a.detail, a.ip, a.created_at, u.username
               FROM audit_log a
               LEFT JOIN users u ON u.id = a.actor_user_id
               ORDER BY a.id DESC LIMIT ? OFFSET ?"#,
        )
        .bind(per)
        .bind(offset)
        .fetch_all(&state.pool)
        .await?;

    let entries: Vec<AuditRow> = raw
        .into_iter()
        .map(|(id, _actor_id, action, target_type, target_id, detail, ip, created_at, username)| AuditRow {
            id, actor_username: username, action, target_type, target_id, detail, ip, created_at,
        })
        .collect();

    let total: (i64,) = sqlx::query_as("SELECT COUNT(*) FROM audit_log")
        .fetch_one(&state.pool).await?;

    let mut ctx = build_ctx(&session, maybe_user, "/admin").await;
    ctx.no_index = true;
    Ok(Html(templates::admin::render_audit(&ctx, &entries, page, per, total.0).into_string()))
}
