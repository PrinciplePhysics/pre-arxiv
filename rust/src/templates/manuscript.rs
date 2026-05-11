use maud::{html, Markup, PreEscaped};

use crate::licenses;
use crate::markdown;
use crate::models::comment::CommentWithAuthor;
use crate::models::Manuscript;

use super::layout::{external_link, layout, time_ago, PageCtx};

fn md(s: &str) -> PreEscaped<String> {
    PreEscaped(markdown::render(s))
}

fn md_inline(s: &str) -> PreEscaped<String> {
    PreEscaped(markdown::render_inline(s))
}

pub fn render(
    ctx: &PageCtx,
    m: &Manuscript,
    comments: &[CommentWithAuthor],
    submitter: Option<&(String, Option<String>)>,
    cats: &[(String, i64)],
    my_vote: i64,
) -> Markup {
    let logged_in = ctx.user.is_some();
    let slug = m.arxiv_like_id.as_deref().unwrap_or("");

    let body = html! {
        div.bx-grid {

            // ─── main column ─────────────────────────────────────────────
            div.bx-main {
                @if m.is_withdrawn() {
                    span.bx-eyebrow.withdrawn { "withdrawn" }
                } @else if m.conductor_type == "ai-agent" {
                    span.bx-eyebrow.agent { "AI-agent (autonomous)" }
                } @else {
                    span.bx-eyebrow { "New submission" }
                }
                h1.ms-h1 { (md_inline(&m.title)) }
                p.ms-authors-line { (m.authors) }
                @if let Some(doi) = &m.doi {
                    p.muted.small.mono { "doi: " (doi) }
                }

                nav.bx-tabs aria-label="manuscript sections" {
                    a href="#abstract" { "Abstract" }
                    a href="#conductor" { "Conductor" }
                    @if m.has_auditor != 0 { a href="#auditor" { "Auditor" } }
                    a href="#comments" { "Comments (" (comments.len()) ")" }
                    a href={ "/m/" (slug) "/cite" } { "Cite" }
                }

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
                                " responsible for the correctness of this work."
                            }
                        } @else if m.has_auditor != 0 {
                            @let self_audited = match (&m.auditor_name, &m.conductor_human) {
                                (Some(an), Some(ch)) => an.trim() == ch.trim() && !ch.trim().is_empty(),
                                _ => false,
                            };
                            div.audit-banner {
                                @if self_audited {
                                    strong { "Self-audited." }
                                    " "
                                    @if let Some(n) = &m.auditor_name { (n) }
                                    " is both the conductor and the auditor: they directed the AI and have read the manuscript line by line, signing a correctness statement (see below). This is a stronger claim than conducting alone, but weaker than a third-party audit."
                                } @else {
                                    strong { "Audited." }
                                    " "
                                    @if let Some(n) = &m.auditor_name { (n) }
                                    @if let Some(a) = &m.auditor_affiliation { " (" (a) ")" }
                                    " has read the manuscript and provided a signed correctness statement (see below)."
                                }
                            }
                        }
                    }

                    section.ms-section id="abstract" {
                        h2.ms-section-h { "Abstract" }
                        div.ms-abstract.markdown { (md(&m.r#abstract)) }
                    }

                    section.ms-section.ms-conductor id="conductor" {
                        h2.ms-section-h { "Conductor" }
                        @if m.conductor_type == "ai-agent" {
                            p.muted.small { "No human conductor. Produced by an AI agent acting autonomously." }
                            table.kv {
                                tr { th { "Mode" } td { span.role-tag.agent-tag { "AI agent (autonomous)" } } }
                                tr { th { "AI agent" } td {
                                    strong {
                                        @if m.conductor_ai_model_public != 0 { (m.conductor_ai_model) }
                                        @else { "(undisclosed)" }
                                    }
                                } }
                                @if let Some(f) = &m.agent_framework { tr { th { "Framework" } td { (f) } } }
                                @if let Some(notes) = &m.conductor_notes { tr { th { "Notes" } td.markdown { (md(notes)) } } }
                            }
                        } @else {
                            table.kv {
                                tr { th { "Mode" } td { span.role-tag { "Human + AI co-author" } } }
                                tr { th { "Conductor (human)" } td {
                                    strong {
                                        @if m.conductor_human_public != 0 {
                                            (m.conductor_human.as_deref().unwrap_or("(undisclosed)"))
                                        } @else { "(undisclosed)" }
                                    }
                                    @if let Some(role) = &m.conductor_role {
                                        " · " span.muted { (role) }
                                    }
                                } }
                                tr { th { "AI co-author" } td {
                                    em {
                                        @if m.conductor_ai_model_public != 0 { (m.conductor_ai_model) }
                                        @else { "(undisclosed)" }
                                    }
                                } }
                                @if let Some(notes) = &m.conductor_notes { tr { th { "Notes" } td.markdown { (md(notes)) } } }
                            }
                        }
                    }

                    @if m.has_auditor != 0 {
                        @let self_audited = match (&m.auditor_name, &m.conductor_human) {
                            (Some(an), Some(ch)) => an.trim() == ch.trim() && !ch.trim().is_empty(),
                            _ => false,
                        };
                        section.ms-section.ms-auditor id="auditor" {
                            h2.ms-section-h {
                                @if self_audited { "Self-audit" } @else { "Auditor" }
                            }
                            table.kv {
                                @if let Some(n) = &m.auditor_name { tr { th { "Name" } td { strong { (n) } } } }
                                @if let Some(a) = &m.auditor_affiliation { tr { th { "Affiliation" } td { (a) } } }
                                @if let Some(r) = &m.auditor_role { tr { th { "Role" } td { (r) } } }
                                @if let Some(o) = &m.auditor_orcid { tr { th { "ORCID" } td { (o) } } }
                            }
                            @if let Some(stmt) = &m.auditor_statement {
                                blockquote.auditor-statement.markdown { (md(stmt)) }
                            }
                        }
                    }
                }

                section.comments id="comments" {
                    h2 { "Comments (" (comments.len()) ")" }
                    @if logged_in {
                        form.comment-form action={"/m/" (slug) "/comment"} method="post" {
                            input type="hidden" name="csrf_token" value=(ctx.csrf_token);
                            textarea name="content" required rows="4" placeholder="Add a comment…  Markdown supported (**bold**, `code`, lists, links, etc.). LaTeX math via $E=mc^2$ or $$\\int…$$" {}
                            div.comment-form-actions {
                                button.btn-primary type="submit" { "Post comment" }
                                span.hint style="margin-left:8px" { "Markdown + LaTeX math supported." }
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
                                    div.comment-body.markdown { (md(&c.content)) }
                                }
                            }
                        }
                    }
                }
            }

            // ─── right sidebar ──────────────────────────────────────────
            aside.bx-sidebar aria-label="manuscript actions and metadata" {
                div.bx-sidebar-block {
                    @if let Some(ts) = &m.created_at {
                        h3 { "Posted" }
                        p style="margin:0" { (ts.format("%B %-d, %Y")) }
                        p.muted.small style="margin:4px 0 10px" { (time_ago(ts)) }
                    }
                    @if let Some(path) = &m.pdf_path {
                        a.bx-sidebar-btn href={ "/static/uploads/" (path) } target="_blank" rel="noopener" {
                            "↓ Download PDF"
                        }
                    }
                    @if let Some(url) = &m.external_url {
                        (sidebar_external(url))
                    }
                    a.bx-sidebar-btn.secondary href={ "/m/" (slug) "/cite" } { "Citation Tools" }
                    @if !m.is_withdrawn() && logged_in {
                        form action="/vote" method="post" style="margin-top:8px;display:flex;gap:4px" {
                            input type="hidden" name="csrf_token" value=(ctx.csrf_token);
                            input type="hidden" name="target_type" value="manuscript";
                            input type="hidden" name="target_id" value=(m.id);
                            button.bx-sidebar-btn.secondary.voted[my_vote == 1] style="flex:1;margin:0" name="value" value="1" type="submit"
                                title=(if my_vote == 1 { "You upvoted. Click again to remove." } else { "Upvote" }) {
                                @if my_vote == 1 { "▲ Upvoted ✓" } @else { "▲ Upvote" }
                            }
                            button.bx-sidebar-btn.secondary.voted[my_vote == -1] style="flex:1;margin:0" name="value" value="-1" type="submit"
                                title=(if my_vote == -1 { "You downvoted. Click again to remove." } else { "Downvote" }) {
                                @if my_vote == -1 { "▼ Downvoted ✓" } @else { "▼ Downvote" }
                            }
                        }
                        @if my_vote != 0 {
                            p.muted.small style="margin:6px 0 0;text-align:center" {
                                "You voted "
                                @if my_vote == 1 { strong { "▲ up" } } @else { strong { "▼ down" } }
                                ". Click the same button again to remove your vote."
                            }
                        }
                    } @else if !m.is_withdrawn() {
                        a.bx-sidebar-btn.secondary href={ "/login?next=/m/" (slug) } style="margin-top:8px" { "Sign in to vote" }
                    }
                }

                div.bx-sidebar-block {
                    h3 { "Statistics" }
                    ul.bx-stats {
                        li { span.lbl { "Score" }    span.val { (m.score.unwrap_or(0)) } }
                        li { span.lbl { "Views" }    span.val { (m.view_count.unwrap_or(0)) } }
                        li { span.lbl { "Comments" } span.val { (m.comment_count.unwrap_or(0)) } }
                    }
                }

                div.bx-sidebar-block {
                    h3 { "Subject area" }
                    a.ms-cat-pill href={ "/browse/" (m.category) } { (m.category) }
                    @if let Some((un, dn)) = submitter {
                        p.muted.small style="margin:12px 0 0" {
                            "Submitted by "
                            a href={ "/u/" (un) } { (dn.as_deref().unwrap_or(un.as_str())) }
                        }
                    }
                }

                @let license_id = m.license.as_deref().unwrap_or("CC-BY-4.0");
                @let lic = licenses::lookup(license_id);
                @let ai_id = m.ai_training.as_deref().unwrap_or("allow");
                @let ai = licenses::ai_training_lookup(ai_id);
                div.bx-sidebar-block {
                    h3 { "License" }
                    @if let Some(l) = lic {
                        a href=(l.url) target="_blank" rel="noopener" style="font-weight:600" { (l.short) }
                        p.muted.small style="margin:6px 0 0" { (l.summary) }
                    } @else {
                        span.muted { (license_id) }
                    }
                    hr style="margin:10px 0;border:none;border-top:1px solid var(--rule)";
                    h3 { "AI training" }
                    @if let Some(o) = ai {
                        strong { (o.short) }
                        p.muted.small style="margin:6px 0 0" {
                            @if ai_id == "disallow" {
                                "Submitter requests this manuscript NOT be used as training data."
                            } @else if ai_id == "allow-with-attribution" {
                                "Training permitted; submitter requests attribution in trained-model output."
                            } @else {
                                "Training permitted under the reader license above."
                            }
                        }
                    }
                    p.muted.small style="margin:8px 0 0" {
                        a href="/licenses" { "What do these mean?" }
                    }
                }

                @if !cats.is_empty() {
                    div.bx-sidebar-block {
                        h3 { "Subject areas" }
                        ul.bx-cat-list {
                            @for (cat, n) in cats {
                                li.on[*cat == m.category] {
                                    a href={ "/browse/" (cat) } { (cat) }
                                    span.n { "(" (n) ")" }
                                }
                            }
                        }
                    }
                }
            }
        }
    };
    layout(&m.title, ctx, body)
}

fn sidebar_external(url: &str) -> Markup {
    html! {
        a.bx-sidebar-btn href=(url) rel="nofollow ugc noopener" target="_blank" { "External link ↗" }
    }
}

#[allow(dead_code)]
fn _ext(u: &str) -> Markup { external_link(u, u) }
