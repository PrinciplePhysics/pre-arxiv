#![allow(clippy::type_complexity)]
//! Password-reset tokens — mint, hash, persist, resolve.
//!
//! Shape mirrors `verify.rs` (email-verification tokens) but with a much
//! shorter TTL (1 hour) because password reset is the higher-value
//! attack surface: the redeemed link lets the holder set a new password
//! and immediately authenticate. Tighter window → less wall-clock for
//! a leaked link to be abused.
//!
//! Token format: `prexiv_reset_` + 36 base64url chars (27 random bytes
//! of entropy, same as API and verify tokens). The DB only stores
//! SHA-256(plaintext) so a database leak doesn't disclose redeemable
//! material.

use std::time::Duration as StdDuration;

use crate::db::DbPool;
use anyhow::{Context, Result};
use base64::Engine;
use chrono::{Duration, NaiveDateTime, Utc};
use rand::RngCore;
use sha2::{Digest, Sha256};

const TOKEN_PREFIX: &str = "prexiv_reset_";
pub const TOKEN_TTL_MINUTES: i64 = 60;

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

/// Mint a new reset token for `user_id`, invalidating any prior tokens
/// for that user so only the most recent link is redeemable. Returns
/// the plaintext token (caller embeds it in the reset URL).
pub async fn mint_token(pool: &DbPool, user_id: i64) -> Result<String> {
    // Invalidate prior tokens in the same transaction so a flooded
    // attacker can't keep one alive.
    let mut tx = pool.begin().await?;
    sqlx::query(crate::db::pg(
        "DELETE FROM password_reset_tokens WHERE user_id = ?",
    ))
    .bind(user_id)
    .execute(&mut *tx)
    .await
    .context("deleting prior password_reset_tokens")?;

    let plain = generate_token();
    let hash = hash_token(&plain);
    let expires_at: NaiveDateTime = (Utc::now() + Duration::minutes(TOKEN_TTL_MINUTES)).naive_utc();

    sqlx::query(crate::db::pg(
        "INSERT INTO password_reset_tokens (user_id, token_hash, expires_at) VALUES (?, ?, ?)",
    ))
    .bind(user_id)
    .bind(&hash)
    .bind(expires_at)
    .execute(&mut *tx)
    .await
    .context("inserting password_reset_tokens row")?;

    tx.commit().await?;
    Ok(plain)
}

/// Mint + send. Like `verify::mint_and_send` this returns the plaintext
/// token synchronously and spawns the email send so the HTTP response
/// isn't blocked on the upstream provider. The plaintext link is never
/// logged.
pub async fn mint_and_send(
    pool: &DbPool,
    user_id: i64,
    email: &str,
    username: &str,
    app_url: Option<&str>,
) -> Result<String> {
    let token = mint_token(pool, user_id).await?;
    let base = app_url.unwrap_or("http://localhost:3001");
    let link = format!("{}/reset-password/{}", base.trim_end_matches('/'), token);

    tracing::info!(
        target: "prexiv::passwords",
        %username,
        "password reset link minted"
    );

    let to = email.to_string();
    let username_owned = username.to_string();
    let link_for_send = link.clone();
    tokio::spawn(async move {
        let send_fut =
            crate::email::send_password_reset_email(&to, &username_owned, &link_for_send);
        match tokio::time::timeout(StdDuration::from_secs(12), send_fut).await {
            Ok(Ok(())) => {}
            Ok(Err(e)) => {
                tracing::error!(target: "prexiv::passwords", error = %e, username = %username_owned, "password reset email send failed");
            }
            Err(_) => {
                tracing::error!(target: "prexiv::passwords", username = %username_owned, "password reset email send timed out after 12s");
            }
        }
    });

    Ok(token)
}

/// Resolve a plaintext token to the user it can reset for. Returns the
/// token row id + user id when the token matches an unexpired row. Does
/// not delete the row — the caller deletes after successfully updating
/// the password, so a transient DB error doesn't leave the user
/// password-changed with a still-redeemable token.
pub async fn resolve_token(pool: &DbPool, plain: &str) -> Result<Option<(i64, i64)>> {
    let hash = hash_token(plain);
    let row: Option<(i64, i64, NaiveDateTime)> = sqlx::query_as(crate::db::pg(
        "SELECT id, user_id, expires_at FROM password_reset_tokens WHERE token_hash = ?",
    ))
    .bind(&hash)
    .fetch_optional(pool)
    .await
    .context("looking up password reset token")?;

    let Some((token_id, user_id, expires_at)) = row else {
        return Ok(None);
    };
    if expires_at < Utc::now().naive_utc() {
        let _ = sqlx::query(crate::db::pg(
            "DELETE FROM password_reset_tokens WHERE id = ?",
        ))
        .bind(token_id)
        .execute(pool)
        .await;
        return Ok(None);
    }
    Ok(Some((token_id, user_id)))
}

/// Set the user's password to `new_hash` and consume the token in a
/// single transaction. Returns Ok on success; the caller is responsible
/// for rotating the session and logging the user in.
pub async fn consume_and_set(
    pool: &DbPool,
    token_id: i64,
    user_id: i64,
    new_hash: &str,
) -> Result<()> {
    let mut tx = pool.begin().await?;
    sqlx::query(crate::db::pg(
        "UPDATE users SET password_hash = ? WHERE id = ?",
    ))
    .bind(new_hash)
    .bind(user_id)
    .execute(&mut *tx)
    .await
    .context("updating users.password_hash")?;
    sqlx::query(crate::db::pg(
        "DELETE FROM password_reset_tokens WHERE id = ?",
    ))
    .bind(token_id)
    .execute(&mut *tx)
    .await
    .context("deleting consumed reset token")?;
    tx.commit().await?;
    Ok(())
}

/// Look up a user_id by either username or email. Used by the forgot-
/// password endpoint, which accepts either. Returns Ok(None) for misses
/// — the caller treats hit and miss identically to defeat enumeration.
pub async fn find_user_by_email_or_username(
    pool: &DbPool,
    needle: &str,
) -> Result<Option<(i64, String, String)>> {
    // Look up by username OR by the blind-index `email_hash`. The plaintext
    // `email` column may be empty post-harden, so we recover the address
    // from `email_enc` and decrypt before returning it to the caller (used
    // to address the password-reset email).
    let needle_hash = crate::crypto::email_hash(needle).to_vec();
    let row: Option<(i64, String, Option<Vec<u8>>, Option<String>)> =
        sqlx::query_as(crate::db::pg(
            "SELECT id, username, email_enc, email
           FROM users WHERE username = ? OR email_hash = ? LIMIT 1",
        ))
        .bind(needle)
        .bind(&needle_hash)
        .fetch_optional(pool)
        .await
        .context("looking up user for password reset")?;
    let Some((id, username, enc, plain)) = row else {
        return Ok(None);
    };
    let email = match enc.as_deref() {
        Some(b) => crate::crypto::open_email(b).unwrap_or_else(|_| plain.unwrap_or_default()),
        None => plain.unwrap_or_default(),
    };
    Ok(Some((id, username, email)))
}
