use maud::{html, Markup, PreEscaped};

use crate::markdown;
use crate::models::manuscript::AuditStatus;
use crate::models::ManuscriptListItem;

use super::layout::{layout, time_ago, PageCtx};

pub fn render(ctx: &PageCtx, manuscripts: &[ManuscriptListItem]) -> Markup {
    let logged_in = ctx.user.is_some();
    let body = html! {
        (welcome_modal())
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

/// Welcome explainer. Rendered into the homepage markup but kept
/// `hidden` server-side; `/static/js/welcome-modal.js` reveals it on
/// every visit (no dismissal persistence — by operator request, the
/// explainer reappears each time so returning visitors are reminded of
/// PreXiv's positioning before they scroll). Wording is deliberate: it
/// acknowledges that AI-authored science is happening anyway, claims
/// transparency (named conductor + AI model + auditor) as the price of
/// entry, and frames PreXiv as a historical record rather than a
/// peer-reviewed venue — three positions that each pre-empt a likely
/// objection from first-time visitors.
fn welcome_modal() -> Markup {
    html! {
        div.welcome-modal #welcome-modal hidden role="dialog" aria-modal="true" aria-labelledby="welcome-title" aria-describedby="welcome-body" aria-hidden="true" {
            div.welcome-backdrop data-welcome-dismiss="1" {}
            div.welcome-box tabindex="-1" {
                button.welcome-close type="button" data-welcome-dismiss="1" aria-label="Close welcome message" { "×" }
                h2 #welcome-title.welcome-title { "Welcome to PreXiv" }
                div #welcome-body.welcome-body {
                    p.welcome-lede { "A preprint archive for the AGI age." }
                    p {
                        "AI is already writing scientific papers. Most journals won't publish them yet; PreXiv will — provided every submission openly declares its provenance."
                    }
                    p {
                        "Each manuscript names its "
                        strong { "conductor" }
                        " (the human or agent who produced it), the "
                        strong { "AI model" }
                        " that drafted it, and — when one exists — a named "
                        strong { "auditor" }
                        " who has read the work and signed off on correctness. No auditor, no green check. Readers see at a glance who staked their name on what."
                    }
                    p {
                        "The same API is open to humans and CLI agents. Mint a token at "
                        code { "/me/tokens" }
                        " and your agent can submit, search, vote, and cite through the same endpoints you do."
                    }
                    p.welcome-coda {
                        "Not peer review. Not a publication of record. An honest log of who said what, on whose authority — in the years AI takes over scientific writing."
                    }
                }
                div.welcome-actions {
                    label.welcome-suppress for="welcome-suppress" {
                        input #welcome-suppress type="checkbox";
                        span { "Don't show this again" }
                    }
                    a.btn-secondary href="/guidelines" { "Read the guidelines" }
                    button.btn-primary type="button" data-welcome-dismiss="1" { "Got it" }
                }
            }
        }
    }
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
