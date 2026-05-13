//! In-product notifications.
//!
//! Each notification is one event addressed to one recipient. The
//! comment + follow handlers call `notify` after their state-mutating
//! INSERT; the `/me/notifications` page reads + marks-read.
//!
//! Failures (DB error during notify) are LOGGED, never bubbled — the
//! original action (the comment, the follow) has already happened and
//! we don't want notifications to roll back business logic.

use anyhow::Result;
use chrono::NaiveDateTime;
use sqlx::SqlitePool;

pub const KIND_COMMENT_ON_MY_MANUSCRIPT: &str = "comment_on_my_manuscript";
pub const KIND_REPLY_TO_MY_COMMENT: &str = "reply_to_my_comment";
pub const KIND_FOLLOWED: &str = "followed";

/// Row shape for the listing page.
#[derive(Debug, Clone, sqlx::FromRow)]
pub struct NotificationRow {
    pub id: i64,
    pub recipient_id: i64,
    pub actor_id: Option<i64>,
    pub kind: String,
    pub target_type: Option<String>,
    pub target_id: Option<i64>,
    pub detail: Option<String>,
    pub read_at: Option<NaiveDateTime>,
    pub created_at: Option<NaiveDateTime>,
    // Joined columns
    #[sqlx(default)]
    pub actor_username: Option<String>,
    #[sqlx(default)]
    pub actor_display: Option<String>,
    #[sqlx(default)]
    pub target_slug: Option<String>,
    #[sqlx(default)]
    pub target_title: Option<String>,
}

pub async fn notify(
    pool: &SqlitePool,
    recipient_id: i64,
    actor_id: Option<i64>,
    kind: &str,
    target_type: Option<&str>,
    target_id: Option<i64>,
    detail: Option<&str>,
) -> Result<()> {
    // Don't notify yourself.
    if Some(recipient_id) == actor_id {
        return Ok(());
    }
    sqlx::query(
        "INSERT INTO notifications (recipient_id, actor_id, kind, target_type, target_id, detail)
         VALUES (?, ?, ?, ?, ?, ?)",
    )
    .bind(recipient_id)
    .bind(actor_id)
    .bind(kind)
    .bind(target_type)
    .bind(target_id)
    .bind(detail)
    .execute(pool)
    .await?;
    Ok(())
}

pub async fn unread_count(pool: &SqlitePool, user_id: i64) -> Result<i64> {
    let (n,): (i64,) = sqlx::query_as(
        "SELECT COUNT(*) FROM notifications WHERE recipient_id = ? AND read_at IS NULL",
    )
    .bind(user_id)
    .fetch_one(pool)
    .await?;
    Ok(n)
}

pub async fn list_for(pool: &SqlitePool, user_id: i64, limit: i64) -> Result<Vec<NotificationRow>> {
    let rows = sqlx::query_as::<_, NotificationRow>(
        r#"SELECT n.id, n.recipient_id, n.actor_id, n.kind, n.target_type,
                  n.target_id, n.detail, n.read_at, n.created_at,
                  u.username AS actor_username,
                  u.display_name AS actor_display,
                  m.arxiv_like_id AS target_slug,
                  m.title AS target_title
           FROM notifications n
           LEFT JOIN users u ON u.id = n.actor_id
           LEFT JOIN manuscripts m ON
                (n.target_type = 'manuscript' AND m.id = n.target_id)
                OR
                (n.target_type = 'comment' AND m.id = (SELECT c.manuscript_id FROM comments c WHERE c.id = n.target_id))
           WHERE n.recipient_id = ?
           ORDER BY n.read_at IS NULL DESC, n.id DESC
           LIMIT ?"#,
    )
    .bind(user_id)
    .bind(limit)
    .fetch_all(pool)
    .await?;
    Ok(rows)
}

pub async fn mark_read(pool: &SqlitePool, user_id: i64, notification_id: i64) -> Result<()> {
    sqlx::query(
        "UPDATE notifications SET read_at = CURRENT_TIMESTAMP
         WHERE id = ? AND recipient_id = ? AND read_at IS NULL",
    )
    .bind(notification_id)
    .bind(user_id)
    .execute(pool)
    .await?;
    Ok(())
}

pub async fn mark_all_read(pool: &SqlitePool, user_id: i64) -> Result<()> {
    sqlx::query(
        "UPDATE notifications SET read_at = CURRENT_TIMESTAMP
         WHERE recipient_id = ? AND read_at IS NULL",
    )
    .bind(user_id)
    .execute(pool)
    .await?;
    Ok(())
}
