//! ORCID OAuth/OpenID Connect binding.
//!
//! PreXiv treats ORCID as account-ownership proof only when the user
//! completes ORCID's authorization-code flow. Pasted ORCID iDs and
//! public-record name matches are intentionally not accepted as a
//! verification signal.

use anyhow::{anyhow, bail, Context, Result};
use jsonwebtoken::jwk::JwkSet;
use jsonwebtoken::{decode, decode_header, Algorithm, DecodingKey, Validation};
use serde::Deserialize;

const ORCID_DEFAULT_BASE: &str = "https://orcid.org";

#[derive(Debug, Clone)]
pub struct OAuthConfig {
    pub client_id: String,
    pub client_secret: String,
    pub base_url: String,
    pub redirect_uri: String,
}

impl OAuthConfig {
    pub fn authorize_url(&self, state: &str, nonce: &str) -> String {
        format!(
            "{}/oauth/authorize?client_id={}&response_type=code&scope={}&redirect_uri={}&state={}&nonce={}",
            self.base_url.trim_end_matches('/'),
            urlencoding::encode(&self.client_id),
            urlencoding::encode("openid"),
            urlencoding::encode(&self.redirect_uri),
            urlencoding::encode(state),
            urlencoding::encode(nonce),
        )
    }

    fn token_url(&self) -> String {
        format!("{}/oauth/token", self.base_url.trim_end_matches('/'))
    }

    fn discovery_url(&self) -> String {
        format!(
            "{}/.well-known/openid-configuration",
            self.base_url.trim_end_matches('/')
        )
    }
}

/// Build ORCID OAuth config from env.
///
/// Required when enabling OAuth:
///   * ORCID_CLIENT_ID
///   * ORCID_CLIENT_SECRET
///
/// Optional:
///   * ORCID_BASE_URL, defaults to https://orcid.org. Use
///     https://sandbox.orcid.org for sandbox testing.
///   * ORCID_REDIRECT_URI. If absent, derived from APP_URL/state.app_url
///     as `{app_url}/auth/orcid/callback`.
pub fn oauth_config(app_url: Option<&str>) -> Result<Option<OAuthConfig>> {
    let client_id = match std::env::var("ORCID_CLIENT_ID")
        .ok()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
    {
        Some(v) => v,
        None => return Ok(None),
    };
    let client_secret = std::env::var("ORCID_CLIENT_SECRET")
        .ok()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .ok_or_else(|| anyhow!("ORCID_CLIENT_SECRET is required when ORCID_CLIENT_ID is set"))?;
    let base_url = std::env::var("ORCID_BASE_URL")
        .ok()
        .map(|s| s.trim().trim_end_matches('/').to_string())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| ORCID_DEFAULT_BASE.to_string());
    let redirect_uri = match std::env::var("ORCID_REDIRECT_URI")
        .ok()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
    {
        Some(v) => v,
        None => {
            let base = app_url
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .ok_or_else(|| anyhow!("ORCID_REDIRECT_URI is required when APP_URL is not set"))?;
            format!("{}/auth/orcid/callback", base.trim_end_matches('/'))
        }
    };
    Ok(Some(OAuthConfig {
        client_id,
        client_secret,
        base_url,
        redirect_uri,
    }))
}

#[derive(Debug, Deserialize)]
struct OAuthTokenResponse {
    #[serde(default)]
    orcid: Option<String>,
    #[serde(default)]
    name: Option<String>,
    id_token: Option<String>,
}

#[derive(Debug, Clone)]
pub struct AuthenticatedOrcid {
    pub orcid: String,
    pub name: Option<String>,
}

#[derive(Debug, Deserialize)]
struct OpenIdDiscovery {
    issuer: String,
    jwks_uri: String,
}

#[derive(Debug, Deserialize)]
struct IdTokenClaims {
    sub: String,
    #[serde(default)]
    nonce: Option<String>,
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    given_name: Option<String>,
    #[serde(default)]
    family_name: Option<String>,
}

pub async fn exchange_authorization_code(
    cfg: &OAuthConfig,
    code: &str,
    nonce: &str,
) -> Result<AuthenticatedOrcid> {
    let client = reqwest::Client::builder()
        .user_agent("PreXiv/0.1 (orcid-oauth)")
        .timeout(std::time::Duration::from_secs(15))
        .build()
        .context("building ORCID OAuth HTTP client")?;
    let resp = client
        .post(cfg.token_url())
        .header("Accept", "application/json")
        .form(&[
            ("client_id", cfg.client_id.as_str()),
            ("client_secret", cfg.client_secret.as_str()),
            ("grant_type", "authorization_code"),
            ("code", code),
            ("redirect_uri", cfg.redirect_uri.as_str()),
        ])
        .send()
        .await
        .context("POST ORCID OAuth token endpoint")?;
    let status = resp.status();
    if !status.is_success() {
        let body = resp.text().await.unwrap_or_default();
        return Err(anyhow!(
            "ORCID OAuth token exchange failed with HTTP {}: {}",
            status.as_u16(),
            body.chars().take(500).collect::<String>()
        ));
    }
    let token = resp
        .json::<OAuthTokenResponse>()
        .await
        .context("parsing ORCID OAuth token response")?;
    let id_token = token
        .id_token
        .as_deref()
        .ok_or_else(|| anyhow!("ORCID OpenID token response did not include id_token"))?;
    let claims = verify_id_token(&client, cfg, id_token, nonce).await?;
    let Some(orcid) = normalize(&claims.sub) else {
        return Err(anyhow!("ORCID id_token subject was not a valid ORCID iD"));
    };
    if let Some(token_orcid) = token.orcid.as_deref().and_then(normalize) {
        if token_orcid != orcid {
            bail!(
                "ORCID token response mismatch: id_token sub {} != response orcid {}",
                orcid,
                token_orcid
            );
        }
    }
    let claim_name = claims.name.or_else(|| {
        let given = claims.given_name.unwrap_or_default();
        let family = claims.family_name.unwrap_or_default();
        let joined = format!("{} {}", given.trim(), family.trim())
            .trim()
            .to_string();
        if joined.is_empty() {
            None
        } else {
            Some(joined)
        }
    });
    Ok(AuthenticatedOrcid {
        orcid,
        name: token.name.or(claim_name),
    })
}

async fn verify_id_token(
    client: &reqwest::Client,
    cfg: &OAuthConfig,
    id_token: &str,
    expected_nonce: &str,
) -> Result<IdTokenClaims> {
    let discovery = client
        .get(cfg.discovery_url())
        .header("Accept", "application/json")
        .send()
        .await
        .context("GET ORCID OpenID discovery")?
        .error_for_status()
        .context("ORCID OpenID discovery returned error")?
        .json::<OpenIdDiscovery>()
        .await
        .context("parsing ORCID OpenID discovery")?;
    if discovery.issuer.trim().is_empty() || discovery.jwks_uri.trim().is_empty() {
        bail!("ORCID OpenID discovery did not include issuer and jwks_uri");
    }

    let header = decode_header(id_token).context("decoding ORCID id_token header")?;
    if header.alg != Algorithm::RS256 {
        bail!(
            "ORCID id_token used unexpected JWT algorithm {:?}",
            header.alg
        );
    }
    let kid = header
        .kid
        .as_deref()
        .ok_or_else(|| anyhow!("ORCID id_token did not include a key id"))?;
    let jwks = client
        .get(&discovery.jwks_uri)
        .header("Accept", "application/json")
        .send()
        .await
        .context("GET ORCID JWKS")?
        .error_for_status()
        .context("ORCID JWKS returned error")?
        .json::<JwkSet>()
        .await
        .context("parsing ORCID JWKS")?;
    let jwk = jwks
        .find(kid)
        .ok_or_else(|| anyhow!("ORCID JWKS did not contain signing key {kid}"))?;
    let decoding_key = DecodingKey::from_jwk(jwk).context("building ORCID JWT decoding key")?;
    let mut validation = Validation::new(Algorithm::RS256);
    validation.leeway = 60;
    validation.set_issuer(&[discovery.issuer.as_str()]);
    validation.set_audience(&[cfg.client_id.as_str()]);
    let claims = decode::<IdTokenClaims>(id_token, &decoding_key, &validation)
        .context("verifying ORCID id_token signature and claims")?
        .claims;
    if claims.nonce.as_deref() != Some(expected_nonce) {
        bail!("ORCID id_token nonce did not match the login session");
    }
    Ok(claims)
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
    fn authorize_url_uses_openid_nonce() {
        let cfg = OAuthConfig {
            client_id: "APP-123".to_string(),
            client_secret: "secret".to_string(),
            base_url: "https://sandbox.orcid.org".to_string(),
            redirect_uri: "https://prexiv.example/auth/orcid/callback".to_string(),
        };
        let url = cfg.authorize_url("state value", "nonce value");
        assert!(url.starts_with("https://sandbox.orcid.org/oauth/authorize?"));
        assert!(url.contains("scope=openid"));
        assert!(url.contains("state=state%20value"));
        assert!(url.contains("nonce=nonce%20value"));
        assert!(url.contains("redirect_uri=https%3A%2F%2Fprexiv.example%2Fauth%2Forcid%2Fcallback"));
    }
}
