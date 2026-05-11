//! Bearer-token API authentication for AI agents.
//!
//! Token format: `prexiv_` + 36 base64url chars (27 random bytes), exactly
//! matching the JS app's `generateToken()`. Stored as SHA-256 hex in
//! `api_tokens.token_hash`; the plaintext is shown to the caller exactly
//! once at creation and never persisted.
//!
//! `ApiUser` is an axum extractor that pulls the bearer from the
//! `Authorization` header, looks the hash up, honours `expires_at`, and
//! touches `last_used_at` so token-management UIs can show recency.

use axum::extract::{FromRef, FromRequestParts};
use axum::http::header;
use axum::http::request::Parts;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;
use base64::Engine;
use rand::RngCore;
use serde_json::json;
use sha2::{Digest, Sha256};
use sqlx::SqlitePool;

use crate::models::User;
use crate::state::AppState;

pub const TOKEN_PREFIX: &str = "prexiv_";

/// Mint a fresh API token. Plaintext only — caller must hash with
/// `hash_token` before storing in the DB.
pub fn generate_token() -> String {
    let mut bytes = [0u8; 27];
    rand::thread_rng().fill_bytes(&mut bytes);
    let b64 = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(bytes);
    format!("{TOKEN_PREFIX}{b64}")
}

/// SHA-256 hex of the plaintext token.
pub fn hash_token(plain: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(plain.as_bytes());
    hex::encode(hasher.finalize())
}

fn extract_bearer(parts: &Parts) -> Option<String> {
    let h = parts.headers.get(header::AUTHORIZATION)?.to_str().ok()?;
    let mut it = h.splitn(2, ' ');
    let scheme = it.next()?;
    if !scheme.eq_ignore_ascii_case("bearer") {
        return None;
    }
    Some(it.next()?.trim().to_string())
}

/// Look up the user that owns the given plaintext bearer token. Honours
/// `expires_at` (returns None if expired). Touches `last_used_at` on a
/// successful match.
pub async fn find_user_by_bearer(pool: &SqlitePool, plain: &str) -> Option<User> {
    let h = hash_token(plain);
    let row = sqlx::query_as::<_, (i64, i64, Option<String>)>(
        r#"SELECT t.id, t.user_id, t.expires_at
           FROM api_tokens t
           WHERE t.token_hash = ?
           LIMIT 1"#,
    )
    .bind(&h)
    .fetch_optional(pool)
    .await
    .ok()
    .flatten()?;
    let (token_id, user_id, expires_at): (i64, i64, Option<String>) = row;

    // Expiry check (SQLite stores as ISO-8601 text via CURRENT_TIMESTAMP).
    if let Some(exp) = expires_at {
        if let Ok(t) = chrono::NaiveDateTime::parse_from_str(&exp, "%Y-%m-%d %H:%M:%S") {
            if t < chrono::Utc::now().naive_utc() {
                return None;
            }
        }
    }

    let _ = sqlx::query("UPDATE api_tokens SET last_used_at = CURRENT_TIMESTAMP WHERE id = ?")
        .bind(token_id)
        .execute(pool)
        .await;

    sqlx::query_as::<_, User>(
        r#"SELECT id, username, email, display_name, affiliation, bio,
                  karma, is_admin, email_verified, orcid, created_at
           FROM users WHERE id = ?"#,
    )
    .bind(user_id)
    .fetch_optional(pool)
    .await
    .ok()
    .flatten()
}

/// Required-bearer extractor. Use on agent-only endpoints; returns 401
/// JSON if no valid token is present.
pub struct ApiUser(pub User);

impl<S> FromRequestParts<S> for ApiUser
where
    AppState: FromRef<S>,
    S: Send + Sync,
{
    type Rejection = Response;
    async fn from_request_parts(parts: &mut Parts, state: &S) -> Result<Self, Self::Rejection> {
        let app: AppState = AppState::from_ref(state);
        let plain = match extract_bearer(parts) {
            Some(t) => t,
            None => {
                return Err((
                    StatusCode::UNAUTHORIZED,
                    Json(json!({
                        "error": "missing or malformed Authorization header",
                        "hint": "send `Authorization: Bearer prexiv_…` — mint a token at /me/tokens"
                    })),
                )
                    .into_response());
            }
        };
        match find_user_by_bearer(&app.pool, &plain).await {
            Some(u) => Ok(ApiUser(u)),
            None => Err((
                StatusCode::UNAUTHORIZED,
                Json(json!({"error": "invalid or expired bearer token"})),
            )
                .into_response()),
        }
    }
}

/// Same as ApiUser but the user must be email_verified. Mirrors the JS
/// `requireApiVerified` gate on POST /manuscripts.
pub struct ApiVerifiedUser(pub User);

impl<S> FromRequestParts<S> for ApiVerifiedUser
where
    AppState: FromRef<S>,
    S: Send + Sync,
{
    type Rejection = Response;
    async fn from_request_parts(parts: &mut Parts, state: &S) -> Result<Self, Self::Rejection> {
        let ApiUser(u) = ApiUser::from_request_parts(parts, state).await?;
        if u.is_verified() || u.is_admin() {
            Ok(ApiVerifiedUser(u))
        } else {
            Err((
                StatusCode::FORBIDDEN,
                Json(json!({"error": "email not verified — verify your account first"})),
            )
                .into_response())
        }
    }
}
