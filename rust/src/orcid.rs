//! ORCID iD verification.
//!
//! Fetches the public record from `https://pub.orcid.org/v3.0/{orcid}/record`
//! (no OAuth required — this is the read-only public mirror) and
//! compares the registered name against the user's PreXiv display name.
//! A reasonable match flips `users.orcid_verified` to 1.
//!
//! This is NOT cryptographic proof that the PreXiv user IS the ORCID
//! holder — a determined adversary could paste any ORCID iD and the
//! check would pass if their display name happens to match. The point
//! is to raise the bar high enough that crank-tier 民科 spam attacks
//! don't bother: they'd need to (a) find an ORCID matching their fake
//! identity, (b) be willing to namespace their crank work to a real
//! person, and (c) hope nobody compares the ORCID record (which is
//! linked from the profile page) against their submissions.
//!
//! For stronger verification, a future iteration can wire the full
//! 3-legged ORCID OAuth flow (member API or public-API auth-code
//! grant) — at which point we'd flip a separate `orcid_oauth_verified`
//! column. The current public-record check is fine as a first pass.

use anyhow::{anyhow, Context, Result};
use serde::Deserialize;

const ORCID_RECORD_BASE: &str = "https://pub.orcid.org/v3.0";

/// Reduced view of the ORCID public-record JSON. ORCID's schema is
/// large; we only pull the bits we need for name comparison.
#[derive(Debug, Deserialize)]
pub struct OrcidRecord {
    #[serde(rename = "person")]
    pub person: Option<OrcidPerson>,
}

#[derive(Debug, Deserialize)]
pub struct OrcidPerson {
    pub name: Option<OrcidName>,
}

#[derive(Debug, Deserialize)]
pub struct OrcidName {
    #[serde(rename = "given-names")]
    pub given_names: Option<OrcidValue>,
    #[serde(rename = "family-name")]
    pub family_name: Option<OrcidValue>,
    #[serde(rename = "credit-name")]
    pub credit_name: Option<OrcidValue>,
}

#[derive(Debug, Deserialize)]
pub struct OrcidValue {
    pub value: Option<String>,
}

impl OrcidName {
    /// Canonical "First Last" string assembled from the name parts.
    /// Falls back to credit-name (the "display the public sees on ORCID")
    /// when given/family aren't both present.
    pub fn assembled(&self) -> String {
        if let Some(c) = self.credit_name.as_ref().and_then(|v| v.value.as_deref()) {
            if !c.trim().is_empty() {
                return c.trim().to_string();
            }
        }
        let g = self
            .given_names
            .as_ref()
            .and_then(|v| v.value.as_deref())
            .unwrap_or("")
            .trim();
        let f = self
            .family_name
            .as_ref()
            .and_then(|v| v.value.as_deref())
            .unwrap_or("")
            .trim();
        match (g.is_empty(), f.is_empty()) {
            (true, true) => String::new(),
            (true, false) => f.to_string(),
            (false, true) => g.to_string(),
            (false, false) => format!("{g} {f}"),
        }
    }
}

/// ORCID iDs are 16 digits in groups of four, with a trailing checksum
/// digit that can be `X`. Form: `0000-0000-0000-000X`. We accept the
/// canonical form only — pasting a URL gets normalised to the iD by
/// stripping `https://orcid.org/` if present.
pub fn normalize(raw: &str) -> Option<String> {
    let s = raw.trim();
    let s = s
        .trim_start_matches("https://orcid.org/")
        .trim_start_matches("http://orcid.org/")
        .trim_start_matches("orcid.org/");
    // Must look like NNNN-NNNN-NNNN-NNNX (X = digit or 'X')
    let bytes = s.as_bytes();
    if bytes.len() != 19 {
        return None;
    }
    for (i, &b) in bytes.iter().enumerate() {
        let want_dash = i == 4 || i == 9 || i == 14;
        if want_dash {
            if b != b'-' {
                return None;
            }
        } else if i == 18 {
            if !(b.is_ascii_digit() || b == b'X' || b == b'x') {
                return None;
            }
        } else if !b.is_ascii_digit() {
            return None;
        }
    }
    let mut out = s.to_string();
    // Normalise the checksum letter to uppercase X.
    if out.ends_with('x') {
        out.pop();
        out.push('X');
    }
    Some(out)
}

/// Fetch the public ORCID record for `orcid` (canonical
/// `0000-0000-0000-000X` form expected — call [`normalize`] first).
/// Returns a `Result` so the caller can render a clean error page on
/// network failure or "iD not found".
pub async fn fetch_record(orcid: &str) -> Result<OrcidRecord> {
    let url = format!("{ORCID_RECORD_BASE}/{orcid}/record");
    let client = reqwest::Client::builder()
        .user_agent("PreXiv/0.1 (orcid-verify)")
        .timeout(std::time::Duration::from_secs(10))
        .build()
        .context("building ORCID HTTP client")?;
    let resp = client
        .get(&url)
        .header("Accept", "application/json")
        .send()
        .await
        .with_context(|| format!("GET {url}"))?;
    if !resp.status().is_success() {
        return Err(anyhow!(
            "ORCID returned HTTP {} for {orcid}",
            resp.status().as_u16()
        ));
    }
    let rec = resp
        .json::<OrcidRecord>()
        .await
        .with_context(|| format!("parsing ORCID JSON for {orcid}"))?;
    Ok(rec)
}

/// Decide whether the ORCID record name "matches" the user's PreXiv
/// display name well enough to flip `orcid_verified`. The check is
/// deliberately forgiving — academic display names vary (initials,
/// honorifics, "Dr.", maiden names, hyphenated names). The criterion
/// we use:
///
///   1. Lowercase, strip non-letters from both sides.
///   2. If either is a (whitespace-separated) subset of the other,
///      it's a match.
///
/// So "Jane K. Doe" matches "Jane Doe"; "Doe, Jane" matches "Jane Doe"
/// after the punctuation strip; "jdoe" alone does NOT match
/// "Jane Doe". That's the intended behavior: a username-only display
/// name forces the user to set their real name in `display_name`
/// before they can prove they own the ORCID.
pub fn name_matches(orcid_name: &str, display_name: &str) -> bool {
    let a = token_set(orcid_name);
    let b = token_set(display_name);
    if a.is_empty() || b.is_empty() {
        return false;
    }
    a.is_subset(&b) || b.is_subset(&a)
}

fn token_set(s: &str) -> std::collections::BTreeSet<String> {
    s.split(|c: char| !c.is_alphabetic())
        .filter(|t| t.len() >= 2)
        .map(|t| t.to_ascii_lowercase())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalize_canonical() {
        assert_eq!(
            normalize("0000-0002-1825-0097"),
            Some("0000-0002-1825-0097".to_string())
        );
    }

    #[test]
    fn normalize_url_form() {
        assert_eq!(
            normalize(" https://orcid.org/0000-0002-1825-0097 "),
            Some("0000-0002-1825-0097".to_string())
        );
    }

    #[test]
    fn normalize_uppercases_x() {
        assert_eq!(
            normalize("0000-0001-5109-371x"),
            Some("0000-0001-5109-371X".to_string())
        );
    }

    #[test]
    fn normalize_rejects_garbage() {
        assert!(normalize("not-an-orcid").is_none());
        assert!(normalize("0000-0002-1825-009").is_none()); // too short
        assert!(normalize("0000.0002.1825.0097").is_none()); // wrong sep
    }

    #[test]
    fn name_match_basic() {
        assert!(name_matches("Jane Doe", "Jane Doe"));
        assert!(name_matches("Jane K. Doe", "Jane Doe"));
        assert!(name_matches("Doe, Jane", "Jane Doe"));
    }

    #[test]
    fn name_match_initial_only() {
        // Strict mode: a name token has to be ≥2 chars. "J Doe" gives
        // only {"doe"} which is a subset of {"jane","doe"} → match.
        assert!(name_matches("J Doe", "Jane Doe"));
    }

    #[test]
    fn name_match_username_rejected() {
        // A username-style display name with no human name doesn't match.
        assert!(!name_matches("Jane Doe", "jdoe"));
    }
}
