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
    submitter: Option<&(String, Option<String>, i64, i64, i64)>,
    cats: &[(String, i64)],
    my_vote: i64,
) -> Markup {
    let logged_in = ctx.user.is_some();
    let slug = m.arxiv_like_id.as_deref().unwrap_or("");
    let submitter_verified_scholar = submitter
        .map(|(_, _, ev, ie, oo)| *oo != 0 || (*ev != 0 && *ie != 0))
        .unwrap_or(false);
    let cat_restricted = crate::categories::is_restricted(&m.category);

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
                p.muted.small {
                    "Subjects: "
                    a.ms-cat-pill href={ "/browse/" (m.category) } { (m.category) }
                    @if let Some(secs) = m.secondary_categories.as_deref().filter(|s| !s.trim().is_empty()) {
                        @for s in secs.split_whitespace() {
                            " · "
                            a href={ "/browse/" (s) } { (s) }
                        }
                    }
                }
                p.muted.small.mono {
                    @if let Some(doi) = &m.doi { "doi: " (doi) }
                    @if m.current_version > 1 {
                        " \u{00b7} version: v" (m.current_version)
                        @if let Some(ts) = &m.updated_at {
                            " (revised " (time_ago(ts)) ")"
                        }
                    } @else if m.current_version == 1 {
                        " \u{00b7} version: v1"
                    }
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
                            @let ai_list = m.ai_models();
                            div.agent-banner {
                                strong { "AI agent (autonomous)." }
                                " This manuscript was produced by "
                                @if m.conductor_ai_model_public != 0 {
                                    @if ai_list.len() <= 1 {
                                        (m.conductor_ai_model)
                                    } @else {
                                        "multiple AI models — "
                                        @for (i, name) in ai_list.iter().enumerate() {
                                            @if i > 0 { ", " }
                                            (name)
                                        }
                                    }
                                } @else { "(undisclosed)" }
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

                        // Soft FYI banners: restricted category + unverified author.
                        // Slate / blue palette so they read as advisory, not warning —
                        // distinct from the amber "Unaudited" banner above which signals
                        // potential correctness risk.
                        @if cat_restricted {
                            div.advisory-banner role="note" {
                                span {
                                    span.advisory-title { "Restricted category." }
                                    " "
                                    code { (m.category) }
                                    " is one of a handful of \"general\" buckets that historically attract speculative work. PreXiv keeps it reachable via "
                                    code { "/browse" }
                                    " and direct link, but does "
                                    em { "not" }
                                    " surface its contents on the default ranked listings (/, /new, /top, /audited)."
                                }
                            }
                        }
                        @if !submitter_verified_scholar && !m.is_withdrawn() {
                            div.advisory-banner role="note" {
                                span {
                                    span.advisory-title { "Unverified author." }
                                    " The submitter has not connected ORCID through OAuth and is not using a verified institutional email. Default listings only surface verified-scholar work. This submission is reachable via search, "
                                    code { "/browse" }
                                    ", and direct link."
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
                                @let ai_list_a = m.ai_models();
                                tr {
                                    th { (if ai_list_a.len() > 1 { "AI agents" } else { "AI agent" }) }
                                    td {
                                        @if m.conductor_ai_model_public != 0 {
                                            span.ai-model-pills {
                                                @for name in &ai_list_a {
                                                    span.ai-model-pill { (name) }
                                                }
                                            }
                                        } @else {
                                            strong { "(undisclosed)" }
                                        }
                                    }
                                }
                                @if let Some(f) = &m.agent_framework { tr { th { "Framework" } td { (f) } } }
                                @if let Some(notes) = &m.conductor_notes { tr { th { "Notes" } td.markdown { (md(notes)) } } }
                            }
                        } @else {
                            @let ai_list_h = m.ai_models();
                            table.kv {
                                tr { th { "Mode" } td { span.role-tag { "Human + AI co-author" @if ai_list_h.len() > 1 { "s" } } } }
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
                                tr {
                                    th { (if ai_list_h.len() > 1 { "AI co-authors" } else { "AI co-author" }) }
                                    td {
                                        @if m.conductor_ai_model_public != 0 {
                                            span.ai-model-pills {
                                                @for name in &ai_list_h {
                                                    span.ai-model-pill { (name) }
                                                }
                                            }
                                        } @else {
                                            em { "(undisclosed)" }
                                        }
                                    }
                                }
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
                                @let viewer_owns_comment = ctx.user.as_ref().map(|u| u.id == c.author_id).unwrap_or(false);
                                @let viewer_is_admin = ctx.user.as_ref().map(|u| u.is_admin()).unwrap_or(false);
                                @let viewer_can_delete = viewer_owns_comment || viewer_is_admin;
                                @let viewer_can_flag = ctx.user.is_some() && !viewer_owns_comment;
                                li.comment id={"comment-" (c.id)} {
                                    div.comment-meta {
                                        strong { (c.author_username) }
                                        @if let Some(ts) = &c.created_at {
                                            " · " span.muted { (time_ago(ts)) }
                                        }
                                        @if viewer_can_delete || viewer_can_flag {
                                            span.comment-actions {
                                                @if viewer_can_delete {
                                                    form.inline action={"/c/" (c.id) "/delete"} method="post"
                                                        onsubmit="return confirm('Delete this comment? Replies under it will also be removed.');" {
                                                        input type="hidden" name="csrf_token" value=(ctx.csrf_token);
                                                        button.linklike.danger type="submit"
                                                            title="Delete this comment" { "delete" }
                                                    }
                                                }
                                                @if viewer_can_flag {
                                                    @if viewer_can_delete { " · " }
                                                    form.inline action={"/c/" (c.id) "/flag"} method="post"
                                                        onsubmit="return confirm('Flag this comment for moderator review?');" {
                                                        input type="hidden" name="csrf_token" value=(ctx.csrf_token);
                                                        button.linklike type="submit"
                                                            title="Flag this comment for moderator review" { "flag" }
                                                    }
                                                }
                                            }
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
                    @if let Some(src) = &m.source_path {
                        a.bx-sidebar-btn.secondary href={ "/static/uploads/" (src) } target="_blank" rel="noopener" title="Compiled from this LaTeX source" {
                            "↓ LaTeX source"
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

                    // Flag-for-moderation. Hidden when the viewer is the
                    // submitter (one can't flag one's own manuscript) and
                    // when the manuscript is already withdrawn (no point).
                    @let viewer_is_submitter_for_flag = ctx.user.as_ref().map(|u| u.id == m.submitter_id).unwrap_or(false);
                    @if logged_in && !m.is_withdrawn() && !viewer_is_submitter_for_flag {
                        details.flag-disclosure style="margin-top:10px" {
                            summary.linklike.small.muted { "🚩 Flag for moderator review" }
                            form action={"/m/" (slug) "/flag"} method="post" class="flag-form" {
                                input type="hidden" name="csrf_token" value=(ctx.csrf_token);
                                label.small.muted style="display:block;margin:6px 0 4px" { "Optional reason" }
                                textarea name="reason" maxlength="500" rows="2"
                                         placeholder="What looks wrong? Spam, plagiarism, manifest hate, etc." {}
                                button.btn-secondary.btn-small type="submit" style="margin-top:6px" { "Submit report" }
                            }
                        }
                    }
                }

                div.bx-sidebar-block {
                    h3 { "Statistics" }
                    ul.bx-stats {
                        li { span.lbl { "Score" }    span.val { (m.score.unwrap_or(0)) } }
                        li { span.lbl { "Views" }    span.val { (m.view_count.unwrap_or(0)) } }
                        li { span.lbl { "Comments" } span.val { (m.comment_count.unwrap_or(0)) } }
                        li { span.lbl { "Version" }  span.val { "v" (m.current_version) } }
                    }
                }

                // Versions block: list link + revise CTA when allowed.
                @let viewer_is_submitter = ctx.user.as_ref().map(|u| u.id == m.submitter_id).unwrap_or(false);
                @let viewer_is_admin     = ctx.user.as_ref().map(|u| u.is_admin()).unwrap_or(false);
                @let can_revise = (viewer_is_submitter || viewer_is_admin) && !m.is_withdrawn();
                div.bx-sidebar-block {
                    h3 { "Versions" }
                    @if m.current_version > 1 {
                        a.bx-sidebar-btn.secondary href={ "/m/" (slug) "/versions" } {
                            "View all " (m.current_version) " versions"
                        }
                    } @else {
                        p.muted.small style="margin:0 0 6px" { "Only v1 has been published." }
                        a.bx-sidebar-btn.secondary href={ "/m/" (slug) "/versions" } { "Version history" }
                    }
                    @if can_revise {
                        a.bx-sidebar-btn href={ "/m/" (slug) "/revise" } style="margin-top:6px" {
                            "\u{2728} Publish a new revision"
                        }
                    }
                }

                div.bx-sidebar-block {
                    h3 { "Subject area" }
                    a.ms-cat-pill href={ "/browse/" (m.category) } { (m.category) }
                    @if let Some((un, dn, ev, ie, oo)) = submitter {
                        p.muted.small style="margin:12px 0 0" {
                            "Submitted by "
                            a href={ "/u/" (un) } { (dn.as_deref().unwrap_or(un.as_str())) }
                            @if *oo != 0 || (*ev != 0 && *ie != 0) {
                                " "
                                span.profile-vbadge title="Verified scholar"
                                    style="font-size:10.5px;padding:2px 7px" { "✓ verified" }
                            }
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

                @let can_withdraw = match &ctx.user {
                    Some(u) => !m.is_withdrawn() && (u.id == m.submitter_id || u.is_admin()),
                    None => false,
                };
                @if can_withdraw {
                    div.bx-sidebar-block {
                        h3 { "Submitter actions" }
                        p.muted.small style="margin:0 0 8px" {
                            "You are the submitter of this manuscript. "
                            @if let Some(u) = &ctx.user { @if u.is_admin() && u.id != m.submitter_id { "(Admin override.) " } }
                            "Withdrawing replaces this page with a tombstone — id, DOI, title, conductor metadata, and the reason stay so citations don't break, but the body, PDF link, and search index drop."
                        }
                        form action={"/m/" (slug) "/withdraw"} method="post" {
                            input type="hidden" name="csrf_token" value=(ctx.csrf_token);
                            textarea name="reason" rows="3" maxlength="500"
                                     style="width:100%;font-size:0.9em"
                                     placeholder="Reason (optional but encouraged — shown on the tombstone). e.g. 'duplicate of prexiv:…', 'Lemma 2 has a fatal hole', 'replaced by journal version'." {}
                            button.btn-secondary.danger type="submit"
                                   style="margin-top:6px;width:100%"
                                   onclick="return confirm('Withdraw this manuscript? The page will be replaced with a tombstone immediately. This action is not reversible from the UI.');"
                                { "▾ Withdraw manuscript" }
                        }
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
fn _ext(u: &str) -> Markup {
    external_link(u, u)
}
