use chrono::NaiveDateTime;
use serde::Serialize;
use sqlx::FromRow;

/// Full manuscript row, used on the manuscript-detail page.
#[derive(Debug, Clone, FromRow, Serialize)]
pub struct Manuscript {
    pub id: i64,
    pub arxiv_like_id: Option<String>,
    pub doi: Option<String>,
    pub submitter_id: i64,
    pub title: String,
    pub r#abstract: String,
    pub authors: String,
    pub category: String,
    pub pdf_path: Option<String>,
    pub external_url: Option<String>,
    pub conductor_type: String,
    pub conductor_ai_model: String,
    pub conductor_ai_model_public: i64,
    pub conductor_human: Option<String>,
    pub conductor_human_public: i64,
    pub conductor_role: Option<String>,
    pub conductor_notes: Option<String>,
    pub agent_framework: Option<String>,
    pub has_auditor: i64,
    pub auditor_name: Option<String>,
    pub auditor_affiliation: Option<String>,
    pub auditor_role: Option<String>,
    pub auditor_statement: Option<String>,
    pub auditor_orcid: Option<String>,
    pub view_count: Option<i64>,
    pub score: Option<i64>,
    pub comment_count: Option<i64>,
    pub withdrawn: i64,
    pub withdrawn_reason: Option<String>,
    pub withdrawn_at: Option<NaiveDateTime>,
    pub created_at: Option<NaiveDateTime>,
    pub updated_at: Option<NaiveDateTime>,
    #[sqlx(default)]
    pub license: Option<String>,
    #[sqlx(default)]
    pub ai_training: Option<String>,
    /// Latest version_number; selected only by routes that need to
    /// render the version chrome (manuscript-detail, /versions list).
    /// SELECTs that don't query the column get 0 here — they don't use
    /// it anyway.
    #[sqlx(default)]
    pub current_version: i64,
}

/// One historical snapshot of a manuscript. Versions are immutable
/// once written; revising a manuscript means inserting a new row here
/// with version_number = current_version + 1 AND mirroring the new
/// values onto the manuscripts row so the listings show the latest.
#[derive(Debug, Clone, FromRow, Serialize)]
pub struct ManuscriptVersion {
    pub id: i64,
    pub manuscript_id: i64,
    pub version_number: i64,
    pub title: String,
    pub r#abstract: String,
    pub authors: String,
    pub category: String,
    pub pdf_path: Option<String>,
    pub external_url: Option<String>,
    pub conductor_notes: Option<String>,
    pub license: Option<String>,
    pub ai_training: Option<String>,
    pub revision_note: Option<String>,
    pub revised_at: Option<NaiveDateTime>,
}

/// Slim row used in listings (home, search results, profile pages).
#[derive(Debug, Clone, FromRow, Serialize)]
pub struct ManuscriptListItem {
    pub id: i64,
    pub arxiv_like_id: Option<String>,
    pub doi: Option<String>,
    pub title: String,
    pub authors: String,
    pub category: String,
    pub conductor_type: String,
    pub conductor_ai_model: String,
    pub conductor_ai_model_public: i64,
    pub conductor_human: Option<String>,
    pub conductor_human_public: i64,
    pub has_auditor: i64,
    pub auditor_name: Option<String>,
    pub score: Option<i64>,
    pub comment_count: Option<i64>,
    pub withdrawn: i64,
    pub created_at: Option<NaiveDateTime>,
}

/// Three-way audit status, computed from has_auditor + auditor_name vs
/// conductor_human. Listing rows + manuscript page agree on the rule.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AuditStatus { Unaudited, SelfAudited, ThirdParty }

impl ManuscriptListItem {
    pub fn audit_status(&self) -> AuditStatus {
        if self.has_auditor == 0 { return AuditStatus::Unaudited; }
        let an = self.auditor_name.as_deref().map(str::trim).unwrap_or("");
        let ch = self.conductor_human.as_deref().map(str::trim).unwrap_or("");
        if !an.is_empty() && an == ch { AuditStatus::SelfAudited }
        else { AuditStatus::ThirdParty }
    }
}

impl Manuscript {
    pub fn is_withdrawn(&self) -> bool {
        self.withdrawn != 0
    }
}

impl ManuscriptListItem {
    pub fn is_withdrawn(&self) -> bool {
        self.withdrawn != 0
    }

    /// Display string for the conductor: "Alice + GPT-4" / "Autonomous: GPT-4" /
    /// "Anonymous + GPT-4" depending on privacy flags. Mirrors the JS helper.
    pub fn conductor_label(&self) -> String {
        match self.conductor_type.as_str() {
            "ai-agent" => {
                if self.conductor_ai_model_public != 0 {
                    format!("Autonomous: {}", self.conductor_ai_model)
                } else {
                    "Autonomous (model private)".to_string()
                }
            }
            _ => {
                let human = if self.conductor_human_public != 0 {
                    self.conductor_human.as_deref().unwrap_or("Anonymous")
                } else {
                    "Anonymous"
                };
                let model = if self.conductor_ai_model_public != 0 {
                    self.conductor_ai_model.as_str()
                } else {
                    "AI (private)"
                };
                format!("{human} + {model}")
            }
        }
    }
}
