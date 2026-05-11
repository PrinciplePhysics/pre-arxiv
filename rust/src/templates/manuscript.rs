use maud::{html, Markup};

use crate::models::comment::CommentWithAuthor;
use crate::models::Manuscript;

use super::layout::{external_link, layout, time_ago, PageCtx};

pub fn render(
    ctx: &PageCtx,
    m: &Manuscript,
    comments: &[CommentWithAuthor],
) -> Markup {
    let logged_in = ctx.user.is_some();
    let body = html! {
        article.manuscript {

            @if m.is_withdrawn() {
                div.tombstone-banner {
                    strong { "Withdrawn." }
                    " This manuscript was withdrawn."
                    @if let Some(r) = &m.withdrawn_reason {
                        " " span.tombstone-reason { "Reason: " (r) }
                    }
                    " The contents below are kept for citation continuity."
                }
            } @else {
                @if m.conductor_type == "ai-agent" {
                    div.agent-banner {
                        strong { "AI agent (autonomous)." }
                        " This manuscript was produced by "
                        @if m.conductor_ai_model_public != 0 { (m.conductor_ai_model) } @else { "(undisclosed)" }
                        " acting on its own, without ongoing human direction."
                        @if m.has_auditor == 0 {
                            " " strong { "No human" } " — including the submitter — takes responsibility for its conduct or contents."
                        }
                    }
                }
                @if m.has_auditor == 0 && m.conductor_type != "ai-agent" {
                    div.warn-banner {
                        strong { "Unaudited manuscript." }
                        " The submitter has explicitly stated that they are "
                        em { "not" }
                        " responsible for the correctness of this work. Treat the result with appropriate skepticism."
                    }
                } @else if m.has_auditor != 0 {
                    div.audit-banner {
                        strong { "Audited." }
                        " "
                        @if let Some(n) = &m.auditor_name { (n) }
                        @if let Some(a) = &m.auditor_affiliation { " (" (a) ")" }
                        " has read the manuscript and provided a signed correctness statement (see below)."
                    }
                }
            }

            header.ms-header {
                div.ms-id-row {
                    @if let Some(id) = &m.arxiv_like_id {
                        span.ms-arxivid-big { (id) }
                    }
                    @if let Some(doi) = &m.doi {
                        " "
                        span.ms-doi.muted.mono { (doi) }
                    }
                    " "
                    a.ms-cat-pill href={ "/browse/" (m.category) } { (m.category) }
                    " "
                    span.muted { "·" }
                    " "
                    @if let Some(ts) = &m.created_at {
                        span.muted { "submitted " (time_ago(ts)) }
                    }
                    " "
                    span.muted { "·" }
                    " "
                    span.muted { (m.view_count.unwrap_or(0)) " views" }
                }
                h1.ms-h1 { (m.title) }
                div.ms-authors-line { (m.authors) }
            }

            div.ms-actions-bar {
                div.ms-actions-left {
                    @if let Some(path) = &m.pdf_path {
                        a.btn-primary href={ "/static/uploads/" (path) } target="_blank" rel="noopener" { "Download PDF" }
                    }
                    @if let Some(url) = &m.external_url {
                        (external_link_btn(url, "External link ↗"))
                    }
                    a.btn-secondary href={ "/m/" (m.arxiv_like_id.as_deref().unwrap_or("")) "/cite" } { "Cite" }
                }
                div.ms-actions-right {
                    @if !m.is_withdrawn() && logged_in {
                        form.inline-vote-form.vote-form action="/vote" method="post" {
                            input type="hidden" name="csrf_token" value=(ctx.csrf_token);
                            input type="hidden" name="target_type" value="manuscript";
                            input type="hidden" name="target_id" value=(m.id);
                            input type="hidden" name="value" value="1";
                            button.vote-pill.vote-up type="submit" { "▲ upvote" }
                        }
                        form.inline-vote-form.vote-form action="/vote" method="post" {
                            input type="hidden" name="csrf_token" value=(ctx.csrf_token);
                            input type="hidden" name="target_type" value="manuscript";
                            input type="hidden" name="target_id" value=(m.id);
                            input type="hidden" name="value" value="-1";
                            button.vote-pill.vote-dn type="submit" { "▼ downvote" }
                        }
                    }
                    span.score-pill title="net score" { (m.score.unwrap_or(0)) " pts" }
                }
            }

            section.ms-section {
                h2.ms-section-h { "Abstract" }
                p.ms-abstract { (m.r#abstract) }
            }

            section.ms-section.ms-conductor {
                h2.ms-section-h { "Conductor" }
                @if m.conductor_type == "ai-agent" {
                    p.muted.small { "No human conductor. This manuscript was produced by an AI agent acting autonomously." }
                    table.kv {
                        tr { th { "Mode" } td { span.role-tag.agent-tag { "AI agent (autonomous)" } } }
                        tr {
                            th { "AI agent" }
                            td {
                                strong {
                                    @if m.conductor_ai_model_public != 0 { (m.conductor_ai_model) }
                                    @else { "(undisclosed)" }
                                }
                            }
                        }
                        @if let Some(f) = &m.agent_framework {
                            tr { th { "Framework" } td { (f) } }
                        }
                        @if let Some(notes) = &m.conductor_notes {
                            tr { th { "Notes" } td { (notes) } }
                        }
                    }
                } @else {
                    table.kv {
                        tr { th { "Mode" } td { span.role-tag { "Human + AI co-author" } } }
                        tr {
                            th { "Conductor (human)" }
                            td {
                                strong {
                                    @if m.conductor_human_public != 0 {
                                        (m.conductor_human.as_deref().unwrap_or("(undisclosed)"))
                                    } @else { "(undisclosed)" }
                                }
                                @if let Some(role) = &m.conductor_role {
                                    " · " span.muted { (role) }
                                }
                            }
                        }
                        tr {
                            th { "AI co-author" }
                            td {
                                em {
                                    @if m.conductor_ai_model_public != 0 { (m.conductor_ai_model) }
                                    @else { "(undisclosed)" }
                                }
                            }
                        }
                        @if let Some(notes) = &m.conductor_notes {
                            tr { th { "Notes" } td { (notes) } }
                        }
                    }
                }
            }

            @if m.has_auditor != 0 {
                section.ms-section.ms-auditor {
                    h2.ms-section-h { "Auditor" }
                    table.kv {
                        @if let Some(n) = &m.auditor_name { tr { th { "Name" } td { strong { (n) } } } }
                        @if let Some(a) = &m.auditor_affiliation { tr { th { "Affiliation" } td { (a) } } }
                        @if let Some(r) = &m.auditor_role { tr { th { "Role" } td { (r) } } }
                        @if let Some(o) = &m.auditor_orcid { tr { th { "ORCID" } td { (o) } } }
                    }
                    @if let Some(stmt) = &m.auditor_statement {
                        blockquote.auditor-statement { (stmt) }
                    }
                }
            }
        }

        section.comments id="comments" {
            h2 { "Comments (" (comments.len()) ")" }
            @if logged_in {
                form.comment-form action={"/m/" (m.arxiv_like_id.as_deref().unwrap_or(&m.id.to_string())) "/comment"} method="post" {
                    input type="hidden" name="csrf_token" value=(ctx.csrf_token);
                    textarea name="content" required rows="4" placeholder="Add a comment…" {}
                    div.comment-form-actions {
                        button.btn-primary type="submit" { "Post comment" }
                    }
                }
            } @else {
                p.login-cta {
                    a href="/login" { "Sign in" }
                    " to comment."
                }
            }
            @if comments.is_empty() {
                p.muted { "No comments yet." }
            } @else {
                ul.comment-list {
                    @for c in comments {
                        li.comment id={"comment-" (c.id)} {
                            div.comment-meta {
                                strong { (c.author_username) }
                                @if let Some(ts) = &c.created_at {
                                    " · " span.muted { (time_ago(ts)) }
                                }
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

fn external_link_btn(url: &str, label: &str) -> Markup {
    html! {
        a.btn-secondary href=(url) rel="nofollow ugc noopener" target="_blank" { (label) }
    }
}

#[allow(dead_code)]
fn _ext_link_unused() -> Markup { external_link("", "") }
