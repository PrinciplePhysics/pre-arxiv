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
    /// "Verified scholar" signals — see migration `0013_verified_scholar.sql`.
    /// Either flag alone makes the user a verified scholar (see [`is_verified_scholar`]).
    #[serde(default)]
    pub orcid_verified: i64,
    #[serde(default)]
    pub institutional_email: i64,
}

impl User {
    pub fn is_admin(&self) -> bool {
        self.is_admin != 0
    }
    pub fn is_verified(&self) -> bool {
        self.email_verified != 0
    }
    pub fn display(&self) -> &str {
        self.display_name.as_deref().unwrap_or(&self.username)
    }

    /// `true` if the user has either a verified ORCID iD on file or
    /// registered with an institutional-looking email domain.
    pub fn is_verified_scholar(&self) -> bool {
        self.orcid_verified != 0 || self.institutional_email != 0
    }
    pub fn is_orcid_verified(&self) -> bool { self.orcid_verified != 0 }
    pub fn is_institutional_email(&self) -> bool { self.institutional_email != 0 }

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
