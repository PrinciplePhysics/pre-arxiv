//! Email-change tokens — mint, hash, persist, resolve.
//!
//! Same shape as `passwords.rs` and `verify.rs`, but the row also
//! carries the proposed new email address encrypted at rest: we don't
//! mutate the account email until the user confirms by clicking the link
//! we send to that new address. This proves both that the user typed a
//! real, reachable address and that they currently control it.

use std::time::Duration as StdDuration;

use anyhow::{Context, Result};
use base64::Engine;
use chrono::{Duration, NaiveDateTime, Utc};
use rand::RngCore;
use sha2::{Digest, Sha256};

use crate::db::DbPool;

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
pub async fn mint_token(pool: &DbPool, user_id: i64, new_email: &str) -> Result<String> {
    let mut tx = pool.begin().await?;
    sqlx::query(crate::db::pg(
        "DELETE FROM pending_email_changes WHERE user_id = ?",
    ))
    .bind(user_id)
    .execute(&mut *tx)
    .await
    .context("deleting prior pending_email_changes")?;

    let plain = generate_token();
    let hash = hash_token(&plain);
    let expires_at: NaiveDateTime = (Utc::now() + Duration::hours(TOKEN_TTL_HOURS)).naive_utc();
    let (email_hash, email_enc) =
        crate::crypto::seal_email(new_email).context("sealing pending email-change address")?;
    let email_hash = email_hash.to_vec();
    sqlx::query(crate::db::pg(
        "INSERT INTO pending_email_changes
             (user_id, new_email, new_email_hash, new_email_enc, token_hash, expires_at)
         VALUES (?, '', ?, ?, ?, ?)",
    ))
    .bind(user_id)
    .bind(&email_hash)
    .bind(&email_enc)
    .bind(&hash)
    .bind(expires_at)
    .execute(&mut *tx)
    .await
    .context("inserting pending_email_changes row")?;
    tx.commit().await?;
    Ok(plain)
}

pub async fn mint_and_send(
    pool: &DbPool,
    user_id: i64,
    new_email: &str,
    username: &str,
    app_url: Option<&str>,
) -> Result<String> {
    let token = mint_token(pool, user_id, new_email).await?;
    let base = app_url.unwrap_or("http://localhost:3001");
    let link = format!(
        "{}/confirm-email-change/{}",
        base.trim_end_matches('/'),
        token
    );

    tracing::info!(
        target: "prexiv::email_change",
        %username,
        "email-change confirmation link minted"
    );

    let to = new_email.to_string();
    let username_owned = username.to_string();
    let link_for_send = link.clone();
    tokio::spawn(async move {
        let send_fut =
            crate::email::send_email_change_confirmation(&to, &username_owned, &link_for_send);
        match tokio::time::timeout(StdDuration::from_secs(12), send_fut).await {
            Ok(Ok(())) => {}
            Ok(Err(e)) => {
                tracing::error!(target: "prexiv::email_change", error = %e, username = %username_owned, "email-change confirmation send failed");
            }
            Err(_) => {
                tracing::error!(target: "prexiv::email_change", username = %username_owned, "email-change confirmation send timed out after 12s");
            }
        }
    });

    Ok(token)
}

/// Resolve a plaintext token, honouring expiry. Returns (token_row_id,
/// user_id, decrypted_new_email) on hit.
pub async fn resolve_token(pool: &DbPool, plain: &str) -> Result<Option<(i64, i64, String)>> {
    let hash = hash_token(plain);
    let row: Option<(i64, i64, Vec<u8>, NaiveDateTime)> = sqlx::query_as(
        crate::db::pg("SELECT id, user_id, new_email_enc, expires_at FROM pending_email_changes WHERE token_hash = ?"),
    )
    .bind(&hash)
    .fetch_optional(pool)
    .await
    .context("looking up pending_email_changes token")?;

    let Some((token_id, user_id, new_email_enc, expires_at)) = row else {
        return Ok(None);
    };
    if expires_at < Utc::now().naive_utc() {
        let _ = sqlx::query(crate::db::pg(
            "DELETE FROM pending_email_changes WHERE id = ?",
        ))
        .bind(token_id)
        .execute(pool)
        .await;
        return Ok(None);
    }
    let new_email = crate::crypto::open_email(&new_email_enc)
        .context("decrypting pending email-change address")?;
    Ok(Some((token_id, user_id, new_email)))
}

/// Apply the pending email change: in one transaction, update
/// users.email + set email_verified=1 (the click proved ownership of
/// the new address) and delete the pending row. We also defensively
/// re-check uniqueness — a different user might have claimed this
/// address between the original request and the click.
pub async fn consume_and_apply(
    pool: &DbPool,
    token_id: i64,
    user_id: i64,
    new_email: &str,
) -> Result<bool> {
    let mut tx = pool.begin().await?;

    let (hash_arr, enc) =
        crate::crypto::seal_email(new_email).context("sealing new email for change")?;
    let new_hash = hash_arr.to_vec();
    let conflict: Option<(i64,)> = sqlx::query_as(crate::db::pg(
        "SELECT id FROM users WHERE email_hash = ? AND id != ? LIMIT 1",
    ))
    .bind(&new_hash)
    .bind(user_id)
    .fetch_optional(&mut *tx)
    .await
    .context("checking email uniqueness at consume time")?;
    if conflict.is_some() {
        // Drop the pending row and bail; caller renders an error page.
        let _ = sqlx::query(crate::db::pg(
            "DELETE FROM pending_email_changes WHERE id = ?",
        ))
        .bind(token_id)
        .execute(&mut *tx)
        .await;
        tx.commit().await.ok();
        return Ok(false);
    }

    let inst_email: i64 = if crate::email::is_institutional(new_email) {
        1
    } else {
        0
    };
    sqlx::query(crate::db::pg(
        "UPDATE users
           SET email = ?, email_hash = ?, email_enc = ?,
               email_verified = 1, institutional_email = ?
         WHERE id = ?",
    ))
    .bind("")
    .bind(&new_hash)
    .bind(&enc)
    .bind(inst_email)
    .bind(user_id)
    .execute(&mut *tx)
    .await
    .context("updating users.email + crypto fields")?;
    sqlx::query(crate::db::pg(
        "DELETE FROM pending_email_changes WHERE id = ?",
    ))
    .bind(token_id)
    .execute(&mut *tx)
    .await
    .context("deleting consumed change token")?;
    tx.commit().await?;
    Ok(true)
}

pub async fn invalidate_for_user(pool: &DbPool, user_id: i64) -> Result<()> {
    sqlx::query(crate::db::pg(
        "DELETE FROM pending_email_changes WHERE user_id = ?",
    ))
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
    pool: &DbPool,
    user_id: i64,
) -> Result<Option<(String, NaiveDateTime)>> {
    let row: Option<(Vec<u8>, NaiveDateTime)> = sqlx::query_as(
        crate::db::pg("SELECT new_email_enc, expires_at FROM pending_email_changes WHERE user_id = ? ORDER BY id DESC LIMIT 1"),
    )
    .bind(user_id)
    .fetch_optional(pool)
    .await
    .context("loading pending_email_changes for user")?;
    let Some((enc, exp)) = row else {
        return Ok(None);
    };
    if exp <= Utc::now().naive_utc() {
        return Ok(None);
    }
    let email = crate::crypto::open_email(&enc).context("decrypting pending email for display")?;
    Ok(Some((email, exp)))
}
