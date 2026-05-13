use maud::{html, Markup};

use crate::models::Manuscript;

use super::layout::{layout, PageCtx};

pub fn render(ctx: &PageCtx, m: &Manuscript, base_url: &str) -> Markup {
    let bib = bibtex(m, base_url);
    let ris = ris(m, base_url);
    let plain = plain_text(m);
    let body = html! {
        div.page-header {
            h1 { "Cite: " (m.title) }
            p.muted {
                "Citation formats for "
                @if let Some(id) = &m.arxiv_like_id { code { (id) } }
                ". Use "
                a href={ "/m/" (m.arxiv_like_id.as_deref().unwrap_or("")) "/cite.bib" } { "/cite.bib" }
                " or "
                a href={ "/m/" (m.arxiv_like_id.as_deref().unwrap_or("")) "/cite.ris" } { "/cite.ris" }
                " for raw files, or copy from the blocks below."
            }
        }
        section.ms-section {
            h2.ms-section-h { "BibTeX" }
            div.copy-pre-wrap {
                button.copy-pre-btn type="button" { "Copy BibTeX" }
                pre { (bib) }
            }
        }
        section.ms-section {
            h2.ms-section-h { "RIS" }
            div.copy-pre-wrap {
                button.copy-pre-btn type="button" { "Copy RIS" }
                pre { (ris) }
            }
        }
        section.ms-section {
            h2.ms-section-h { "Plain text" }
            div.copy-pre-wrap {
                button.copy-pre-btn type="button" { "Copy text" }
                pre { (plain) }
            }
        }
        p { a.btn-secondary href={ "/m/" (m.arxiv_like_id.as_deref().unwrap_or("")) } { "← Back to manuscript" } }
    };
    layout("Cite", ctx, body)
}

fn first_author(authors: &str) -> &str {
    authors
        .split(|c| c == ';' || c == ',')
        .next()
        .unwrap_or(authors)
        .trim()
}

fn citekey(m: &Manuscript) -> String {
    let first = first_author(&m.authors);
    let surname: String = first
        .split_whitespace()
        .last()
        .unwrap_or("anon")
        .chars()
        .filter(|c| c.is_ascii_alphanumeric())
        .collect();
    let year = m
        .created_at
        .map(|t| t.format("%Y").to_string())
        .unwrap_or_else(|| "unknown".into());
    let id_tail = m
        .arxiv_like_id
        .as_deref()
        .and_then(|s| s.split(':').nth(1))
        .unwrap_or("0")
        .replace('.', "");
    format!("{}{}_{}", surname.to_lowercase(), year, id_tail)
}

pub fn bibtex(m: &Manuscript, base_url: &str) -> String {
    let key = citekey(m);
    let url = m
        .arxiv_like_id
        .as_deref()
        .map(|id| manuscript_url(base_url, id))
        .unwrap_or_default();
    let year = m
        .created_at
        .map(|t| t.format("%Y").to_string())
        .unwrap_or_default();
    let mut s = String::new();
    s.push_str(&format!("@misc{{{key},\n"));
    s.push_str(&format!("  title        = {{{}}},\n", m.title));
    s.push_str(&format!("  author       = {{{}}},\n", m.authors));
    s.push_str(&format!("  year         = {{{year}}},\n"));
    if let Some(id) = &m.arxiv_like_id {
        s.push_str(&format!("  note         = {{PreXiv id: {id}}},\n"));
    }
    if let Some(doi) = &m.doi {
        s.push_str(&format!("  doi          = {{{doi}}},\n"));
    }
    if !url.is_empty() {
        s.push_str(&format!("  url          = {{{url}}},\n"));
    }
    s.push_str("}\n");
    s
}

pub fn ris(m: &Manuscript, base_url: &str) -> String {
    let year = m
        .created_at
        .map(|t| t.format("%Y").to_string())
        .unwrap_or_default();
    let mut s = String::new();
    s.push_str("TY  - GEN\n");
    s.push_str(&format!("TI  - {}\n", m.title));
    for a in m.authors.split(';') {
        s.push_str(&format!("AU  - {}\n", a.trim()));
    }
    if !year.is_empty() {
        s.push_str(&format!("PY  - {year}\n"));
    }
    if let Some(doi) = &m.doi {
        s.push_str(&format!("DO  - {doi}\n"));
    }
    if let Some(id) = &m.arxiv_like_id {
        s.push_str(&format!("ID  - {id}\n"));
        s.push_str(&format!("UR  - {}\n", manuscript_url(base_url, id)));
    }
    s.push_str("AB  - ");
    s.push_str(&m.r#abstract);
    s.push('\n');
    s.push_str("ER  -\n");
    s
}

fn manuscript_url(base_url: &str, id: &str) -> String {
    format!("{}/m/{id}", base_url.trim_end_matches('/'))
}

fn plain_text(m: &Manuscript) -> String {
    let year = m
        .created_at
        .map(|t| t.format("%Y").to_string())
        .unwrap_or_default();
    format!(
        "{authors} ({year}). {title}. PreXiv {id}{doi}.",
        authors = m.authors,
        year = year,
        title = m.title,
        id = m.arxiv_like_id.as_deref().unwrap_or(""),
        doi = m
            .doi
            .as_deref()
            .map(|d| format!(", doi:{d}"))
            .unwrap_or_default(),
    )
}
