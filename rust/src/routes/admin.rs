//! /admin — operational dashboard, flag queue, and audit log. Gated by RequireAdmin.

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

pub struct AdminStats {
    pub total_manuscripts: i64,
    pub live_manuscripts: i64,
    pub withdrawn_manuscripts: i64,
    pub manuscripts_24h: i64,
    pub manuscripts_7d: i64,
    pub audited_manuscripts: i64,
    pub hidden_human_manuscripts: i64,
    pub hidden_ai_manuscripts: i64,
    pub stored_pdfs: i64,
    pub stored_sources: i64,
    pub total_users: i64,
    pub email_verified_users: i64,
    pub admin_users: i64,
    pub verified_scholar_users: i64,
    pub orcid_oauth_users: i64,
    pub institutional_verified_users: i64,
    pub new_users_24h: i64,
    pub new_users_7d: i64,
    pub total_comments: i64,
    pub comments_24h: i64,
    pub comments_7d: i64,
    pub total_votes: i64,
    pub votes_7d: i64,
    pub open_flags: i64,
    pub flags_24h: i64,
    pub resolved_flags_7d: i64,
    pub open_flags_over_24h: i64,
    pub oldest_open_flag_at: Option<NaiveDateTime>,
    pub active_tokens: i64,
    pub tokens_used_7d: i64,
}

pub struct CategoryStatRow {
    pub category: String,
    pub total: i64,
    pub live: i64,
    pub latest_at: Option<NaiveDateTime>,
}

pub struct DailyTrendRow {
    pub day: String,
    pub primary_count: i64,
    pub secondary_count: i64,
}

pub struct UnverifiedHighActivityUserRow {
    pub username: String,
    pub display_name: Option<String>,
    pub created_at: Option<NaiveDateTime>,
    pub manuscript_count: i64,
    pub comment_count: i64,
    pub vote_count: i64,
    pub token_count: i64,
}

pub struct RecentSubmissionRow {
    pub slug: Option<String>,
    pub title: String,
    pub category: String,
    pub submitter_username: String,
    pub created_at: Option<NaiveDateTime>,
    pub score: i64,
    pub comment_count: i64,
    pub withdrawn: bool,
    pub has_auditor: bool,
    pub current_version: i64,
    pub has_stored_artifact: bool,
}

pub struct RecentUserRow {
    pub username: String,
    pub display_name: Option<String>,
    pub email_verified: bool,
    pub is_admin: bool,
    pub orcid_oauth_verified: bool,
    pub institutional_email: bool,
    pub created_at: Option<NaiveDateTime>,
}

pub struct AdminDashboard {
    pub stats: AdminStats,
    pub moderation_trend: Vec<DailyTrendRow>,
    pub user_growth: Vec<DailyTrendRow>,
    pub category_stats: Vec<CategoryStatRow>,
    pub unverified_high_activity_users: Vec<UnverifiedHighActivityUserRow>,
    pub recent_submissions: Vec<RecentSubmissionRow>,
    pub recent_users: Vec<RecentUserRow>,
    pub recent_audit: Vec<AuditRow>,
}

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
    let dashboard = load_dashboard(&state).await?;
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
            id,
            target_type,
            target_id,
            reason,
            reporter_username,
            created_at,
            target_label,
            target_url,
            target_withdrawn,
        });
    }

    let mut ctx = build_ctx(&session, maybe_user, "/admin").await;
    ctx.no_index = true;
    Ok(Html(
        templates::admin::render_queue(&ctx, &dashboard, &flags).into_string(),
    ))
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
    let note_opt = if note.is_empty() {
        None
    } else {
        Some(note.to_string())
    };
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
    #[serde(default)]
    pub page: Option<i64>,
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

async fn load_dashboard(state: &AppState) -> AppResult<AdminDashboard> {
    let (
        total_manuscripts,
        live_manuscripts,
        withdrawn_manuscripts,
        manuscripts_24h,
        manuscripts_7d,
        audited_manuscripts,
        hidden_human_manuscripts,
        hidden_ai_manuscripts,
        stored_pdfs,
        stored_sources,
    ): (i64, i64, i64, i64, i64, i64, i64, i64, i64, i64) = sqlx::query_as(
        r#"SELECT
              COUNT(*),
              COALESCE(SUM(CASE WHEN withdrawn = 0 THEN 1 ELSE 0 END), 0),
              COALESCE(SUM(CASE WHEN withdrawn != 0 THEN 1 ELSE 0 END), 0),
              COALESCE(SUM(CASE WHEN created_at >= datetime('now', '-1 day') THEN 1 ELSE 0 END), 0),
              COALESCE(SUM(CASE WHEN created_at >= datetime('now', '-7 days') THEN 1 ELSE 0 END), 0),
              COALESCE(SUM(CASE WHEN has_auditor != 0 THEN 1 ELSE 0 END), 0),
              COALESCE(SUM(CASE WHEN conductor_type = 'human-ai' AND conductor_human_public = 0 THEN 1 ELSE 0 END), 0),
              COALESCE(SUM(CASE WHEN conductor_ai_model_public = 0 THEN 1 ELSE 0 END), 0),
              COALESCE(SUM(CASE WHEN pdf_path IS NOT NULL AND pdf_path <> '' THEN 1 ELSE 0 END), 0),
              COALESCE(SUM(CASE WHEN source_path IS NOT NULL AND source_path <> '' THEN 1 ELSE 0 END), 0)
           FROM manuscripts"#,
    )
    .fetch_one(&state.pool)
    .await?;

    let (
        total_users,
        email_verified_users,
        admin_users,
        verified_scholar_users,
        orcid_oauth_users,
        institutional_verified_users,
        new_users_7d,
        new_users_24h,
    ): (i64, i64, i64, i64, i64, i64, i64, i64) = sqlx::query_as(
        r#"SELECT
              COUNT(*),
              COALESCE(SUM(CASE WHEN email_verified != 0 THEN 1 ELSE 0 END), 0),
              COALESCE(SUM(CASE WHEN is_admin != 0 THEN 1 ELSE 0 END), 0),
              COALESCE(SUM(CASE
                  WHEN orcid_oauth_verified != 0
                    OR (email_verified != 0 AND institutional_email != 0)
                  THEN 1 ELSE 0 END), 0),
              COALESCE(SUM(CASE WHEN orcid_oauth_verified != 0 THEN 1 ELSE 0 END), 0),
              COALESCE(SUM(CASE WHEN email_verified != 0 AND institutional_email != 0 THEN 1 ELSE 0 END), 0),
              COALESCE(SUM(CASE WHEN created_at >= datetime('now', '-7 days') THEN 1 ELSE 0 END), 0),
              COALESCE(SUM(CASE WHEN created_at >= datetime('now', '-1 day') THEN 1 ELSE 0 END), 0)
           FROM users"#,
    )
    .fetch_one(&state.pool)
    .await?;

    let (total_comments, comments_24h, comments_7d): (i64, i64, i64) = sqlx::query_as(
        r#"SELECT
              COUNT(*),
              COALESCE(SUM(CASE WHEN created_at >= datetime('now', '-1 day') THEN 1 ELSE 0 END), 0),
              COALESCE(SUM(CASE WHEN created_at >= datetime('now', '-7 days') THEN 1 ELSE 0 END), 0)
           FROM comments"#,
    )
    .fetch_one(&state.pool)
    .await?;

    let (total_votes, votes_7d): (i64, i64) = sqlx::query_as(
        r#"SELECT
              COUNT(*),
              COALESCE(SUM(CASE WHEN created_at >= datetime('now', '-7 days') THEN 1 ELSE 0 END), 0)
           FROM votes"#,
    )
    .fetch_one(&state.pool)
    .await?;

    let (
        open_flags,
        flags_24h,
        resolved_flags_7d,
        open_flags_over_24h,
        oldest_open_flag_at,
    ): (i64, i64, i64, i64, Option<NaiveDateTime>) =
        sqlx::query_as(
            r#"SELECT
                  COALESCE(SUM(CASE WHEN resolved = 0 THEN 1 ELSE 0 END), 0),
                  COALESCE(SUM(CASE WHEN resolved = 0 AND created_at >= datetime('now', '-1 day') THEN 1 ELSE 0 END), 0),
                  COALESCE(SUM(CASE WHEN resolved != 0 AND resolved_at >= datetime('now', '-7 days') THEN 1 ELSE 0 END), 0),
                  COALESCE(SUM(CASE WHEN resolved = 0 AND created_at < datetime('now', '-1 day') THEN 1 ELSE 0 END), 0),
                  MIN(CASE WHEN resolved = 0 THEN created_at ELSE NULL END)
               FROM flag_reports"#,
        )
        .fetch_one(&state.pool)
        .await?;

    let (active_tokens, tokens_used_7d): (i64, i64) = sqlx::query_as(
        r#"SELECT
              COUNT(*),
              COALESCE(SUM(CASE WHEN last_used_at >= datetime('now', '-7 days') THEN 1 ELSE 0 END), 0)
           FROM api_tokens
           WHERE expires_at IS NULL OR expires_at > CURRENT_TIMESTAMP"#,
    )
    .fetch_one(&state.pool)
    .await?;

    let moderation_trend_raw: Vec<(String, i64, i64)> = sqlx::query_as(
        r#"WITH RECURSIVE days(day) AS (
              SELECT date('now', '-6 days')
              UNION ALL
              SELECT date(day, '+1 day') FROM days WHERE day < date('now')
           )
           SELECT
              d.day,
              COALESCE((SELECT COUNT(*) FROM flag_reports f WHERE date(f.created_at) = d.day), 0),
              COALESCE((SELECT COUNT(*) FROM flag_reports f WHERE f.resolved != 0 AND date(f.resolved_at) = d.day), 0)
           FROM days d
           ORDER BY d.day ASC"#,
    )
    .fetch_all(&state.pool)
    .await?;
    let moderation_trend = moderation_trend_raw
        .into_iter()
        .map(|(day, primary_count, secondary_count)| DailyTrendRow {
            day,
            primary_count,
            secondary_count,
        })
        .collect();

    let user_growth_raw: Vec<(String, i64, i64)> = sqlx::query_as(
        r#"WITH RECURSIVE days(day) AS (
              SELECT date('now', '-6 days')
              UNION ALL
              SELECT date(day, '+1 day') FROM days WHERE day < date('now')
           )
           SELECT
              d.day,
              COALESCE((SELECT COUNT(*) FROM users u WHERE date(u.created_at) = d.day), 0),
              COALESCE((SELECT COUNT(*) FROM users u WHERE u.email_verified != 0 AND date(u.created_at) = d.day), 0)
           FROM days d
           ORDER BY d.day ASC"#,
    )
    .fetch_all(&state.pool)
    .await?;
    let user_growth = user_growth_raw
        .into_iter()
        .map(|(day, primary_count, secondary_count)| DailyTrendRow {
            day,
            primary_count,
            secondary_count,
        })
        .collect();

    let category_raw: Vec<(String, i64, i64, Option<NaiveDateTime>)> = sqlx::query_as(
        r#"SELECT
              category,
              COUNT(*) AS total,
              COALESCE(SUM(CASE WHEN withdrawn = 0 THEN 1 ELSE 0 END), 0) AS live,
              MAX(created_at) AS latest_at
           FROM manuscripts
           GROUP BY category
           ORDER BY total DESC, category ASC
           LIMIT 10"#,
    )
    .fetch_all(&state.pool)
    .await?;
    let category_stats = category_raw
        .into_iter()
        .map(|(category, total, live, latest_at)| CategoryStatRow {
            category,
            total,
            live,
            latest_at,
        })
        .collect();

    let high_activity_raw: Vec<(
        String,
        Option<String>,
        Option<NaiveDateTime>,
        i64,
        i64,
        i64,
        i64,
    )> = sqlx::query_as(
        r#"SELECT
              u.username,
              u.display_name,
              u.created_at,
              COALESCE(m.manuscript_count, 0),
              COALESCE(c.comment_count, 0),
              COALESCE(v.vote_count, 0),
              COALESCE(t.token_count, 0)
           FROM users u
           LEFT JOIN (
              SELECT submitter_id, COUNT(*) AS manuscript_count
              FROM manuscripts
              GROUP BY submitter_id
           ) m ON m.submitter_id = u.id
           LEFT JOIN (
              SELECT author_id, COUNT(*) AS comment_count
              FROM comments
              GROUP BY author_id
           ) c ON c.author_id = u.id
           LEFT JOIN (
              SELECT user_id, COUNT(*) AS vote_count
              FROM votes
              GROUP BY user_id
           ) v ON v.user_id = u.id
           LEFT JOIN (
              SELECT user_id, COUNT(*) AS token_count
              FROM api_tokens
              WHERE expires_at IS NULL OR expires_at > CURRENT_TIMESTAMP
              GROUP BY user_id
           ) t ON t.user_id = u.id
           WHERE u.email_verified = 0
             AND (COALESCE(m.manuscript_count, 0)
                + COALESCE(c.comment_count, 0)
                + COALESCE(v.vote_count, 0)
                + COALESCE(t.token_count, 0)) > 0
           ORDER BY
              (COALESCE(m.manuscript_count, 0) * 5
               + COALESCE(c.comment_count, 0) * 2
               + COALESCE(v.vote_count, 0)
               + COALESCE(t.token_count, 0) * 3) DESC,
              u.id DESC
           LIMIT 8"#,
    )
    .fetch_all(&state.pool)
    .await?;
    let unverified_high_activity_users = high_activity_raw
        .into_iter()
        .map(
            |(
                username,
                display_name,
                created_at,
                manuscript_count,
                comment_count,
                vote_count,
                token_count,
            )| UnverifiedHighActivityUserRow {
                username,
                display_name,
                created_at,
                manuscript_count,
                comment_count,
                vote_count,
                token_count,
            },
        )
        .collect();

    let recent_submission_raw: Vec<(
        Option<String>,
        String,
        String,
        String,
        Option<NaiveDateTime>,
        i64,
        i64,
        i64,
        i64,
        i64,
        i64,
    )> = sqlx::query_as(
        r#"SELECT
              m.arxiv_like_id,
              m.title,
              m.category,
              u.username,
              m.created_at,
              COALESCE(m.score, 0),
              COALESCE(m.comment_count, 0),
              m.withdrawn,
              m.has_auditor,
              COALESCE(m.current_version, 1),
              CASE
                WHEN (m.pdf_path IS NOT NULL AND m.pdf_path <> '')
                  OR (m.source_path IS NOT NULL AND m.source_path <> '')
                THEN 1 ELSE 0
              END
           FROM manuscripts m
           JOIN users u ON u.id = m.submitter_id
           ORDER BY m.created_at DESC
           LIMIT 8"#,
    )
    .fetch_all(&state.pool)
    .await?;
    let recent_submissions = recent_submission_raw
        .into_iter()
        .map(
            |(
                slug,
                title,
                category,
                submitter_username,
                created_at,
                score,
                comment_count,
                withdrawn,
                has_auditor,
                current_version,
                has_stored_artifact,
            )| RecentSubmissionRow {
                slug,
                title,
                category,
                submitter_username,
                created_at,
                score,
                comment_count,
                withdrawn: withdrawn != 0,
                has_auditor: has_auditor != 0,
                current_version,
                has_stored_artifact: has_stored_artifact != 0,
            },
        )
        .collect();

    let recent_user_raw: Vec<(
        String,
        Option<String>,
        i64,
        i64,
        i64,
        i64,
        Option<NaiveDateTime>,
    )> = sqlx::query_as(
        r#"SELECT username, display_name, email_verified, is_admin,
                  orcid_oauth_verified, institutional_email, created_at
           FROM users
           ORDER BY id DESC
           LIMIT 8"#,
    )
    .fetch_all(&state.pool)
    .await?;
    let recent_users = recent_user_raw
        .into_iter()
        .map(
            |(
                username,
                display_name,
                email_verified,
                is_admin,
                orcid_oauth_verified,
                institutional_email,
                created_at,
            )| RecentUserRow {
                username,
                display_name,
                email_verified: email_verified != 0,
                is_admin: is_admin != 0,
                orcid_oauth_verified: orcid_oauth_verified != 0,
                institutional_email: institutional_email != 0,
                created_at,
            },
        )
        .collect();

    let recent_audit = load_audit_rows(state, 8, 0).await?;

    Ok(AdminDashboard {
        stats: AdminStats {
            total_manuscripts,
            live_manuscripts,
            withdrawn_manuscripts,
            manuscripts_24h,
            manuscripts_7d,
            audited_manuscripts,
            hidden_human_manuscripts,
            hidden_ai_manuscripts,
            stored_pdfs,
            stored_sources,
            total_users,
            email_verified_users,
            admin_users,
            verified_scholar_users,
            orcid_oauth_users,
            institutional_verified_users,
            new_users_24h,
            new_users_7d,
            total_comments,
            comments_24h,
            comments_7d,
            total_votes,
            votes_7d,
            open_flags,
            flags_24h,
            resolved_flags_7d,
            open_flags_over_24h,
            oldest_open_flag_at,
            active_tokens,
            tokens_used_7d,
        },
        moderation_trend,
        user_growth,
        category_stats,
        unverified_high_activity_users,
        recent_submissions,
        recent_users,
        recent_audit,
    })
}

async fn load_audit_rows(state: &AppState, per: i64, offset: i64) -> AppResult<Vec<AuditRow>> {
    let raw: Vec<(
        i64,
        Option<i64>,
        String,
        Option<String>,
        Option<i64>,
        Option<String>,
        Option<String>,
        Option<NaiveDateTime>,
        Option<String>,
    )> = sqlx::query_as(
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

    Ok(raw
        .into_iter()
        .map(
            |(id, _actor_id, action, target_type, target_id, detail, ip, created_at, username)| {
                AuditRow {
                    id,
                    actor_username: username,
                    action,
                    target_type,
                    target_id,
                    detail,
                    ip,
                    created_at,
                }
            },
        )
        .collect())
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

    let entries = load_audit_rows(&state, per, offset).await?;

    let total: (i64,) = sqlx::query_as("SELECT COUNT(*) FROM audit_log")
        .fetch_one(&state.pool)
        .await?;

    let mut ctx = build_ctx(&session, maybe_user, "/admin").await;
    ctx.no_index = true;
    Ok(Html(
        templates::admin::render_audit(&ctx, &entries, page, per, total.0).into_string(),
    ))
}
