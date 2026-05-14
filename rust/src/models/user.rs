use chrono::NaiveDateTime;
use serde::Serialize;
use sqlx::FromRow;

use crate::crypto;

#[derive(Debug, Clone, FromRow, Serialize)]
pub struct User {
    pub id: i64,
    pub username: String,
    /// Plaintext email — populated either from the legacy plaintext
    /// column or by decrypting [`email_enc`]. Always reflect the
    /// real email by the time you hand a `User` to a request handler;
    /// load via [`crate::auth::load_user`] etc. so the resolve step runs.
    pub email: String,
    pub display_name: Option<String>,
    pub affiliation: Option<String>,
    pub bio: Option<String>,
    pub karma: Option<i64>,
    pub is_admin: i64,
    pub email_verified: i64,
    pub orcid: Option<String>,
    pub created_at: Option<NaiveDateTime>,
    /// AES-256-GCM ciphertext of the email. Never sent to clients.
    #[serde(skip)]
    pub email_enc: Option<Vec<u8>>,
    /// Legacy public-record ORCID name match from older deployments.
    /// Kept for schema compatibility only; the product now treats ORCID
    /// as verified solely through the OAuth fields below.
    #[serde(default)]
    pub orcid_verified: i64,
    #[serde(default)]
    pub institutional_email: i64,
    /// Authenticated ORCID iD obtained through ORCID OAuth. Unlike the
    /// public-record name match above, this proves the browser user
    /// signed into and authorized that ORCID account.
    #[serde(default)]
    #[sqlx(default)]
    pub orcid_oauth_verified: i64,
    #[serde(default)]
    #[sqlx(default)]
    pub orcid_oauth_verified_at: Option<NaiveDateTime>,
    #[serde(default)]
    #[sqlx(default)]
    pub orcid_oauth_sub: Option<String>,
}

impl User {
    pub fn is_admin(&self) -> bool {
        self.is_admin != 0
    }
    pub fn is_verified(&self) -> bool {
        self.email_verified != 0
    }
    pub fn is_verified_or_admin(&self) -> bool {
        self.is_verified() || self.is_admin()
    }
    /// `true` if the user has an authenticated ORCID OAuth binding, or
    /// has verified ownership of an institutional-looking email domain.
    /// Legacy ORCID name matches are intentionally excluded.
    pub fn is_verified_scholar(&self) -> bool {
        self.orcid_oauth_verified != 0
            || (self.email_verified != 0 && self.institutional_email != 0)
    }
    pub fn is_orcid_oauth_verified(&self) -> bool {
        self.orcid_oauth_verified != 0
    }
    pub fn is_institutional_email(&self) -> bool {
        self.institutional_email != 0
    }

    /// Replace `self.email` with the decrypted form of `email_enc` if
    /// present. Falls back to whatever plaintext is already in `email`
    /// when `email_enc` is NULL (legacy row before backfill, or after
    /// a deliberate plaintext-only insert). A decrypt failure is
    /// surfaced loudly — we'd rather refuse to serve than render an
    /// account with the wrong email.
    pub fn resolve_email(&mut self) {
        if let Some(enc) = self.email_enc.as_ref() {
            match crypto::open_email(enc) {
                Ok(s) => self.email = s,
                Err(e) => {
                    tracing::error!(user_id = self.id, error = %e, "decrypt email_enc failed");
                }
            }
        }
    }
}
