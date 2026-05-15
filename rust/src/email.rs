#![allow(clippy::items_after_test_module)]
//! Outbound email — Gmail API.
//!
//! Direct-to-MX SMTP is not reliable from the production host because
//! outbound port 25 is blocked. Gmail API uses HTTPS/443 and sends from a
//! Google mailbox or verified send-as alias such as noreply@prexiv.net.
//!
//! Config, Gmail API:
//!
//!   GMAIL_CLIENT_ID      OAuth client id
//!   GMAIL_CLIENT_SECRET  OAuth client secret
//!   GMAIL_REFRESH_TOKEN  refresh token with gmail.send scope
//!   GMAIL_USER_ID        "me" by default
//!   MAIL_FROM_NAME       PreXiv by default
//!   MAIL_FROM_ADDRESS    noreply@prexiv.net by default
//!
//! Config, Gmail SMTP fallback:
//!
//!   SMTP_HOST            smtp.gmail.com by default
//!   SMTP_PORT            587 by default
//!   SMTP_USERNAME        full Gmail / Google Workspace address
//!   SMTP_PASSWORD        app password (spaces are stripped)
//!   MAIL_FROM_NAME       PreXiv by default
//!   MAIL_FROM_ADDRESS    noreply@prexiv.net by default
//!
//! Dev mode (missing Gmail credentials and NODE_ENV != production) logs that
//! email is not sent. Production treats missing/partial credentials as an
//! operational error.

use std::env;
use std::time::Duration;

use anyhow::{anyhow, Context, Result};
use base64::{engine::general_purpose, Engine as _};
use lettre::message::{Mailbox, Message};
use lettre::transport::smtp::authentication::Credentials;
use lettre::{SmtpTransport, Transport};
use rand::RngCore;
use serde::Deserialize;
use serde_json::json;

const GOOGLE_TOKEN_ENDPOINT: &str = "https://oauth2.googleapis.com/token";
const GMAIL_SEND_ENDPOINT: &str = "https://gmail.googleapis.com/gmail/v1/users";

// ─── Institutional-email allowlist ────────────────────────────────────
//
// `is_institutional()` returns true if an email address's domain looks
// like a research institution: `.edu`, `.ac.<cc>` (UK/JP/KR/NZ/IN/ZA/CN…),
// `.edu.<cc>` (AU/MX/CN/SG…), `.gov` agencies that fund science, plus a
// hand-curated allowlist of well-known international R&D organisations.
// Used at register / email-change to flip `users.institutional_email`.
//
// This is a coarse trust signal, not a credential. A .edu mailbox means
// "an institution's IT department vouched for this person enough to
// issue them an account" — much harder to obtain than a free consumer
// mailbox, which is what 民科 spam attacks would typically use.
// False positives (marketing emails from .edu domains) are harmless;
// false negatives (a researcher using gmail) are fine because the
// ORCID verification path covers them.

const ACADEMIC_SLDS: &[&str] = &[
    "ac",  // .ac.uk, .ac.jp, .ac.kr, .ac.in, .ac.nz, .ac.za, .ac.il, .ac.cn …
    "edu", // .edu.au, .edu.cn, .edu.sg, .edu.mx, .edu.hk, .edu.tw …
    "res", // .res.in (Indian research orgs: IISc, CSIR institutes, etc.)
];

const INSTITUTIONAL_DOMAINS: &[&str] = &[
    // Multi-national / inter-governmental
    "cern.ch",
    "esa.int",
    "iter.org",
    // US national labs & science agencies
    "nasa.gov",
    "nist.gov",
    "nih.gov",
    "noaa.gov",
    "usgs.gov",
    "anl.gov",
    "lanl.gov",
    "ornl.gov",
    "lbl.gov",
    "llnl.gov",
    "pnnl.gov",
    "sandia.gov",
    "fnal.gov",
    "bnl.gov",
    "jlab.org",
    "ameslab.gov",
    "slac.stanford.edu",
    // France
    "cnrs.fr",
    "inria.fr",
    "inserm.fr",
    "cea.fr",
    "ihes.fr",
    // Germany
    "mpg.de",
    "fz-juelich.de",
    "kit.edu",
    "desy.de",
    "hzdr.de",
    "helmholtz.de",
    // UK extras
    "stfc.ac.uk",
    "ukri.org",
    "sanger.ac.uk",
    "diamond.ac.uk",
    // Japan extras
    "riken.jp",
    "kek.jp",
    "jaxa.jp",
    "aist.go.jp",
    "naoj.org",
    // China extras (in addition to .ac.cn / .edu.cn)
    "cas.cn",
    "ihep.ac.cn",
    "ucas.ac.cn",
    // Switzerland
    "ethz.ch",
    "epfl.ch",
    "psi.ch",
    // Italy
    "infn.it",
    "ictp.it",
    "sissa.it",
    // Spain
    "csic.es",
    "ifae.es",
    // Netherlands
    "nikhef.nl",
    "cwi.nl",
    "knaw.nl",
    // Israel
    "weizmann.ac.il",
    "technion.ac.il",
    // Korea
    "kaist.ac.kr",
    "snu.ac.kr",
    "postech.ac.kr",
    "kasi.re.kr",
    "kist.re.kr",
    "kisti.re.kr",
    // Russia / CIS
    "jinr.ru",
    "ras.ru",
    // Canada
    "triumf.ca",
    // Australia (in addition to .edu.au)
    "csiro.au",
    "ansto.gov.au",
    // India extras
    "tifr.res.in",
    // Industry R&D — coarse: ALL of @microsoft.com etc. pass, not just
    // research subdomains. The badge is "their org is large enough to
    // employ research staff," not "this specific person is a researcher."
    "research.google.com",
    "deepmind.com",
    "anthropic.com",
    "openai.com",
    "microsoft.com",
    "research.microsoft.com",
    "meta.com",
    "fb.com",
    "ibm.com",
    "research.ibm.com",
    "intel.com",
    "nvidia.com",
    "amazon.science",
    "apple.com",
    "bell-labs.com",
    "nokia-bell-labs.com",
    "huawei.com",
];

pub fn is_institutional(email: &str) -> bool {
    let lower = email.trim().to_ascii_lowercase();
    let domain = match lower.split_once('@') {
        Some((local, d)) if !local.is_empty() && !d.is_empty() => d,
        _ => return false,
    };
    if domain.split('.').any(str::is_empty) {
        return false;
    }
    // Exact match or subdomain match against the curated list.
    for cur in INSTITUTIONAL_DOMAINS {
        if domain == *cur || domain.ends_with(&format!(".{cur}")) {
            return true;
        }
    }
    let labels: Vec<&str> = domain.split('.').collect();
    // .edu top-level (US universities)
    if labels.len() >= 2 && labels[labels.len() - 1] == "edu" {
        return true;
    }
    // .gov top-level (US federal agencies — most that touch science
    // are already in the curated list, but the sweep catches the rest)
    if labels.len() >= 2 && labels[labels.len() - 1] == "gov" {
        return true;
    }
    // .<sld>.<cc> structural patterns
    if labels.len() >= 3 {
        let sld = labels[labels.len() - 2];
        if ACADEMIC_SLDS.contains(&sld) {
            return true;
        }
    }
    false
}

#[cfg(test)]
mod institutional_tests {
    use super::is_institutional;

    #[test]
    fn edu_passes() {
        assert!(is_institutional("alice@harvard.edu"));
        assert!(is_institutional("alice@cs.stanford.edu"));
    }

    #[test]
    fn ac_cc_passes() {
        assert!(is_institutional("alice@cam.ac.uk"));
        assert!(is_institutional("alice@u-tokyo.ac.jp"));
    }

    #[test]
    fn edu_cc_passes() {
        assert!(is_institutional("alice@unimelb.edu.au"));
        assert!(is_institutional("alice@tsinghua.edu.cn"));
    }

    #[test]
    fn curated_passes() {
        assert!(is_institutional("alice@cern.ch"));
        assert!(is_institutional("alice@cnrs.fr"));
        assert!(is_institutional("alice@mpg.de"));
    }

    #[test]
    fn consumer_fails() {
        assert!(!is_institutional("alice@gmail.com"));
        assert!(!is_institutional("alice@qq.com"));
        assert!(!is_institutional("alice@163.com"));
        assert!(!is_institutional("alice@outlook.com"));
    }

    #[test]
    fn malformed_fails() {
        assert!(!is_institutional(""));
        assert!(!is_institutional("not-an-email"));
        assert!(!is_institutional("@no-local.edu"));
    }
}

#[derive(Debug, Clone)]
struct GmailConfig {
    client_id: String,
    client_secret: String,
    refresh_token: String,
    user_id: String,
}

#[derive(Debug, Clone)]
struct SmtpConfig {
    host: String,
    port: u16,
    username: String,
    password: String,
}

#[derive(Debug, Deserialize)]
struct GoogleTokenResponse {
    access_token: String,
}

#[derive(Debug, Deserialize)]
struct GmailSendResponse {
    id: Option<String>,
}

fn nonempty_env(key: &str) -> Option<String> {
    env::var(key)
        .ok()
        .map(|v| v.trim().to_string())
        .filter(|v| !v.is_empty())
}

fn gmail_config() -> Result<Option<GmailConfig>> {
    let client_id = nonempty_env("GMAIL_CLIENT_ID");
    let client_secret = nonempty_env("GMAIL_CLIENT_SECRET");
    let refresh_token = nonempty_env("GMAIL_REFRESH_TOKEN");
    match (client_id, client_secret, refresh_token) {
        (None, None, None) => Ok(None),
        (Some(client_id), Some(client_secret), Some(refresh_token)) => Ok(Some(GmailConfig {
            client_id,
            client_secret,
            refresh_token,
            user_id: nonempty_env("GMAIL_USER_ID").unwrap_or_else(|| "me".to_string()),
        })),
        _ => Err(anyhow!(
            "partial Gmail API configuration; set GMAIL_CLIENT_ID, GMAIL_CLIENT_SECRET, and GMAIL_REFRESH_TOKEN"
        )),
    }
}

fn smtp_config() -> Result<Option<SmtpConfig>> {
    let username = nonempty_env("SMTP_USERNAME");
    let password = nonempty_env("SMTP_PASSWORD");
    match (username, password) {
        (None, None) => Ok(None),
        (Some(username), Some(password)) => {
            validate_email_header("SMTP_USERNAME", &username)?;
            let port = nonempty_env("SMTP_PORT")
                .map(|v| {
                    v.parse::<u16>()
                        .context("SMTP_PORT must be an integer from 1 to 65535")
                })
                .transpose()?
                .unwrap_or(587);
            Ok(Some(SmtpConfig {
                host: nonempty_env("SMTP_HOST").unwrap_or_else(|| "smtp.gmail.com".to_string()),
                port,
                username,
                password: password.chars().filter(|c| !c.is_whitespace()).collect(),
            }))
        }
        _ => Err(anyhow!(
            "partial SMTP configuration; set SMTP_USERNAME and SMTP_PASSWORD"
        )),
    }
}

pub fn delivery_configured() -> bool {
    gmail_config().ok().flatten().is_some() || smtp_config().ok().flatten().is_some()
}

pub fn inline_token_fallback_enabled() -> bool {
    if env::var("PREXIV_ALLOW_INLINE_EMAIL_TOKENS").as_deref() == Ok("1") {
        return true;
    }
    env::var("NODE_ENV").as_deref() != Ok("production") && !delivery_configured()
}

fn from_pair() -> Result<(String, String)> {
    let name = env::var("MAIL_FROM_NAME").unwrap_or_else(|_| "PreXiv".to_string());
    let addr = env::var("MAIL_FROM_ADDRESS").unwrap_or_else(|_| "noreply@prexiv.net".to_string());
    reject_header_breaks("MAIL_FROM_NAME", &name)?;
    validate_email_header("MAIL_FROM_ADDRESS", &addr)?;
    Ok((name, addr))
}

fn reject_header_breaks(label: &str, value: &str) -> Result<()> {
    if value.contains('\r') || value.contains('\n') {
        return Err(anyhow!("{label} contains a header line break"));
    }
    Ok(())
}

fn validate_email_header(label: &str, value: &str) -> Result<()> {
    reject_header_breaks(label, value)?;
    if !value.contains('@') || value.contains('<') || value.contains('>') {
        return Err(anyhow!("{label} is not a plain email address"));
    }
    Ok(())
}

fn rfc2047(value: &str) -> String {
    if value.is_ascii() {
        value.to_string()
    } else {
        format!(
            "=?UTF-8?B?{}?=",
            general_purpose::STANDARD.encode(value.as_bytes())
        )
    }
}

fn mailbox(name: &str, addr: &str) -> String {
    format!("{} <{}>", rfc2047(name), addr)
}

fn message_id_host(from_addr: &str) -> &str {
    from_addr.split('@').nth(1).unwrap_or("prexiv.net")
}

fn build_mime_message(to: &str, subject: &str, text_body: &str) -> Result<String> {
    validate_email_header("recipient", to)?;
    reject_header_breaks("subject", subject)?;
    let (from_name, from_addr) = from_pair()?;
    let mut random = [0u8; 16];
    rand::thread_rng().fill_bytes(&mut random);
    let msg_id = general_purpose::URL_SAFE_NO_PAD.encode(random);
    let from = mailbox(&from_name, &from_addr);
    let host = message_id_host(&from_addr);
    Ok(format!(
        "From: {from}\r\n\
         To: <{to}>\r\n\
         Subject: {}\r\n\
         Date: {}\r\n\
         Message-ID: <{msg_id}@{host}>\r\n\
         MIME-Version: 1.0\r\n\
         Content-Type: text/plain; charset=UTF-8\r\n\
         Content-Transfer-Encoding: 8bit\r\n\
         \r\n\
         {text_body}",
        rfc2047(subject),
        chrono::Utc::now().to_rfc2822()
    ))
}

async fn gmail_access_token(client: &reqwest::Client, cfg: &GmailConfig) -> Result<String> {
    let resp = client
        .post(GOOGLE_TOKEN_ENDPOINT)
        .form(&[
            ("client_id", cfg.client_id.as_str()),
            ("client_secret", cfg.client_secret.as_str()),
            ("refresh_token", cfg.refresh_token.as_str()),
            ("grant_type", "refresh_token"),
        ])
        .send()
        .await
        .context("refreshing Gmail access token")?;
    let status = resp.status();
    if !status.is_success() {
        let body = resp.text().await.unwrap_or_default();
        let snippet: String = body.chars().take(500).collect();
        return Err(anyhow!(
            "Google token endpoint returned {status}: {snippet}"
        ));
    }
    Ok(resp
        .json::<GoogleTokenResponse>()
        .await
        .context("parsing Google token response")?
        .access_token)
}

fn parse_mailbox(label: &str, value: String) -> Result<Mailbox> {
    value
        .parse::<Mailbox>()
        .with_context(|| format!("parsing {label} mailbox"))
}

async fn send_via_smtp(to: &str, subject: &str, text_body: &str, cfg: SmtpConfig) -> Result<()> {
    let (from_name, from_addr) = from_pair()?;
    let from = parse_mailbox("MAIL_FROM_ADDRESS", mailbox(&from_name, &from_addr))?;
    let to_mailbox = parse_mailbox("recipient", to.to_string())?;
    let subject = subject.to_string();
    let log_subject = subject.clone();
    let text_body = text_body.to_string();
    tokio::task::spawn_blocking(move || -> Result<()> {
        let email = Message::builder()
            .from(from)
            .to(to_mailbox)
            .subject(subject)
            .body(text_body)
            .context("building SMTP message")?;
        let creds = Credentials::new(cfg.username, cfg.password);
        let transport = SmtpTransport::starttls_relay(&cfg.host)
            .with_context(|| format!("creating SMTP relay for {}", cfg.host))?
            .port(cfg.port)
            .credentials(creds)
            .build();
        transport.send(&email).context("sending SMTP message")?;
        Ok(())
    })
    .await
    .context("joining SMTP send task")??;
    tracing::info!(
        target: "prexiv::email",
        subject = %log_subject,
        "transactional email accepted by SMTP relay"
    );
    Ok(())
}

/// Low-level transactional send. Both verification and password-reset
/// emails route through here so the wire format / timeout / error
/// handling stay in one place.
async fn send_transactional(to: &str, subject: &str, text_body: &str) -> Result<()> {
    if let Some(cfg) = smtp_config()? {
        return send_via_smtp(to, subject, text_body, cfg).await;
    }

    let cfg = match gmail_config()? {
        Some(cfg) => cfg,
        None if env::var("NODE_ENV").as_deref() != Ok("production") => {
            tracing::warn!(
                target: "prexiv::email",
                %subject,
                "Gmail API credentials not configured — dev mode, email not sent"
            );
            return Ok(());
        }
        None => return Err(anyhow!("Gmail API credentials not configured")),
    };

    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(10))
        .user_agent("PreXiv/0.1 (+https://github.com/prexiv/prexiv)")
        .build()
        .context("building reqwest client")?;

    let access_token = gmail_access_token(&client, &cfg).await?;
    let mime = build_mime_message(to, subject, text_body)?;
    let raw = general_purpose::URL_SAFE_NO_PAD.encode(mime.as_bytes());
    let endpoint = format!(
        "{}/{}/messages/send",
        GMAIL_SEND_ENDPOINT,
        urlencoding::encode(&cfg.user_id)
    );
    let resp = client
        .post(endpoint)
        .bearer_auth(access_token)
        .header("accept", "application/json")
        .header("content-type", "application/json")
        .json(&json!({ "raw": raw }))
        .send()
        .await
        .context("posting to Gmail API")?;

    let status = resp.status();
    if !status.is_success() {
        let body = resp.text().await.unwrap_or_default();
        let snippet: String = body.chars().take(500).collect();
        return Err(anyhow!("Gmail API returned {status}: {snippet}"));
    }
    let gmail_id = resp
        .json::<GmailSendResponse>()
        .await
        .ok()
        .and_then(|r| r.id)
        .unwrap_or_default();

    tracing::info!(
        target: "prexiv::email",
        %subject,
        %gmail_id,
        "transactional email accepted by Gmail API"
    );
    Ok(())
}

/// Sends the email-verification email.
pub async fn send_verification_email(to: &str, username: &str, verify_link: &str) -> Result<()> {
    send_transactional(
        to,
        "Verify your email - PreXiv",
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

/// Sends the email-change confirmation to the NEW address. Confirming
/// the link is how the user proves they actually control that mailbox.
pub async fn send_email_change_confirmation(
    new_address: &str,
    username: &str,
    confirm_link: &str,
) -> Result<()> {
    send_transactional(
        new_address,
        "Confirm your new email - PreXiv",
        &format!(
            "Hi {username},

You asked us to change the email on your PreXiv account to this address. Click
the link below to confirm — until you do, your account email stays unchanged
and password-reset mail continues to go to the previous address:

  {confirm_link}

The link expires in 24 hours. If you didn't request this change, ignore this
email; the request will simply expire and the account email won't be touched.

— PreXiv
"
        ),
    )
    .await
}

/// Sends the password-reset email. Shorter TTL (1h) is reflected in
/// the body copy so the user knows to act quickly.
pub async fn send_password_reset_email(to: &str, username: &str, reset_link: &str) -> Result<()> {
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

#[cfg(test)]
mod outbound_tests {
    use super::{build_mime_message, rfc2047};

    #[test]
    fn mime_message_is_gmail_api_ready() {
        let msg = build_mime_message("reader@example.com", "Verify your email - PreXiv", "hello")
            .unwrap();
        assert!(msg.contains("From: PreXiv <noreply@prexiv.net>\r\n"));
        assert!(msg.contains("To: <reader@example.com>\r\n"));
        assert!(msg.contains("Content-Type: text/plain; charset=UTF-8\r\n"));
        assert!(msg.ends_with("hello"));
    }

    #[test]
    fn non_ascii_subjects_are_encoded() {
        assert_eq!(rfc2047("PreXiv"), "PreXiv");
        assert!(rfc2047("Verify — PreXiv").starts_with("=?UTF-8?B?"));
    }

    #[test]
    fn header_injection_is_rejected() {
        let err = build_mime_message("reader@example.com\nBcc: x@y", "Subject", "body")
            .unwrap_err()
            .to_string();
        assert!(err.contains("line break"));
    }
}
