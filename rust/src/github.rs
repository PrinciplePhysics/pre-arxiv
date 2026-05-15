//! GitHub OAuth account binding.
//!
//! PreXiv uses GitHub only as account-control proof for public write
//! permissions. We do not store the short-lived OAuth token; after the
//! authorization-code exchange we fetch GitHub's immutable numeric user id
//! and login, store those, and discard the token.

use anyhow::{anyhow, Context, Result};
use serde::Deserialize;

const GITHUB_AUTHORIZE_URL: &str = "https://github.com/login/oauth/authorize";
const GITHUB_TOKEN_URL: &str = "https://github.com/login/oauth/access_token";
const GITHUB_API_USER_URL: &str = "https://api.github.com/user";

#[derive(Debug, Clone)]
pub struct OAuthConfig {
    pub client_id: String,
    pub client_secret: String,
    pub redirect_uri: String,
}

impl OAuthConfig {
    pub fn authorize_url(&self, state: &str) -> String {
        format!(
            "{GITHUB_AUTHORIZE_URL}?client_id={}&redirect_uri={}&scope={}&state={}",
            urlencoding::encode(&self.client_id),
            urlencoding::encode(&self.redirect_uri),
            urlencoding::encode("read:user"),
            urlencoding::encode(state),
        )
    }
}

/// Required:
///   * GITHUB_CLIENT_ID
///   * GITHUB_CLIENT_SECRET
///
/// Optional:
///   * GITHUB_REDIRECT_URI. If absent, derived from APP_URL as
///     `{app_url}/auth/github/callback`.
pub fn oauth_config(app_url: Option<&str>) -> Result<Option<OAuthConfig>> {
    let client_id = match std::env::var("GITHUB_CLIENT_ID")
        .ok()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
    {
        Some(v) => v,
        None => return Ok(None),
    };
    let client_secret = std::env::var("GITHUB_CLIENT_SECRET")
        .ok()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .ok_or_else(|| anyhow!("GITHUB_CLIENT_SECRET is required when GITHUB_CLIENT_ID is set"))?;
    let redirect_uri = match std::env::var("GITHUB_REDIRECT_URI")
        .ok()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
    {
        Some(v) => v,
        None => {
            let base = app_url
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .ok_or_else(|| {
                    anyhow!("GITHUB_REDIRECT_URI is required when APP_URL is not set")
                })?;
            format!("{}/auth/github/callback", base.trim_end_matches('/'))
        }
    };
    Ok(Some(OAuthConfig {
        client_id,
        client_secret,
        redirect_uri,
    }))
}

#[derive(Debug, Deserialize)]
struct TokenResponse {
    #[serde(default)]
    access_token: Option<String>,
    #[serde(default)]
    error: Option<String>,
    #[serde(default)]
    error_description: Option<String>,
}

#[derive(Debug, Deserialize)]
struct GithubUserResponse {
    id: i64,
    login: String,
}

#[derive(Debug, Clone)]
pub struct AuthenticatedGithub {
    pub id: String,
    pub login: String,
}

pub async fn exchange_authorization_code(
    cfg: &OAuthConfig,
    code: &str,
) -> Result<AuthenticatedGithub> {
    let client = reqwest::Client::builder()
        .user_agent("PreXiv/0.1 (github-oauth)")
        .timeout(std::time::Duration::from_secs(15))
        .build()
        .context("building GitHub OAuth HTTP client")?;
    let resp = client
        .post(GITHUB_TOKEN_URL)
        .header("Accept", "application/json")
        .form(&[
            ("client_id", cfg.client_id.as_str()),
            ("client_secret", cfg.client_secret.as_str()),
            ("code", code),
            ("redirect_uri", cfg.redirect_uri.as_str()),
        ])
        .send()
        .await
        .context("POST GitHub OAuth token endpoint")?;
    let status = resp.status();
    if !status.is_success() {
        let body = resp.text().await.unwrap_or_default();
        return Err(anyhow!(
            "GitHub OAuth token exchange failed with HTTP {}: {}",
            status.as_u16(),
            body.chars().take(500).collect::<String>()
        ));
    }
    let token = resp
        .json::<TokenResponse>()
        .await
        .context("parsing GitHub OAuth token response")?;
    if let Some(err) = token.error.as_deref() {
        let details = token.error_description.unwrap_or_default();
        return Err(anyhow!("GitHub OAuth token error: {err} {details}"));
    }
    let access_token = token
        .access_token
        .as_deref()
        .filter(|s| !s.trim().is_empty())
        .ok_or_else(|| anyhow!("GitHub OAuth token response did not include access_token"))?;

    let gh_user = client
        .get(GITHUB_API_USER_URL)
        .header("Accept", "application/vnd.github+json")
        .header("X-GitHub-Api-Version", "2022-11-28")
        .bearer_auth(access_token)
        .send()
        .await
        .context("GET GitHub user endpoint")?
        .error_for_status()
        .context("GitHub user endpoint returned error")?
        .json::<GithubUserResponse>()
        .await
        .context("parsing GitHub user response")?;

    Ok(AuthenticatedGithub {
        id: gh_user.id.to_string(),
        login: gh_user.login,
    })
}

#[cfg(test)]
mod tests {
    use super::OAuthConfig;

    #[test]
    fn authorize_url_contains_state_and_callback() {
        let cfg = OAuthConfig {
            client_id: "client id".to_string(),
            client_secret: "secret".to_string(),
            redirect_uri: "https://prexiv.net/auth/github/callback".to_string(),
        };
        let url = cfg.authorize_url("state value");
        assert!(url.starts_with("https://github.com/login/oauth/authorize?"));
        assert!(url.contains("client_id=client%20id"));
        assert!(url.contains("redirect_uri=https%3A%2F%2Fprexiv.net%2Fauth%2Fgithub%2Fcallback"));
        assert!(url.contains("scope=read%3Auser"));
        assert!(url.contains("state=state%20value"));
    }
}
