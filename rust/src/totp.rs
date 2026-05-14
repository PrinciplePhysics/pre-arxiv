//! TOTP (RFC 6238) primitives + DB access for two-factor auth.
//!
//! On enrollment: we generate a 20-byte random secret, base32-encode it,
//! INSERT into user_totp with enabled_at=NULL, and hand the user a QR
//! code (provisioning URL embedded). They scan it with their
//! authenticator app and type the first 6-digit code; on a match we
//! flip enabled_at to NOW().
//!
//! On login: after password verifies, we check user_totp.enabled_at IS
//! NOT NULL; if so we stash the candidate user_id in the session under
//! `pending_2fa_user_id` and redirect to /login/2fa where the user
//! types the current code.

use crate::db::DbPool;
use anyhow::{Context, Result};
use base64::Engine;
use rand::RngCore;
use totp_rs::{Algorithm, Secret, TOTP};

/// Stored TOTP record for one user. The shared secret is encrypted at rest
/// with PREXIV_DATA_KEY and decrypted only long enough to verify a code.
#[derive(Debug, Clone)]
pub struct UserTotp {
    pub secret: String,
    pub enabled_at: Option<chrono::NaiveDateTime>,
}

#[derive(sqlx::FromRow)]
struct UserTotpRow {
    secret: Vec<u8>,
    enabled_at: Option<chrono::NaiveDateTime>,
}

const ISSUER: &str = "PreXiv";

fn build_totp(secret_b32: &str, account: &str) -> Result<TOTP> {
    let secret = Secret::Encoded(secret_b32.to_string())
        .to_bytes()
        .map_err(|e| anyhow::anyhow!("decoding base32 secret: {e:?}"))?;
    TOTP::new(
        Algorithm::SHA1,
        6,
        1,
        30,
        secret,
        Some(ISSUER.to_string()),
        account.to_string(),
    )
    .map_err(|e| anyhow::anyhow!("TOTP::new: {e}"))
}

/// Generate a fresh base32-encoded secret (20 random bytes → 32 chars).
pub fn generate_secret() -> String {
    let mut bytes = [0u8; 20];
    rand::thread_rng().fill_bytes(&mut bytes);
    // base32 RFC 4648 (no padding).
    use base32::Alphabet::Rfc4648;
    base32::encode(Rfc4648 { padding: false }, &bytes)
}

/// Return the otpauth:// URL for the given secret + account label.
pub fn provisioning_url(secret_b32: &str, account: &str) -> Result<String> {
    Ok(build_totp(secret_b32, account)?.get_url())
}

/// Generate an SVG QR code (as a string of `<svg>...`) for the
/// provisioning URL. Falls back to a plain `<pre>` showing the URL on
/// any error so the user still has a path forward.
pub fn qr_svg(secret_b32: &str, account: &str) -> String {
    match build_totp(secret_b32, account)
        .and_then(|t| t.get_qr_png().map_err(|e| anyhow::anyhow!("{e}")))
    {
        Ok(png_bytes) => {
            // Embed PNG as a data URL — simpler than installing an SVG
            // QR encoder + serving as <svg>.
            let b64 = base64::engine::general_purpose::STANDARD.encode(png_bytes);
            format!(
                r#"<img alt="TOTP enrollment QR code" width="220" height="220" src="data:image/png;base64,{b64}">"#
            )
        }
        Err(_) => format!(
            "<pre class=\"copy-pre\">{}</pre>",
            provisioning_url(secret_b32, account).unwrap_or_default()
        ),
    }
}

pub fn verify(secret_b32: &str, code: &str) -> bool {
    // 30-second window + 1 step skew → 90 seconds of tolerance, which
    // is what authenticator apps assume.
    let code = code.trim();
    if code.len() != 6 || !code.chars().all(|c| c.is_ascii_digit()) {
        return false;
    }
    match build_totp(secret_b32, "verify") {
        Ok(t) => t.check_current(code).unwrap_or(false),
        Err(_) => false,
    }
}

// ── DB access ────────────────────────────────────────────────────────

pub async fn get_for(pool: &DbPool, user_id: i64) -> Result<Option<UserTotp>> {
    let row = sqlx::query_as::<_, UserTotpRow>(crate::db::pg(
        "SELECT secret, enabled_at FROM user_totp WHERE user_id = ?",
    ))
    .bind(user_id)
    .fetch_optional(pool)
    .await
    .context("loading user_totp")?;
    match row {
        Some(row) => {
            let secret =
                crate::crypto::decrypt_blob(&row.secret).context("decrypting TOTP secret")?;
            let secret = String::from_utf8(secret).context("TOTP secret is not UTF-8")?;
            Ok(Some(UserTotp {
                secret,
                enabled_at: row.enabled_at,
            }))
        }
        None => Ok(None),
    }
}

pub async fn start_enrollment(pool: &DbPool, user_id: i64) -> Result<String> {
    let secret = generate_secret();
    let secret_enc =
        crate::crypto::encrypt_blob(secret.as_bytes()).context("encrypting TOTP secret")?;
    sqlx::query(crate::db::pg(
        "INSERT INTO user_totp (user_id, secret, enabled_at)
         VALUES (?, ?, NULL)
         ON CONFLICT(user_id) DO UPDATE SET secret = excluded.secret, enabled_at = NULL",
    ))
    .bind(user_id)
    .bind(&secret_enc)
    .execute(pool)
    .await
    .context("upserting user_totp enrollment")?;
    Ok(secret)
}

pub async fn confirm_enrollment(pool: &DbPool, user_id: i64) -> Result<()> {
    sqlx::query(crate::db::pg(
        "UPDATE user_totp SET enabled_at = CURRENT_TIMESTAMP WHERE user_id = ?",
    ))
    .bind(user_id)
    .execute(pool)
    .await
    .context("setting user_totp.enabled_at")?;
    Ok(())
}

pub async fn disable(pool: &DbPool, user_id: i64) -> Result<()> {
    sqlx::query(crate::db::pg("DELETE FROM user_totp WHERE user_id = ?"))
        .bind(user_id)
        .execute(pool)
        .await
        .context("deleting user_totp")?;
    Ok(())
}

pub async fn is_enabled(pool: &DbPool, user_id: i64) -> bool {
    matches!(
        sqlx::query_as::<_, (i64,)>(crate::db::pg(
            "SELECT 1 FROM user_totp WHERE user_id = ? AND enabled_at IS NOT NULL"
        ),)
        .bind(user_id)
        .fetch_optional(pool)
        .await,
        Ok(Some(_))
    )
}
