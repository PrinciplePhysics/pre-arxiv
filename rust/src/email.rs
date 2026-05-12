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
    "cern.ch", "esa.int", "iter.org",
    // US national labs & science agencies
    "nasa.gov", "nist.gov", "nih.gov", "noaa.gov", "usgs.gov",
    "anl.gov", "lanl.gov", "ornl.gov", "lbl.gov",
    "llnl.gov", "pnnl.gov", "sandia.gov",
    "fnal.gov", "bnl.gov", "jlab.org", "ameslab.gov",
    "slac.stanford.edu",
    // France
    "cnrs.fr", "inria.fr", "inserm.fr", "cea.fr", "ihes.fr",
    // Germany
    "mpg.de", "fz-juelich.de", "kit.edu", "desy.de", "hzdr.de", "helmholtz.de",
    // UK extras
    "stfc.ac.uk", "ukri.org", "sanger.ac.uk", "diamond.ac.uk",
    // Japan extras
    "riken.jp", "kek.jp", "jaxa.jp", "aist.go.jp", "naoj.org",
    // China extras (in addition to .ac.cn / .edu.cn)
    "cas.cn", "ihep.ac.cn", "ucas.ac.cn",
    // Switzerland
    "ethz.ch", "epfl.ch", "psi.ch",
    // Italy
    "infn.it", "ictp.it", "sissa.it",
    // Spain
    "csic.es", "ifae.es",
    // Netherlands
    "nikhef.nl", "cwi.nl", "knaw.nl",
    // Israel
    "weizmann.ac.il", "technion.ac.il",
    // Korea
    "kaist.ac.kr", "snu.ac.kr", "postech.ac.kr",
    "kasi.re.kr", "kist.re.kr", "kisti.re.kr",
    // Russia / CIS
    "jinr.ru", "ras.ru",
    // Canada
    "triumf.ca",
    // Australia (in addition to .edu.au)
    "csiro.au", "ansto.gov.au",
    // India extras
    "tifr.res.in",
    // Industry R&D — coarse: ALL of @microsoft.com etc. pass, not just
    // research subdomains. The badge is "their org is large enough to
    // employ research staff," not "this specific person is a researcher."
    "research.google.com", "deepmind.com",
    "anthropic.com", "openai.com",
    "microsoft.com", "research.microsoft.com",
    "meta.com", "fb.com",
    "ibm.com", "research.ibm.com",
    "intel.com", "nvidia.com",
    "amazon.science",
    "apple.com",
    "bell-labs.com", "nokia-bell-labs.com",
    "huawei.com",
];

pub fn is_institutional(email: &str) -> bool {
    let lower = email.trim().to_ascii_lowercase();
    let domain = match lower.split_once('@') {
        Some((_, d)) if !d.is_empty() => d,
        _ => return false,
    };
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

/// Sends the email-change confirmation to the NEW address. Confirming
/// the link is how the user proves they actually control that mailbox.
pub async fn send_email_change_confirmation(
    new_address: &str,
    username: &str,
    confirm_link: &str,
) -> Result<()> {
    send_transactional(
        new_address,
        "Confirm your new email — PreXiv",
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
