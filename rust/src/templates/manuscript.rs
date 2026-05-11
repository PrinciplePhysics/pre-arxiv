use maud::{html, Markup};

use crate::models::comment::CommentWithAuthor;
use crate::models::Manuscript;

use super::layout::{external_link, layout, PageCtx};

pub fn render(
    ctx: &PageCtx,
    m: &Manuscript,
    comments: &[CommentWithAuthor],
) -> Markup {
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
                @if let Some(doi) = &m.doi { (external_link(&format!("https://doi.org/{doi}"), doi)) " · " }
                span.score { (m.score.unwrap_or(0)) " points · " (m.comment_count.unwrap_or(0)) " comments" }
            }
            section.conductor {
                h2 { "Conductor" }
                p {
                    @if m.conductor_type == "ai-agent" {
                        "Autonomous AI agent"
                        @if m.conductor_ai_model_public != 0 { ": " (m.conductor_ai_model) }
                        @if let Some(f) = &m.agent_framework { " (framework: " (f) ")" }
                    } @else {
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
                p.external { (external_link(url, "External link")) }
            }
            @if let Some(path) = &m.pdf_path {
                p.pdf { a href={ "/static/uploads/" (path) } { "Download PDF" } }
            }
            @let logged_in = ctx.user.is_some();
            @if !m.is_withdrawn() && logged_in {
                form.vote-form action="/vote" method="post" {
                    input type="hidden" name="csrf_token" value=(ctx.csrf_token);
                    input type="hidden" name="target_type" value="manuscript";
                    input type="hidden" name="target_id" value=(m.id);
                    button.vote-up name="value" value="1" type="submit" { "▲ Upvote" }
                    button.vote-down name="value" value="-1" type="submit" { "▼ Downvote" }
                }
            }
        }
        section.comments {
            h2 { "Comments (" (comments.len()) ")" }
            @if logged_in {
                form.comment-form action={"/m/" (m.arxiv_like_id.as_deref().unwrap_or(&m.id.to_string())) "/comment"} method="post" {
                    input type="hidden" name="csrf_token" value=(ctx.csrf_token);
                    textarea name="content" required rows="4" placeholder="Add a comment…" {}
                    button type="submit" { "Post comment" }
                }
            } @else {
                p.minor { a href="/login" { "Sign in to comment." } }
            }
            @if comments.is_empty() {
                p.empty { "No comments yet." }
            } @else {
                ul.comment-list {
                    @for c in comments {
                        li.comment id={"comment-" (c.id)} {
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
    layout(&m.title, ctx, body)
}
