//! Outbound email — HTTPS POST to Brevo's transactional API.
//!
//! Why not SMTP: victoria sits behind a carrier-grade NAT whose ISP blocks
//! outbound ports 25 / 465 / 587. SMTP relaying through any provider is
//! impossible on this network. Brevo's HTTPS API works on port 443 — the
//! same channel the HIBP password check uses — and is reachable.
//!
//! Config:
//!
//!   BREVO_API_KEY      xkeysib-…           (required to send; absent = dev mode)
//!   MAIL_FROM_NAME     PreXiv              (default if unset)
//!   MAIL_FROM_ADDRESS  bydonfancy@gmail.com (must match a verified sender on Brevo)
//!
//! Dev mode (no BREVO_API_KEY) logs the verification link to tracing::warn
//! instead of sending, so `cargo run` locally doesn't need credentials.

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

/// Sends the account verification email via Brevo's transactional API.
///
/// `to` is the recipient address, `username` is the greeting target, and
/// `verify_link` is the absolute URL the user should click. Returns Ok in
/// dev mode (no `BREVO_API_KEY`) without actually sending — the link is
/// logged so a local cargo run still produces something the developer
/// can click.
///
/// The HTTP call is given a 10-second timeout; outbound that takes longer
/// than that is treated as a failure so a slow provider doesn't pin the
/// caller (the register handler awaits this in a tokio::spawn, but we
/// still don't want orphaned futures piling up).
pub async fn send_verification_email(to: &str, username: &str, verify_link: &str) -> Result<()> {
    let api_key = match env::var("BREVO_API_KEY") {
        Ok(k) if !k.trim().is_empty() => k,
        _ => {
            tracing::warn!(
                target: "prexiv::email",
                %to, %username, link = %verify_link,
                "BREVO_API_KEY not configured — dev mode, verification link logged only"
            );
            return Ok(());
        }
    };

    let (from_name, from_addr) = from_pair()?;

    let subject = "Verify your email — PreXiv";
    let text_body = format!(
"Hi {username},

You registered an account at PreXiv. Click the link below to verify your email
address — verification is required before you can submit manuscripts or mint
API tokens:

  {verify_link}

The link expires in 24 hours. If you didn't register, ignore this email; no
action is taken until the link is followed.

— PreXiv
"
    );

    let payload = json!({
        "sender":    { "name": from_name, "email": from_addr },
        "to":        [ { "email": to } ],
        "subject":   subject,
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
        // Pull whatever the API said about the failure so logs are useful
        // when a sender isn't verified, a key is wrong, the quota is
        // exhausted, etc. Cap body length so a HTML error page can't
        // flood the journal.
        let body = resp.text().await.unwrap_or_default();
        let snippet: String = body.chars().take(500).collect();
        return Err(anyhow!(
            "Brevo returned {status}: {snippet}"
        ));
    }

    tracing::info!(
        target: "prexiv::email",
        %to, %username,
        "verification email accepted by Brevo"
    );
    Ok(())
}
