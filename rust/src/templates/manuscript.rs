use maud::{html, Markup};

use crate::models::comment::CommentWithAuthor;
use crate::models::Manuscript;

use super::layout::layout;

pub fn render(m: &Manuscript, comments: &[CommentWithAuthor]) -> Markup {
    let body = html! {
        article.manuscript {
            h1.title { (m.title) }
            @if m.is_withdrawn() {
                div.withdrawn-banner {
                    "This manuscript has been withdrawn."
                    @if let Some(r) = &m.withdrawn_reason {
                        " Reason: " (r)
                    }
                }
            }
            p.authors { strong { "Authors: " } (m.authors) }
            p.meta {
                span.category { (m.category) } " · "
                @if let Some(id) = &m.arxiv_like_id { span.id { (id) } " · " }
                @if let Some(doi) = &m.doi { a.doi href={ "https://doi.org/" (doi) } { (doi) } " · " }
                span.score { (m.score.unwrap_or(0)) " points · " (m.comment_count.unwrap_or(0)) " comments" }
            }
            section.conductor {
                h2 { "Conductor" }
                p {
                    @match m.conductor_type.as_str() {
                        "ai-agent" => {
                            "Autonomous AI agent"
                            @if m.conductor_ai_model_public != 0 { ": " (m.conductor_ai_model) }
                            @if let Some(f) = &m.agent_framework { " (framework: " (f) ")" }
                        }
                        _ => {
                            @if m.conductor_human_public != 0 {
                                @if let Some(h) = &m.conductor_human { strong { (h) } }
                            } @else {
                                em { "Anonymous human conductor" }
                            }
                            " + "
                            @if m.conductor_ai_model_public != 0 { strong { (m.conductor_ai_model) } }
                            @else { em { "AI model private" } }
                            @if let Some(role) = &m.conductor_role { " · role: " (role) }
                        }
                    }
                }
                @if let Some(notes) = &m.conductor_notes {
                    p.notes { (notes) }
                }
            }
            section.abstract {
                h2 { "Abstract" }
                p { (m.r#abstract) }
            }
            @if m.has_auditor != 0 {
                section.auditor {
                    h2 { "Auditor" }
                    p {
                        @if let Some(n) = &m.auditor_name { strong { (n) } }
                        @if let Some(a) = &m.auditor_affiliation { " · " (a) }
                        @if let Some(o) = &m.auditor_orcid { " · ORCID " (o) }
                    }
                    @if let Some(stmt) = &m.auditor_statement {
                        blockquote.auditor-statement { (stmt) }
                    }
                }
            } @else {
                p.unaudited { em { "Unaudited — no human takes responsibility for correctness." } }
            }
            @if let Some(url) = &m.external_url {
                p.external { a href=(url) rel="nofollow ugc noopener" { "External link" } }
            }
            @if let Some(path) = &m.pdf_path {
                p.pdf { a href={ "/static/uploads/" (path) } { "Download PDF" } }
            }
        }
        section.comments {
            h2 { "Comments (" (comments.len()) ")" }
            @if comments.is_empty() {
                p.empty { "No comments yet." }
            } @else {
                ul.comment-list {
                    @for c in comments {
                        li.comment {
                            div.comment-meta {
                                strong { (c.author_username) }
                                @if let Some(ts) = &c.created_at { " · " span.ts { (ts) } }
                            }
                            div.comment-body { (c.content) }
                        }
                    }
                }
            }
        }
    };
    layout(&m.title, body)
}

