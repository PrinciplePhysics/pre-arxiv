//! Email-change tokens — mint, hash, persist, resolve.
//!
//! Same shape as `passwords.rs` and `verify.rs`, but the row also
//! carries the proposed new email address: we don't mutate users.email
//! until the user confirms by clicking the link we send to that new
//! address. This proves both that the user typed a real, reachable
//! address (vs. a typo) and that they currently control it.

use std::time::Duration as StdDuration;

use anyhow::{Context, Result};
use base64::Engine;
use chrono::{Duration, NaiveDateTime, Utc};
use rand::RngCore;
use sha2::{Digest, Sha256};
use sqlx::SqlitePool;

const TOKEN_PREFIX: &str = "prexiv_chgmail_";
pub const TOKEN_TTL_HOURS: i64 = 24;

pub fn generate_token() -> String {
    let mut bytes = [0u8; 27];
    rand::thread_rng().fill_bytes(&mut bytes);
    let b64 = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(bytes);
    format!("{TOKEN_PREFIX}{b64}")
}

pub fn hash_token(plain: &str) -> String {
    let mut h = Sha256::new();
    h.update(plain.as_bytes());
    hex::encode(h.finalize())
}

/// Mint a pending-email-change token. Invalidates any prior pending
/// change for the same user inside one transaction. Returns the
/// plaintext token (which the caller embeds into the /confirm-email-
/// change/{token} URL).
pub async fn mint_token(pool: &SqlitePool, user_id: i64, new_email: &str) -> Result<String> {
    let mut tx = pool.begin().await?;
    sqlx::query("DELETE FROM pending_email_changes WHERE user_id = ?")
        .bind(user_id)
        .execute(&mut *tx)
        .await
        .context("deleting prior pending_email_changes")?;

    let plain = generate_token();
    let hash = hash_token(&plain);
    let expires_at: NaiveDateTime =
        (Utc::now() + Duration::hours(TOKEN_TTL_HOURS)).naive_utc();
    sqlx::query(
        "INSERT INTO pending_email_changes (user_id, new_email, token_hash, expires_at) VALUES (?, ?, ?, ?)",
    )
    .bind(user_id)
    .bind(new_email)
    .bind(&hash)
    .bind(expires_at)
    .execute(&mut *tx)
    .await
    .context("inserting pending_email_changes row")?;
    tx.commit().await?;
    Ok(plain)
}

pub async fn mint_and_send(
    pool: &SqlitePool,
    user_id: i64,
    new_email: &str,
    username: &str,
    app_url: Option<&str>,
) -> Result<String> {
    let token = mint_token(pool, user_id, new_email).await?;
    let base = app_url.unwrap_or("http://localhost:3001");
    let link = format!("{}/confirm-email-change/{}", base.trim_end_matches('/'), token);

    // Log so the operator can pluck the link while Brevo activation is
    // still pending. Same fallback strategy as forgot-password.
    tracing::info!(
        target: "prexiv::email_change",
        %username, %new_email, %link,
        "email-change confirmation link minted"
    );

    let to = new_email.to_string();
    let username_owned = username.to_string();
    let link_for_send = link.clone();
    tokio::spawn(async move {
        let send_fut = crate::email::send_email_change_confirmation(
            &to, &username_owned, &link_for_send,
        );
        match tokio::time::timeout(StdDuration::from_secs(12), send_fut).await {
            Ok(Ok(())) => {}
            Ok(Err(e)) => {
                tracing::error!(target: "prexiv::email_change", error = %e, email = %to, username = %username_owned, "email-change confirmation send failed");
            }
            Err(_) => {
                tracing::error!(target: "prexiv::email_change", email = %to, username = %username_owned, "email-change confirmation send timed out after 12s");
            }
        }
    });

    Ok(token)
}

/// Resolve a plaintext token, honouring expiry. Returns (token_row_id,
/// user_id, new_email) on hit.
pub async fn resolve_token(
    pool: &SqlitePool,
    plain: &str,
) -> Result<Option<(i64, i64, String)>> {
    let hash = hash_token(plain);
    let row: Option<(i64, i64, String, NaiveDateTime)> = sqlx::query_as(
        "SELECT id, user_id, new_email, expires_at FROM pending_email_changes WHERE token_hash = ?",
    )
    .bind(&hash)
    .fetch_optional(pool)
    .await
    .context("looking up pending_email_changes token")?;

    let Some((token_id, user_id, new_email, expires_at)) = row else {
        return Ok(None);
    };
    if expires_at < Utc::now().naive_utc() {
        let _ = sqlx::query("DELETE FROM pending_email_changes WHERE id = ?")
            .bind(token_id)
            .execute(pool)
            .await;
        return Ok(None);
    }
    Ok(Some((token_id, user_id, new_email)))
}

/// Apply the pending email change: in one transaction, update
/// users.email + set email_verified=1 (the click proved ownership of
/// the new address) and delete the pending row. We also defensively
/// re-check uniqueness — a different user might have claimed this
/// address between the original request and the click.
pub async fn consume_and_apply(
    pool: &SqlitePool,
    token_id: i64,
    user_id: i64,
    new_email: &str,
) -> Result<bool> {
    let mut tx = pool.begin().await?;

    let (hash_arr, enc) = crate::crypto::seal_email(new_email)
        .context("sealing new email for change")?;
    let new_hash = hash_arr.to_vec();
    let conflict: Option<(i64,)> = sqlx::query_as(
        "SELECT id FROM users WHERE email_hash = ? AND id != ? LIMIT 1",
    )
    .bind(&new_hash)
    .bind(user_id)
    .fetch_optional(&mut *tx)
    .await
    .context("checking email uniqueness at consume time")?;
    if conflict.is_some() {
        // Drop the pending row and bail; caller renders an error page.
        let _ = sqlx::query("DELETE FROM pending_email_changes WHERE id = ?")
            .bind(token_id)
            .execute(&mut *tx)
            .await;
        tx.commit().await.ok();
        return Ok(false);
    }

    sqlx::query(
        "UPDATE users
           SET email = ?, email_hash = ?, email_enc = ?, email_verified = 1
         WHERE id = ?",
    )
        .bind(new_email)
        .bind(&new_hash)
        .bind(&enc)
        .bind(user_id)
        .execute(&mut *tx)
        .await
        .context("updating users.email + crypto fields")?;
    sqlx::query("DELETE FROM pending_email_changes WHERE id = ?")
        .bind(token_id)
        .execute(&mut *tx)
        .await
        .context("deleting consumed change token")?;
    tx.commit().await?;
    Ok(true)
}

pub async fn invalidate_for_user(pool: &SqlitePool, user_id: i64) -> Result<()> {
    sqlx::query("DELETE FROM pending_email_changes WHERE user_id = ?")
        .bind(user_id)
        .execute(pool)
        .await
        .context("deleting pending_email_changes for user")?;
    Ok(())
}

/// Returns the pending change for a user, if any — used by /me/edit to
/// render the "you have a pending change to X — click here to confirm
/// or [Cancel]" banner. Returns (new_email, plaintext_token_is_none).
/// We don't store the plaintext token (only its SHA-256), so we can't
/// re-render the original link; we just expose the fact that one is
/// outstanding. The user can re-request to get a fresh link.
pub async fn pending_for_user(
    pool: &SqlitePool,
    user_id: i64,
) -> Result<Option<(String, NaiveDateTime)>> {
    let row: Option<(String, NaiveDateTime)> = sqlx::query_as(
        "SELECT new_email, expires_at FROM pending_email_changes WHERE user_id = ? ORDER BY id DESC LIMIT 1",
    )
    .bind(user_id)
    .fetch_optional(pool)
    .await
    .context("loading pending_email_changes for user")?;
    Ok(row.filter(|(_, exp)| *exp > Utc::now().naive_utc()))
}
