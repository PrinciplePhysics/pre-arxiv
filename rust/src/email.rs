//! Outbound email — HTTPS POST to Brevo's transactional API.
//!
//! Why not SMTP: victoria sits behind a carrier-grade NAT whose ISP blocks
//! outbound ports 25 / 465 / 587 to the well-known consumer providers.
//! Brevo's HTTPS API works on port 443 — the same channel HIBP uses —
//! which IS reachable.
//!
//! Config:
//!
//!   BREVO_API_KEY      xkeysib-…           (required to send; absent = dev mode)
//!   MAIL_FROM_NAME     PreXiv              (default if unset)
//!   MAIL_FROM_ADDRESS  bydonfancy@gmail.com (must match a verified sender on Brevo)
//!
//! Dev mode (no BREVO_API_KEY) logs the link to tracing::warn instead of
//! sending, so a local `cargo run` doesn't need credentials.

use std::env;
use std::time::Duration;

use anyhow::{anyhow, Context, Result};
use serde_json::json;

const BREVO_ENDPOINT: &str = "https://api.brevo.com/v3/smtp/email";

fn from_pair() -> Result<(String, String)> {
    let name = env::var("MAIL_FROM_NAME").unwrap_or_else(|_| "PreXiv".to_string());
    let addr = env::var("MAIL_FROM_ADDRESS")
        .map_err(|_| anyhow!("MAIL_FROM_ADDRESS must be set (must match a verified Brevo sender)"))?;
    Ok((name, addr))
}

/// Low-level transactional send. Both verification and password-reset
/// emails route through here so the wire format / timeout / error
/// handling stay in one place.
async fn send_transactional(to: &str, subject: &str, text_body: &str) -> Result<()> {
    let api_key = match env::var("BREVO_API_KEY") {
        Ok(k) if !k.trim().is_empty() => k,
        _ => {
            tracing::warn!(
                target: "prexiv::email",
                %to, %subject,
                "BREVO_API_KEY not configured — dev mode, email not sent"
            );
            return Ok(());
        }
    };

    let (from_name, from_addr) = from_pair()?;
    let payload = json!({
        "sender":      { "name": from_name, "email": from_addr },
        "to":          [ { "email": to } ],
        "subject":     subject,
        "textContent": text_body,
    });

    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(10))
        .user_agent("PreXiv/0.1 (+https://github.com/prexiv/prexiv)")
        .build()
        .context("building reqwest client")?;

    let resp = client
        .post(BREVO_ENDPOINT)
        .header("api-key", api_key)
        .header("accept", "application/json")
        .header("content-type", "application/json")
        .json(&payload)
        .send()
        .await
        .context("posting to Brevo")?;

    let status = resp.status();
    if !status.is_success() {
        // Cap the body snippet so a verbose error page can't flood the
        // journal. Brevo's structured errors are short JSON anyway.
        let body = resp.text().await.unwrap_or_default();
        let snippet: String = body.chars().take(500).collect();
        return Err(anyhow!("Brevo returned {status}: {snippet}"));
    }

    tracing::info!(
        target: "prexiv::email",
        %to, %subject,
        "transactional email accepted by Brevo"
    );
    Ok(())
}

/// Sends the email-verification email.
pub async fn send_verification_email(to: &str, username: &str, verify_link: &str) -> Result<()> {
    send_transactional(
        to,
        "Verify your email — PreXiv",
        &format!(
"Hi {username},

You registered an account at PreXiv. Click the link below to verify your email
address — verification is required before you can submit manuscripts or mint
API tokens:

  {verify_link}

The link expires in 24 hours. If you didn't register, ignore this email; no
action is taken until the link is followed.

— PreXiv
"
        ),
    )
    .await
}

/// Sends the password-reset email. Shorter TTL (1h) is reflected in
/// the body copy so the user knows to act quickly.
pub async fn send_password_reset_email(
    to: &str,
    username: &str,
    reset_link: &str,
) -> Result<()> {
    send_transactional(
        to,
        "Reset your PreXiv password",
        &format!(
"Hi {username},

We received a request to reset the password on your PreXiv account. Click the
link below to set a new password — the link is good for 1 hour:

  {reset_link}

If you didn't request this, ignore this email; no action is taken until the
link is followed, and the link will simply expire.

— PreXiv
"
        ),
    )
    .await
}
