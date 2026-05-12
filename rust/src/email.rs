//! Outbound email — SMTP via lettre.
//!
//! For PreXiv on victoria the practical relay is the operator's own Gmail
//! account: smtp.gmail.com:587 with STARTTLS, authenticated by a Gmail
//! "App Password" (a 16-char per-app credential generated at
//! https://myaccount.google.com/apppasswords). We can't run an authoritative
//! MTA on victoria because the `victoria.tail921ea4.ts.net` hostname isn't a
//! DNS zone we control — without SPF/DKIM/DMARC/rDNS, mail straight to
//! gmail.com/outlook.com gets rejected or spam-binned. Relaying through
//! Gmail gives us deliverability that just works.
//!
//! Config is read once from environment variables at module init:
//!
//!   SMTP_HOST          smtp.gmail.com
//!   SMTP_PORT          587
//!   SMTP_USER          bydonfancy@gmail.com
//!   SMTP_PASS          <16-char Gmail App Password>
//!   MAIL_FROM_NAME     PreXiv             (default if unset)
//!   MAIL_FROM_ADDRESS  defaults to SMTP_USER (Gmail rejects unrelated From)
//!
//! If SMTP_HOST is unset we run in **dev mode**: emails are logged to
//! tracing::info! and never sent. This lets the local cargo run flow stay
//! self-contained and doesn't require an SMTP secret for development.

use std::env;

use anyhow::{anyhow, Context, Result};
use lettre::message::header::ContentType;
use lettre::message::Mailbox;
use lettre::transport::smtp::authentication::Credentials;
use lettre::{AsyncSmtpTransport, AsyncTransport, Message, Tokio1Executor};

/// Single SMTP transport built lazily once per process. Cloning it is cheap
/// (it shares the connection pool internally).
fn transport() -> Result<Option<AsyncSmtpTransport<Tokio1Executor>>> {
    let host = match env::var("SMTP_HOST") {
        Ok(h) if !h.trim().is_empty() => h,
        _ => return Ok(None),
    };
    let port: u16 = env::var("SMTP_PORT")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(587);
    let user = env::var("SMTP_USER").context("SMTP_USER is required when SMTP_HOST is set")?;
    let pass = env::var("SMTP_PASS").context("SMTP_PASS is required when SMTP_HOST is set")?;

    let creds = Credentials::new(user, pass);
    let tx = AsyncSmtpTransport::<Tokio1Executor>::starttls_relay(&host)
        .context("building SMTP relay")?
        .port(port)
        .credentials(creds)
        .build();
    Ok(Some(tx))
}

/// Returns the configured `From:` mailbox, or constructs a default from
/// `SMTP_USER` so we never accidentally rewrite the envelope-from when the
/// upstream provider (Gmail) won't accept a mismatched header.
fn from_mailbox() -> Result<Mailbox> {
    let name = env::var("MAIL_FROM_NAME").unwrap_or_else(|_| "PreXiv".to_string());
    let addr = env::var("MAIL_FROM_ADDRESS").or_else(|_| env::var("SMTP_USER"))
        .map_err(|_| anyhow!("MAIL_FROM_ADDRESS or SMTP_USER must be set"))?;
    Ok(Mailbox::new(Some(name), addr.parse().context("MAIL_FROM_ADDRESS not a valid email")?))
}

/// Sends the account verification email. `to` is the recipient address;
/// `username` is what we'll greet them by; `verify_link` is the absolute
/// URL the user should click. The function awaits the SMTP roundtrip — call
/// it from a spawned task if you don't want the HTTP response to block.
///
/// Dev-mode (SMTP_HOST unset) logs the link instead of sending. This is
/// intentional: a `cargo run` on a laptop doesn't need a real SMTP server,
/// and showing the link in stdout matches what the JS app did.
pub async fn send_verification_email(to: &str, username: &str, verify_link: &str) -> Result<()> {
    let tx = match transport()? {
        Some(t) => t,
        None => {
            tracing::warn!(
                target: "prexiv::email",
                %to, %username, link = %verify_link,
                "SMTP_HOST not configured — dev mode, verification link logged only"
            );
            return Ok(());
        }
    };

    let from = from_mailbox()?;
    let to_mb: Mailbox = to.parse().with_context(|| format!("invalid recipient: {to}"))?;

    let subject = "Verify your email — PreXiv";
    let body = format!(
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

    let email = Message::builder()
        .from(from)
        .to(to_mb)
        .subject(subject)
        .header(ContentType::TEXT_PLAIN)
        .body(body)
        .context("constructing email")?;

    tx.send(email).await.context("SMTP send")?;
    tracing::info!(target: "prexiv::email", %to, %username, "verification email sent");
    Ok(())
}
