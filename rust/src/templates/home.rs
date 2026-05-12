use maud::{html, Markup, PreEscaped};

use crate::markdown;
use crate::models::manuscript::AuditStatus;
use crate::models::ManuscriptListItem;

use super::layout::{layout, time_ago, PageCtx};

pub fn render(ctx: &PageCtx, manuscripts: &[ManuscriptListItem]) -> Markup {
    let logged_in = ctx.user.is_some();
    let body = html! {
        @if manuscripts.is_empty() {
            div.empty {
                p { "No manuscripts here yet." }
                p { a.btn-primary href="/submit" { "Be the first to submit one →" } }
            }
        } @else {
            ol.ms-list {
                @for (i, m) in manuscripts.iter().enumerate() {
                    (manuscript_row(ctx, m, i + 1, logged_in))
                }
            }
        }
    };
    layout("Ranked", ctx, body)
}

fn truncate_name(s: &str) -> String {
    let s = s.trim();
    if s.chars().count() <= 24 { s.to_string() }
    else { let mut t: String = s.chars().take(22).collect(); t.push('…'); t }
}

pub fn manuscript_row(ctx: &PageCtx, m: &ManuscriptListItem, rank: usize, logged_in: bool) -> Markup {
    let id_url = m.arxiv_like_id.as_deref().unwrap_or("");
    let withdrawn = m.is_withdrawn();
    html! {
        li.ms-row.ms-row-withdrawn[withdrawn] id={ "m" (m.id) } {
            div.ms-rank { (rank) "." }
            div.ms-vote {
                @if !withdrawn && logged_in {
                    form.vote-form action="/vote" method="post" {
                        input type="hidden" name="csrf_token" value=(ctx.csrf_token);
                        input type="hidden" name="target_type" value="manuscript";
                        input type="hidden" name="target_id" value=(m.id);
                        input type="hidden" name="value" value="1";
                        button.vote-btn.vote-up type="submit" title="upvote" aria-label="upvote" { "▲" }
                    }
                    div.vote-score data-score=(m.score.unwrap_or(0)) { (m.score.unwrap_or(0)) }
                    form.vote-form action="/vote" method="post" {
                        input type="hidden" name="csrf_token" value=(ctx.csrf_token);
                        input type="hidden" name="target_type" value="manuscript";
                        input type="hidden" name="target_id" value=(m.id);
                        input type="hidden" name="value" value="-1";
                        button.vote-btn.vote-dn type="submit" title="downvote" aria-label="downvote" { "▼" }
                    }
                } @else if !withdrawn {
                    a.vote-btn.vote-up href={ "/login?next=/m/" (id_url) } title="log in to upvote" { "▲" }
                    div.vote-score data-score=(m.score.unwrap_or(0)) { (m.score.unwrap_or(0)) }
                    a.vote-btn.vote-dn href={ "/login?next=/m/" (id_url) } title="log in to downvote" { "▼" }
                } @else {
                    div.vote-score.withdrawn-score title="withdrawn" { "—" }
                }
            }
            div.ms-body {
                div.ms-title-line {
                    a.ms-title href={ "/m/" (id_url) } { (PreEscaped(markdown::render_inline(&m.title))) }
                    " "
                    span.ms-arxivid { "[" (id_url) "]" }
                }
                div.ms-meta {
                    span.ms-authors { (m.authors) }
                    " " span.dot { "·" } " "
                    a.ms-cat href={ "/browse/" (m.category) } { (m.category) }
                    " "
                    @if withdrawn {
                        span.badge.badge-withdrawn title="The submitter (or an admin) withdrew this manuscript" { "⊘ withdrawn" }
                    } @else {
                        @if m.conductor_type == "ai-agent" {
                            span.badge.badge-agent title="Produced autonomously by an AI agent — no human conductor" { "⚙ AI-agent" }
                        }
                        @match m.audit_status() {
                            AuditStatus::ThirdParty => {
                                span.badge.badge-audited title=(format!("Audited by {}", m.auditor_name.as_deref().unwrap_or(""))) {
                                    "✓ audited"
                                    @if let Some(n) = &m.auditor_name { " by " (truncate_name(n)) }
                                }
                            }
                            AuditStatus::SelfAudited => {
                                span.badge.badge-self-audited title=(format!("Self-audit: conductor {} is also the auditor — stronger than unaudited, weaker than a third-party audit", m.auditor_name.as_deref().unwrap_or(""))) {
                                    "◐ self-audited"
                                    @if let Some(n) = &m.auditor_name { " by " (truncate_name(n)) }
                                }
                            }
                            AuditStatus::Unaudited => {
                                span.badge.badge-unaudited title="No auditor — no human takes responsibility for correctness" { "⚠ unaudited" }
                            }
                        }
                    }
                }
                div.ms-sub {
                    span.muted { "submitted " }
                    @if let Some(ts) = &m.created_at {
                        span.muted { (time_ago(ts)) }
                    }
                    " " span.dot { "·" } " "
                    @if m.conductor_type == "ai-agent" {
                        span.muted { "produced by" }
                        " "
                        span.conductor-pair {
                            em {
                                @if m.conductor_ai_model_public != 0 { (m.conductor_ai_model) }
                                @else { "(undisclosed)" }
                            }
                            " "
                            span.muted.small { "(autonomous)" }
                        }
                    } @else {
                        span.muted { "conducted by" }
                        " "
                        span.conductor-pair {
                            strong {
                                @if m.conductor_human_public != 0 {
                                    (m.conductor_human.as_deref().unwrap_or("(undisclosed)"))
                                } @else { "(undisclosed)" }
                            }
                            " + "
                            em {
                                @if m.conductor_ai_model_public != 0 { (m.conductor_ai_model) }
                                @else { "(undisclosed)" }
                            }
                        }
                    }
                    " " span.dot { "·" } " "
                    a href={ "/m/" (id_url) "#comments" } {
                        (m.comment_count.unwrap_or(0))
                        " comment"
                        @if m.comment_count.unwrap_or(0) != 1 { "s" }
                    }
                }
            }
        }
    }
}
