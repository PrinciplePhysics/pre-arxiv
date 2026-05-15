//! Email-verification tokens: mint, hash, persist, resolve.
//!
//! The plaintext token is shown to the user exactly once — embedded in the
//! /verify/{token} link we email them. We store SHA-256(plaintext) in
//! `email_verification_tokens.token_hash` so a DB leak doesn't disclose
//! follow-the-link auth material. Tokens expire in 24h; the verify handler
//! checks `expires_at` and refuses stale tokens.

use std::time::Duration as StdDuration;

use crate::db::DbPool;
use anyhow::{anyhow, Context, Result};
use base64::Engine;
use chrono::{Duration, NaiveDateTime, Utc};
use rand::RngCore;
use sha2::{Digest, Sha256};

const TOKEN_PREFIX: &str = "prexiv_verify_";
pub const TOKEN_TTL_HOURS: i64 = 24;

/// Plaintext token. 27 random bytes → 36 base64url chars, like the API
/// bearer token format (same entropy). The `prexiv_verify_` prefix makes
/// it easy to grep for in logs or accident-pasted secrets.
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

/// Insert a fresh token row for `user_id` and return the plaintext (which
/// must be embedded in the email and is never persisted).
pub async fn mint_token(pool: &DbPool, user_id: i64) -> Result<String> {
    let plain = generate_token();
    let hash = hash_token(&plain);
    let expires_at: NaiveDateTime = (Utc::now() + Duration::hours(TOKEN_TTL_HOURS)).naive_utc();

    sqlx::query(crate::db::pg(
        "INSERT INTO email_verification_tokens (user_id, token_hash, expires_at) VALUES (?, ?, ?)",
    ))
    .bind(user_id)
    .bind(&hash)
    .bind(expires_at)
    .execute(pool)
    .await
    .context("inserting email_verification_tokens row")?;

    Ok(plain)
}

/// Invalidate all pending tokens for a user. Used by /me/resend-verification
/// so the prior link can no longer be replayed.
pub async fn invalidate_for_user(pool: &DbPool, user_id: i64) -> Result<()> {
    sqlx::query(crate::db::pg(
        "DELETE FROM email_verification_tokens WHERE user_id = ?",
    ))
    .bind(user_id)
    .execute(pool)
    .await
    .context("deleting old email_verification_tokens rows")?;
    Ok(())
}

/// Mint a token, build the verify URL, send the outbound email, and
/// return the plaintext token so the caller can stash it in the session
/// for an inline-verify-link fallback in development.
///
/// The send is awaited with a short cap. Verification gates public write
/// access, so telling the user "sent" when the mail provider rejected the
/// message is worse than a slower response.
pub async fn mint_and_send(
    pool: &DbPool,
    user_id: i64,
    email: &str,
    username: &str,
    app_url: Option<&str>,
) -> Result<String> {
    let token = mint_token(pool, user_id).await?;
    let base = app_url.unwrap_or("http://localhost:3001");
    let link = format!("{}/verify/{}", base.trim_end_matches('/'), token);

    let send_fut = crate::email::send_verification_email(email, username, &link);
    match tokio::time::timeout(StdDuration::from_secs(12), send_fut).await {
        Ok(Ok(())) => {}
        Ok(Err(e)) => return Err(e).context("sending verification email"),
        Err(_) => return Err(anyhow!("verification email send timed out after 12s")),
    }

    Ok(token)
}

/// Resolve a plaintext token to the user it verifies. Returns Some(user_id)
/// if the token matches a row whose expires_at is still in the future.
/// Does NOT delete the row — the caller does, after successfully marking
/// the user verified, so a transient DB error doesn't leave a verified
/// user with a still-redeemable token.
pub async fn resolve_token(pool: &DbPool, plain: &str) -> Result<Option<(i64, i64)>> {
    let hash = hash_token(plain);
    let row: Option<(i64, i64, NaiveDateTime)> = sqlx::query_as(crate::db::pg(
        "SELECT id, user_id, expires_at FROM email_verification_tokens WHERE token_hash = ?",
    ))
    .bind(&hash)
    .fetch_optional(pool)
    .await
    .context("looking up verification token")?;

    let Some((token_id, user_id, expires_at)) = row else {
        return Ok(None);
    };
    if expires_at < Utc::now().naive_utc() {
        // Expired — clean it up while we're here.
        let _ = sqlx::query(crate::db::pg(
            "DELETE FROM email_verification_tokens WHERE id = ?",
        ))
        .bind(token_id)
        .execute(pool)
        .await;
        return Ok(None);
    }
    Ok(Some((token_id, user_id)))
}

pub async fn consume(pool: &DbPool, token_id: i64, user_id: i64) -> Result<()> {
    let mut tx = pool.begin().await?;
    sqlx::query(crate::db::pg(
        "UPDATE users SET email_verified = 1 WHERE id = ?",
    ))
    .bind(user_id)
    .execute(&mut *tx)
    .await
    .context("marking user verified")?;
    sqlx::query(crate::db::pg(
        "DELETE FROM email_verification_tokens WHERE id = ?",
    ))
    .bind(token_id)
    .execute(&mut *tx)
    .await
    .context("deleting consumed token")?;
    tx.commit().await?;
    Ok(())
}
