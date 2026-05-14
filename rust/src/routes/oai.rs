//! OAI-PMH 2.0 endpoint.
//!
//! Single entry point `GET /oai` dispatching by `?verb=`. Supports the
//! six required verbs:
//!
//!   Identify             — repository description
//!   ListMetadataFormats  — formats we serve (oai_dc only for now)
//!   ListSets             — sets = categories
//!   ListIdentifiers      — header-only listing
//!   ListRecords          — header+metadata listing
//!   GetRecord            — single record
//!
//! Metadata format: oai_dc (Dublin Core). That's what every major
//! academic harvester (OpenAIRE, BASE, CORE, COAR Notify) expects as
//! the baseline. Future formats (arXivRaw, datacite) can be added.
//!
//! Identifier scheme: `oai:prexiv:<arxiv_like_id>` (e.g.
//! `oai:prexiv:prexiv:260513.3n9jxa`). The repo prefix lets harvesters
//! distinguish our records from records they pull from elsewhere.

use axum::extract::{Query, State};
use axum::http::header;
use axum::response::IntoResponse;
use chrono::{DateTime, NaiveDateTime, Utc};
use serde::Deserialize;

use crate::error::AppResult;
use crate::state::AppState;

const ID_PREFIX: &str = "oai:prexiv:";
const PAGE_LIST: usize = 100;

#[derive(Debug, Deserialize)]
#[allow(dead_code)]
pub struct OaiQuery {
    pub verb: Option<String>,
    pub identifier: Option<String>,
    pub metadata_prefix: Option<String>,
    #[serde(rename = "metadataPrefix")]
    pub metadata_prefix_alt: Option<String>,
    pub set: Option<String>,
    pub from: Option<String>,
    pub until: Option<String>,
    pub resumption_token: Option<String>,
    #[serde(rename = "resumptionToken")]
    pub resumption_token_alt: Option<String>,
}

fn x(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&apos;")
}

fn iso(ts: &NaiveDateTime) -> String {
    DateTime::<Utc>::from_naive_utc_and_offset(*ts, Utc)
        .format("%Y-%m-%dT%H:%M:%SZ")
        .to_string()
}

fn base(state: &AppState) -> String {
    state
        .app_url
        .as_deref()
        .unwrap_or("http://localhost:3001")
        .trim_end_matches('/')
        .to_string()
}

fn envelope_open(response_date: &str, request_attrs: &str, base_url: &str) -> String {
    format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<OAI-PMH xmlns="http://www.openarchives.org/OAI/2.0/"
         xmlns:xsi="http://www.w3.org/2001/XMLSchema-instance"
         xsi:schemaLocation="http://www.openarchives.org/OAI/2.0/ http://www.openarchives.org/OAI/2.0/OAI-PMH.xsd">
<responseDate>{response_date}</responseDate>
<request{request_attrs}>{base_url}/oai</request>
"#
    )
}

fn envelope_close() -> String {
    "</OAI-PMH>\n".to_string()
}

fn error(code: &str, message: &str, base_url: &str) -> String {
    let now = chrono::Utc::now().format("%Y-%m-%dT%H:%M:%SZ").to_string();
    let mut out = envelope_open(&now, "", base_url);
    out.push_str(&format!(
        "<error code=\"{}\">{}</error>\n",
        x(code),
        x(message)
    ));
    out.push_str(&envelope_close());
    out
}

// ── dispatch ─────────────────────────────────────────────────────────

pub async fn oai(
    State(state): State<AppState>,
    Query(q): Query<OaiQuery>,
) -> AppResult<impl IntoResponse> {
    let base_url = base(&state);
    let xml = match q.verb.as_deref() {
        Some("Identify") => identify(&state).await,
        Some("ListMetadataFormats") => list_metadata_formats(&base_url),
        Some("ListSets") => list_sets(&base_url),
        Some("ListIdentifiers") => list_identifiers(&state, &q).await,
        Some("ListRecords") => list_records(&state, &q).await,
        Some("GetRecord") => get_record(&state, &q).await,
        Some(v) => error("badVerb", &format!("Unknown verb '{v}'"), &base_url),
        None => error("badVerb", "Missing verb parameter", &base_url),
    };
    Ok((
        [(header::CONTENT_TYPE, "application/xml; charset=utf-8")],
        xml,
    ))
}

// ── Identify ─────────────────────────────────────────────────────────

async fn identify(state: &AppState) -> String {
    let base_url = base(state);
    let earliest: Option<(NaiveDateTime,)> =
        sqlx::query_as("SELECT MIN(created_at) FROM manuscripts WHERE created_at IS NOT NULL")
            .fetch_optional(&state.pool)
            .await
            .ok()
            .flatten();
    let earliest_str = earliest
        .map(|(t,)| iso(&t))
        .unwrap_or_else(|| "2026-01-01T00:00:00Z".to_string());

    let now = chrono::Utc::now().format("%Y-%m-%dT%H:%M:%SZ").to_string();
    let mut out = envelope_open(&now, " verb=\"Identify\"", &base_url);
    out.push_str("<Identify>\n");
    out.push_str("  <repositoryName>PreXiv</repositoryName>\n");
    out.push_str(&format!("  <baseURL>{}/oai</baseURL>\n", x(&base_url)));
    out.push_str("  <protocolVersion>2.0</protocolVersion>\n");
    out.push_str("  <adminEmail>noreply@prexiv.local</adminEmail>\n");
    out.push_str(&format!(
        "  <earliestDatestamp>{}</earliestDatestamp>\n",
        x(&earliest_str)
    ));
    out.push_str("  <deletedRecord>persistent</deletedRecord>\n");
    out.push_str("  <granularity>YYYY-MM-DDThh:mm:ssZ</granularity>\n");
    out.push_str("  <description>\n");
    out.push_str(
        "    <oai-identifier xmlns=\"http://www.openarchives.org/OAI/2.0/oai-identifier\"\n",
    );
    out.push_str("                    xmlns:xsi=\"http://www.w3.org/2001/XMLSchema-instance\"\n");
    out.push_str("                    xsi:schemaLocation=\"http://www.openarchives.org/OAI/2.0/oai-identifier http://www.openarchives.org/OAI/2.0/oai-identifier.xsd\">\n");
    out.push_str("      <scheme>oai</scheme>\n");
    out.push_str("      <repositoryIdentifier>prexiv</repositoryIdentifier>\n");
    out.push_str("      <delimiter>:</delimiter>\n");
    out.push_str("      <sampleIdentifier>oai:prexiv:prexiv:260513.3n9jxa</sampleIdentifier>\n");
    out.push_str("    </oai-identifier>\n");
    out.push_str("  </description>\n");
    out.push_str("</Identify>\n");
    out.push_str(&envelope_close());
    out
}

// ── ListMetadataFormats ──────────────────────────────────────────────

fn list_metadata_formats(base_url: &str) -> String {
    let now = chrono::Utc::now().format("%Y-%m-%dT%H:%M:%SZ").to_string();
    let mut out = envelope_open(&now, " verb=\"ListMetadataFormats\"", base_url);
    out.push_str("<ListMetadataFormats>\n");
    out.push_str("  <metadataFormat>\n");
    out.push_str("    <metadataPrefix>oai_dc</metadataPrefix>\n");
    out.push_str("    <schema>http://www.openarchives.org/OAI/2.0/oai_dc.xsd</schema>\n");
    out.push_str(
        "    <metadataNamespace>http://www.openarchives.org/OAI/2.0/oai_dc/</metadataNamespace>\n",
    );
    out.push_str("  </metadataFormat>\n");
    out.push_str("</ListMetadataFormats>\n");
    out.push_str(&envelope_close());
    out
}

// ── ListSets ─────────────────────────────────────────────────────────

fn list_sets(base_url: &str) -> String {
    let now = chrono::Utc::now().format("%Y-%m-%dT%H:%M:%SZ").to_string();
    let mut out = envelope_open(&now, " verb=\"ListSets\"", base_url);
    out.push_str("<ListSets>\n");
    for c in crate::categories::CATEGORIES.iter() {
        out.push_str("  <set>\n");
        out.push_str(&format!("    <setSpec>{}</setSpec>\n", x(c.id)));
        out.push_str(&format!("    <setName>{}</setName>\n", x(c.name)));
        out.push_str("  </set>\n");
    }
    out.push_str("</ListSets>\n");
    out.push_str(&envelope_close());
    out
}

// ── ListIdentifiers / ListRecords / GetRecord ───────────────────────

struct DbRow {
    arxiv_like_id: Option<String>,
    title: String,
    abstract_: String,
    authors: String,
    category: String,
    created_at: Option<NaiveDateTime>,
    license: Option<String>,
    doi: Option<String>,
    withdrawn: i64,
    withdrawn_at: Option<NaiveDateTime>,
}

fn dc_metadata(r: &DbRow, base_url: &str) -> String {
    let slug = r.arxiv_like_id.as_deref().unwrap_or("");
    let mut out = String::new();
    out.push_str("<metadata>\n");
    out.push_str("  <oai_dc:dc xmlns:oai_dc=\"http://www.openarchives.org/OAI/2.0/oai_dc/\"\n");
    out.push_str("             xmlns:dc=\"http://purl.org/dc/elements/1.1/\"\n");
    out.push_str("             xmlns:xsi=\"http://www.w3.org/2001/XMLSchema-instance\"\n");
    out.push_str("             xsi:schemaLocation=\"http://www.openarchives.org/OAI/2.0/oai_dc/ http://www.openarchives.org/OAI/2.0/oai_dc.xsd\">\n");
    out.push_str(&format!("    <dc:title>{}</dc:title>\n", x(&r.title)));
    for a in r
        .authors
        .split(',')
        .map(|s| s.trim())
        .filter(|s| !s.is_empty())
    {
        out.push_str(&format!("    <dc:creator>{}</dc:creator>\n", x(a)));
    }
    out.push_str(&format!(
        "    <dc:subject>{}</dc:subject>\n",
        x(&r.category)
    ));
    out.push_str(&format!(
        "    <dc:description>{}</dc:description>\n",
        x(&r.abstract_)
    ));
    out.push_str("    <dc:publisher>PreXiv</dc:publisher>\n");
    if let Some(t) = r.created_at {
        out.push_str(&format!("    <dc:date>{}</dc:date>\n", iso(&t)));
    }
    out.push_str("    <dc:type>Preprint</dc:type>\n");
    let public_slug = slug.strip_prefix("prexiv:").unwrap_or(slug);
    out.push_str(&format!(
        "    <dc:identifier>{}/abs/{}</dc:identifier>\n",
        x(base_url),
        x(public_slug)
    ));
    if let Some(d) = &r.doi {
        out.push_str(&format!(
            "    <dc:identifier>doi:{}</dc:identifier>\n",
            x(d)
        ));
    }
    out.push_str("    <dc:language>en</dc:language>\n");
    if let Some(l) = &r.license {
        out.push_str(&format!("    <dc:rights>{}</dc:rights>\n", x(l)));
    }
    out.push_str("  </oai_dc:dc>\n");
    out.push_str("</metadata>\n");
    out
}

fn header_xml(r: &DbRow) -> String {
    let slug = r.arxiv_like_id.as_deref().unwrap_or("");
    let stamp = r
        .created_at
        .as_ref()
        .map(iso)
        .unwrap_or_else(|| chrono::Utc::now().format("%Y-%m-%dT%H:%M:%SZ").to_string());
    let status_attr = if r.withdrawn != 0 {
        " status=\"deleted\""
    } else {
        ""
    };
    let mut out = String::new();
    out.push_str(&format!("    <header{status_attr}>\n"));
    out.push_str(&format!(
        "      <identifier>{}{}</identifier>\n",
        ID_PREFIX,
        x(slug)
    ));
    let used = if r.withdrawn != 0 {
        r.withdrawn_at.as_ref().map(iso).unwrap_or(stamp)
    } else {
        stamp
    };
    out.push_str(&format!("      <datestamp>{used}</datestamp>\n"));
    out.push_str(&format!("      <setSpec>{}</setSpec>\n", x(&r.category)));
    out.push_str("    </header>\n");
    out
}

async fn fetch_rows(state: &AppState, set: Option<&str>, limit: usize) -> Vec<DbRow> {
    let sql = match set {
        Some(_) => "SELECT arxiv_like_id, title, abstract, authors, category, created_at, license, doi, withdrawn, withdrawn_at
                    FROM manuscripts WHERE category = ? ORDER BY id DESC LIMIT ?".to_string(),
        None => "SELECT arxiv_like_id, title, abstract, authors, category, created_at, license, doi, withdrawn, withdrawn_at
                 FROM manuscripts ORDER BY id DESC LIMIT ?".to_string(),
    };
    let q = sqlx::query_as::<
        _,
        (
            Option<String>,
            String,
            String,
            String,
            String,
            Option<NaiveDateTime>,
            Option<String>,
            Option<String>,
            i64,
            Option<NaiveDateTime>,
        ),
    >(&sql);
    let rows = match set {
        Some(s) => q.bind(s).bind(limit as i64).fetch_all(&state.pool).await,
        None => q.bind(limit as i64).fetch_all(&state.pool).await,
    };
    rows.unwrap_or_default()
        .into_iter()
        .map(
            |(
                slug,
                title,
                abstract_,
                authors,
                category,
                created_at,
                license,
                doi,
                withdrawn,
                withdrawn_at,
            )| DbRow {
                arxiv_like_id: slug,
                title,
                abstract_,
                authors,
                category,
                created_at,
                license,
                doi,
                withdrawn,
                withdrawn_at,
            },
        )
        .collect()
}

async fn list_identifiers(state: &AppState, q: &OaiQuery) -> String {
    let base_url = base(state);
    if let Some(err) = require_metadata_prefix(q, &base_url) {
        return err;
    }
    let rows = fetch_rows(state, q.set.as_deref(), PAGE_LIST).await;
    if rows.is_empty() {
        return error("noRecordsMatch", "No records match the query.", &base_url);
    }
    let now = chrono::Utc::now().format("%Y-%m-%dT%H:%M:%SZ").to_string();
    let mut out = envelope_open(&now, " verb=\"ListIdentifiers\"", &base_url);
    out.push_str("<ListIdentifiers>\n");
    for r in &rows {
        out.push_str(&header_xml(r));
    }
    out.push_str("</ListIdentifiers>\n");
    out.push_str(&envelope_close());
    out
}

async fn list_records(state: &AppState, q: &OaiQuery) -> String {
    let base_url = base(state);
    if let Some(err) = require_metadata_prefix(q, &base_url) {
        return err;
    }
    let rows = fetch_rows(state, q.set.as_deref(), PAGE_LIST).await;
    if rows.is_empty() {
        return error("noRecordsMatch", "No records match the query.", &base_url);
    }
    let now = chrono::Utc::now().format("%Y-%m-%dT%H:%M:%SZ").to_string();
    let mut out = envelope_open(&now, " verb=\"ListRecords\"", &base_url);
    out.push_str("<ListRecords>\n");
    for r in &rows {
        out.push_str("  <record>\n");
        out.push_str(&header_xml(r));
        if r.withdrawn == 0 {
            out.push_str(&dc_metadata(r, &base_url));
        }
        out.push_str("  </record>\n");
    }
    out.push_str("</ListRecords>\n");
    out.push_str(&envelope_close());
    out
}

async fn get_record(state: &AppState, q: &OaiQuery) -> String {
    let base_url = base(state);
    if let Some(err) = require_metadata_prefix(q, &base_url) {
        return err;
    }
    let id = match q.identifier.as_deref() {
        Some(i) => i,
        None => return error("badArgument", "Missing identifier", &base_url),
    };
    let slug = match id.strip_prefix(ID_PREFIX) {
        Some(s) => s,
        None => return error("idDoesNotExist", "Unknown identifier prefix", &base_url),
    };
    let row: Option<DbRow> = sqlx::query_as::<_, (Option<String>, String, String, String, String, Option<NaiveDateTime>, Option<String>, Option<String>, i64, Option<NaiveDateTime>)>(
        "SELECT arxiv_like_id, title, abstract, authors, category, created_at, license, doi, withdrawn, withdrawn_at
         FROM manuscripts WHERE arxiv_like_id = ? LIMIT 1",
    )
    .bind(slug)
    .fetch_optional(&state.pool)
    .await
    .ok()
    .flatten()
    .map(|(slug, title, abstract_, authors, category, created_at, license, doi, withdrawn, withdrawn_at)| DbRow {
        arxiv_like_id: slug,
        title,
        abstract_,
        authors,
        category,
        created_at,
        license,
        doi,
        withdrawn,
        withdrawn_at,
    });
    let r = match row {
        Some(r) => r,
        None => return error("idDoesNotExist", "Record not found", &base_url),
    };
    let now = chrono::Utc::now().format("%Y-%m-%dT%H:%M:%SZ").to_string();
    let mut out = envelope_open(&now, " verb=\"GetRecord\"", &base_url);
    out.push_str("<GetRecord>\n  <record>\n");
    out.push_str(&header_xml(&r));
    if r.withdrawn == 0 {
        out.push_str(&dc_metadata(&r, &base_url));
    }
    out.push_str("  </record>\n</GetRecord>\n");
    out.push_str(&envelope_close());
    out
}

fn require_metadata_prefix(q: &OaiQuery, base_url: &str) -> Option<String> {
    let pref = q
        .metadata_prefix
        .as_deref()
        .or(q.metadata_prefix_alt.as_deref());
    match pref {
        Some("oai_dc") => None,
        Some(other) => Some(error(
            "cannotDisseminateFormat",
            &format!("Unsupported metadataPrefix '{other}'; we support 'oai_dc'."),
            base_url,
        )),
        None => Some(error("badArgument", "metadataPrefix is required", base_url)),
    }
}
