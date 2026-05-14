use maud::{html, Markup};

use super::layout::{layout, time_ago, PageCtx};
use crate::routes::admin::{AdminDashboard, AuditRow, FlagRow};

fn fmt_int(n: i64) -> String {
    let s = n.abs().to_string();
    let mut out = String::with_capacity(s.len() + s.len() / 3 + 1);
    for (i, ch) in s.chars().rev().enumerate() {
        if i > 0 && i % 3 == 0 {
            out.push(',');
        }
        out.push(ch);
    }
    let mut out: String = out.chars().rev().collect();
    if n < 0 {
        out.insert(0, '-');
    }
    out
}

fn percent(part: i64, whole: i64) -> String {
    if whole <= 0 {
        "0%".to_string()
    } else {
        format!("{:.0}%", (part as f64 / whole as f64) * 100.0)
    }
}

fn stat_card(label: &str, value: i64, detail: Markup) -> Markup {
    html! {
        div.admin-stat-card {
            span.admin-stat-label { (label) }
            strong.admin-stat-value { (fmt_int(value)) }
            div.admin-stat-detail { (detail) }
        }
    }
}

fn status_pill(label: &str, class_name: &str) -> Markup {
    html! { span class=(format!("admin-pill {class_name}")) { (label) } }
}

fn gap_card(title: &str, body: &str) -> Markup {
    html! {
        div.admin-empty {
            strong { (title) }
            span { (body) }
        }
    }
}

pub fn render_queue(ctx: &PageCtx, dashboard: &AdminDashboard, flags: &[FlagRow]) -> Markup {
    let s = &dashboard.stats;
    let body = html! {
        div.page-header {
            h1 { "Admin dashboard" }
            p.muted {
                "Operational snapshot for submissions, users, provenance, tokens, and moderation. "
                a href="/admin/audit" { "View audit log →" }
            }
        }

        section.admin-stat-grid aria-label="Site statistics" {
            (stat_card("Manuscripts", s.total_manuscripts, html! {
                (fmt_int(s.live_manuscripts)) " live · "
                (fmt_int(s.manuscripts_7d)) " new in 7d"
            }))
            (stat_card("Open flags", s.open_flags, html! {
                (fmt_int(s.flags_24h)) " opened in 24h"
                @if let Some(ts) = &s.oldest_open_flag_at {
                    " · oldest " (time_ago(ts))
                }
            }))
            (stat_card("Moderation SLA", s.open_flags_over_24h, html! {
                "open longer than 24h · "
                (fmt_int(s.resolved_flags_7d)) " resolved in 7d"
            }))
            (stat_card("User growth", s.total_users, html! {
                (fmt_int(s.new_users_24h)) " new in 24h · "
                (fmt_int(s.new_users_7d)) " new in 7d"
            }))
            (stat_card("Verified users", s.email_verified_users, html! {
                (percent(s.email_verified_users, s.total_users)) " of "
                (fmt_int(s.total_users)) " accounts"
            }))
            (stat_card("Verified scholars", s.verified_scholar_users, html! {
                (fmt_int(s.orcid_oauth_users)) " ORCID · "
                (fmt_int(s.institutional_verified_users)) " institutional"
            }))
            (stat_card("API tokens", s.active_tokens, html! {
                (fmt_int(s.tokens_used_7d)) " used in 7d"
            }))
            (stat_card("Discussion", s.total_comments, html! {
                (fmt_int(s.comments_24h)) " comments in 24h · "
                (fmt_int(s.comments_7d)) " in 7d · "
                (fmt_int(s.votes_7d)) " votes in 7d · "
                (fmt_int(s.total_votes)) " total votes"
            }))
        }

        div.admin-panel-grid {
            section.admin-panel.admin-panel-wide {
                div.admin-panel-head {
                    div {
                        h2 { "Abuse and moderation trend" }
                        p.muted { "Reports opened and resolved over the last seven calendar days." }
                    }
                    span.admin-mini-stat { (fmt_int(s.open_flags)) " open now" }
                }
                @if dashboard.moderation_trend.is_empty() {
                    div.admin-empty { strong { "No moderation events yet" } }
                } @else {
                    table.admin-table.admin-table-compact {
                        thead { tr { th { "Day" } th { "Opened" } th { "Resolved" } } }
                        tbody {
                            @for row in &dashboard.moderation_trend {
                                tr {
                                    td.mono { (row.day) }
                                    td { (fmt_int(row.primary_count)) }
                                    td { (fmt_int(row.secondary_count)) }
                                }
                            }
                        }
                    }
                }
            }

            section.admin-panel {
                div.admin-panel-head {
                    div {
                        h2 { "Auth and rate limits" }
                        p.muted { "Visibility for blocked or failed access attempts." }
                    }
                }
                (gap_card(
                    "Not yet instrumented",
                    "Failed login attempts and rate-limit rejections are enforced in middleware but are not persisted to a queryable table."
                ))
            }

            section.admin-panel {
                div.admin-panel-head {
                    div {
                        h2 { "Legal request queue" }
                        p.muted { "Operational tracking for takedown or records requests." }
                    }
                }
                (gap_card(
                    "No persisted queue",
                    "The database schema does not currently include legal request records, deadlines, custodians, or status transitions."
                ))
            }

            section.admin-panel.admin-panel-wide {
                div.admin-panel-head {
                    div {
                        h2 { "Moderation queue" }
                        p.muted { "Open reports from users. Resolve when handled; the report remains in the audit trail." }
                    }
                    a.btn-secondary href="/admin/audit" { "Audit log" }
                }
                @if flags.is_empty() {
                    div.admin-empty {
                        strong { "No open flags" }
                        span { "The public moderation queue is clear." }
                    }
                } @else {
                    ul.flag-list {
                        @for f in flags {
                            li.flag-row {
                                div.flag-head {
                                    span.flag-type-pill { (f.target_type) }
                                    span.flag-type-pill { "#" (f.target_id) }
                                    " "
                                    @if let (Some(label), Some(url)) = (&f.target_label, &f.target_url) {
                                        a.flag-target href=(url) { (label) }
                                        @if f.target_withdrawn {
                                            " "
                                            span.badge.badge-unaudited { "withdrawn" }
                                        }
                                    } @else {
                                        span.muted { "(target deleted)" }
                                    }
                                    " "
                                    span.flag-when.muted.small {
                                        "flagged "
                                        @if let Some(ts) = &f.created_at { (time_ago(ts)) }
                                        " by "
                                        a href={ "/u/" (f.reporter_username) } { (f.reporter_username) }
                                    }
                                }
                                div.flag-reason { (f.reason) }
                                div.flag-actions {
                                    form.inline-form action={"/admin/flag/" (f.id) "/resolve"} method="post" {
                                        input type="hidden" name="csrf_token" value=(ctx.csrf_token);
                                        input type="text" name="note" maxlength="500" placeholder="resolution note (optional)";
                                        button.btn-secondary type="submit" { "mark resolved" }
                                    }
                                }
                            }
                        }
                    }
                }
            }

            section.admin-panel {
                div.admin-panel-head {
                    div {
                        h2 { "Storage and backup" }
                        p.muted { "Hosted artifact coverage and backup observability." }
                    }
                }
                dl.admin-kv {
                    div { dt { "Hosted PDFs" } dd { (fmt_int(s.stored_pdfs)) } }
                    div { dt { "Hosted sources" } dd { (fmt_int(s.stored_sources)) } }
                    div { dt { "Total manuscripts" } dd { (fmt_int(s.total_manuscripts)) } }
                    div { dt { "PDF coverage" } dd { (percent(s.stored_pdfs, s.total_manuscripts)) } }
                }
                (gap_card(
                    "Backup status not persisted",
                    "No backup run table, object checksum inventory, restore-test result, or retention status is available to show here."
                ))
            }

            section.admin-panel {
                div.admin-panel-head {
                    div {
                        h2 { "LaTeX compile failures" }
                        p.muted { "Diagnostics for source-upload processing." }
                    }
                }
                (gap_card(
                    "Compile logs are not persisted",
                    "The submit path returns compile errors to the requester, but failed compile logs are not saved for admin review."
                ))
            }

            section.admin-panel {
                div.admin-panel-head {
                    div {
                        h2 { "Provenance and privacy" }
                        p.muted { "Submission disclosure and admin posture." }
                    }
                }
                dl.admin-kv {
                    div { dt { "Audited manuscripts" } dd { (fmt_int(s.audited_manuscripts)) } }
                    div { dt { "Withdrawn manuscripts" } dd { (fmt_int(s.withdrawn_manuscripts)) } }
                    div { dt { "Human name hidden" } dd { (fmt_int(s.hidden_human_manuscripts)) } }
                    div { dt { "AI model hidden" } dd { (fmt_int(s.hidden_ai_manuscripts)) } }
                    div { dt { "Admins" } dd { (fmt_int(s.admin_users)) } }
                    div { dt { "New users, 7d" } dd { (fmt_int(s.new_users_7d)) } }
                }
            }

            section.admin-panel.admin-panel-wide {
                div.admin-panel-head {
                    div {
                        h2 { "User growth" }
                        p.muted { "New accounts by creation day; verified column reflects accounts already email-verified." }
                    }
                    span.admin-mini-stat { (fmt_int(s.total_users)) " total" }
                }
                @if dashboard.user_growth.is_empty() {
                    div.admin-empty { strong { "No user events yet" } }
                } @else {
                    table.admin-table.admin-table-compact {
                        thead { tr { th { "Day" } th { "New accounts" } th { "Email verified" } } }
                        tbody {
                            @for row in &dashboard.user_growth {
                                tr {
                                    td.mono { (row.day) }
                                    td { (fmt_int(row.primary_count)) }
                                    td { (fmt_int(row.secondary_count)) }
                                }
                            }
                        }
                    }
                }
            }

            section.admin-panel.admin-panel-wide {
                div.admin-panel-head {
                    div {
                        h2 { "Recent submissions" }
                        p.muted { "Newest manuscripts and storage/status signals." }
                    }
                    span.admin-mini-stat { (fmt_int(s.manuscripts_24h)) " in 24h" }
                }
                @if dashboard.recent_submissions.is_empty() {
                    div.admin-empty { strong { "No manuscripts yet" } }
                } @else {
                    table.admin-table {
                        thead {
                            tr {
                                th { "Manuscript" }
                                th { "Category" }
                                th { "Submitter" }
                                th { "Status" }
                                th { "Activity" }
                                th { "When" }
                            }
                        }
                        tbody {
                            @for m in &dashboard.recent_submissions {
                                tr {
                                    td.admin-title-cell {
                                        @if let Some(slug) = &m.slug {
                                            @let public_slug = slug.strip_prefix("prexiv:").unwrap_or(slug);
                                            a href={ "/abs/" (public_slug) } { (m.title) }
                                            div.muted.small.mono { (slug) " · v" (m.current_version) }
                                        } @else {
                                            (m.title)
                                        }
                                    }
                                    td.mono { a href={ "/browse/" (m.category) } { (m.category) } }
                                    td { a href={ "/u/" (m.submitter_username) } { (m.submitter_username) } }
                                    td.admin-pill-stack {
                                        @if m.withdrawn {
                                            (status_pill("withdrawn", "admin-pill-warn"))
                                        } @else {
                                            (status_pill("live", "admin-pill-ok"))
                                        }
                                        @if m.has_auditor {
                                            (status_pill("audited", "admin-pill-ok"))
                                        } @else {
                                            (status_pill("unaudited", "admin-pill-muted"))
                                        }
                                        @if m.has_stored_artifact {
                                            (status_pill("stored", "admin-pill-muted"))
                                        }
                                    }
                                    td.muted.small { (m.score) " score · " (m.comment_count) " comments" }
                                    td.muted.small {
                                        @if let Some(ts) = &m.created_at { (time_ago(ts)) }
                                    }
                                }
                            }
                        }
                    }
                }
            }

            section.admin-panel {
                div.admin-panel-head {
                    div {
                        h2 { "Category activity" }
                        p.muted { "Largest categories by total submissions." }
                    }
                }
                @if dashboard.category_stats.is_empty() {
                    div.admin-empty { strong { "No category data" } }
                } @else {
                    table.admin-table.admin-table-compact {
                        thead {
                            tr { th { "Category" } th { "Total" } th { "Live" } th { "Latest" } }
                        }
                        tbody {
                            @for c in &dashboard.category_stats {
                                tr {
                                    td.mono { a href={ "/browse/" (c.category) } { (c.category) } }
                                    td { (fmt_int(c.total)) }
                                    td { (fmt_int(c.live)) }
                                    td.muted.small {
                                        @if let Some(ts) = &c.latest_at { (time_ago(ts)) }
                                    }
                                }
                            }
                        }
                    }
                }
            }

            section.admin-panel {
                div.admin-panel-head {
                    div {
                        h2 { "Unverified high-activity users" }
                        p.muted { "Unverified accounts with submissions, comments, votes, or active API tokens." }
                    }
                }
                @if dashboard.unverified_high_activity_users.is_empty() {
                    div.admin-empty { strong { "No unverified activity found" } }
                } @else {
                    table.admin-table.admin-table-compact {
                        thead { tr { th { "User" } th { "Activity" } th { "Joined" } } }
                        tbody {
                            @for u in &dashboard.unverified_high_activity_users {
                                tr {
                                    td {
                                        a href={ "/u/" (u.username) } { (u.username) }
                                        @if let Some(name) = &u.display_name {
                                            div.muted.small { (name) }
                                        }
                                    }
                                    td.muted.small {
                                        (fmt_int(u.manuscript_count)) " manuscripts · "
                                        (fmt_int(u.comment_count)) " comments · "
                                        (fmt_int(u.vote_count)) " votes · "
                                        (fmt_int(u.token_count)) " tokens"
                                    }
                                    td.muted.small {
                                        @if let Some(ts) = &u.created_at { (time_ago(ts)) }
                                    }
                                }
                            }
                        }
                    }
                }
            }

            section.admin-panel {
                div.admin-panel-head {
                    div {
                        h2 { "Recent accounts" }
                        p.muted { "Newest registrations and trust signals." }
                    }
                }
                @if dashboard.recent_users.is_empty() {
                    div.admin-empty { strong { "No users yet" } }
                } @else {
                    table.admin-table.admin-table-compact {
                        thead { tr { th { "User" } th { "Signals" } th { "Joined" } } }
                        tbody {
                            @for u in &dashboard.recent_users {
                                tr {
                                    td {
                                        a href={ "/u/" (u.username) } { (u.username) }
                                        @if let Some(name) = &u.display_name {
                                            div.muted.small { (name) }
                                        }
                                    }
                                    td.admin-pill-stack {
                                        @if u.email_verified {
                                            (status_pill("email", "admin-pill-ok"))
                                        } @else {
                                            (status_pill("unverified", "admin-pill-warn"))
                                        }
                                        @if u.orcid_oauth_verified {
                                            (status_pill("ORCID", "admin-pill-ok"))
                                        }
                                        @if u.institutional_email {
                                            (status_pill("institutional", "admin-pill-muted"))
                                        }
                                        @if u.is_admin {
                                            (status_pill("admin", "admin-pill-muted"))
                                        }
                                    }
                                    td.muted.small {
                                        @if let Some(ts) = &u.created_at { (time_ago(ts)) }
                                    }
                                }
                            }
                        }
                    }
                }
            }

            section.admin-panel.admin-panel-wide {
                div.admin-panel-head {
                    div {
                        h2 { "Recent audit events" }
                        p.muted { "Newest administrative events. Full log keeps pagination and IP detail." }
                    }
                    a.btn-secondary href="/admin/audit" { "Open full log" }
                }
                @if dashboard.recent_audit.is_empty() {
                    div.admin-empty { strong { "No audit events yet" } }
                } @else {
                    table.admin-table {
                        thead {
                            tr { th { "Log" } th { "When" } th { "Actor" } th { "Action" } th { "Target" } th { "Detail" } }
                        }
                        tbody {
                            @for e in &dashboard.recent_audit {
                                tr {
                                    td.muted.small.mono { "#" (e.id) }
                                    td.muted.small {
                                        @if let Some(ts) = &e.created_at { (time_ago(ts)) }
                                    }
                                    td {
                                        @if let Some(u) = &e.actor_username { a href={ "/u/" (u) } { (u) } }
                                        @else { em.muted { "(system)" } }
                                    }
                                    td { code { (e.action) } }
                                    td.muted.small {
                                        @if let Some(t) = &e.target_type {
                                            (t)
                                            @if let Some(id) = e.target_id { "#" (id) }
                                        }
                                    }
                                    td.small { @if let Some(d) = &e.detail { (d) } }
                                }
                            }
                        }
                    }
                }
            }
        }
    };
    layout("admin · flag queue", ctx, body)
}

pub fn render_audit(
    ctx: &PageCtx,
    entries: &[AuditRow],
    page: i64,
    per: i64,
    total: i64,
) -> Markup {
    let body = html! {
        div.page-header {
            h1 { "Audit log" }
            p.muted {
                (total) " total entries · page " (page) " · "
                a href="/admin" { "← back to flag queue" }
            }
        }
        @if entries.is_empty() {
            p.muted { "No entries yet." }
        } @else {
            table.kv {
                tr {
                    th { "Log" } th { "When" } th { "Actor" } th { "Action" }
                    th { "Target" } th { "Detail" } th { "IP" }
                }
                @for e in entries {
                    tr {
                        td.muted.small.mono { "#" (e.id) }
                        td.muted.small {
                            @if let Some(ts) = &e.created_at { (time_ago(ts)) }
                        }
                        td {
                            @if let Some(u) = &e.actor_username { a href={ "/u/" (u) } { (u) } }
                            @else { em.muted { "(system)" } }
                        }
                        td { code { (e.action) } }
                        td.muted.small {
                            @if let Some(t) = &e.target_type {
                                (t)
                                @if let Some(id) = e.target_id { "#" (id) }
                            }
                        }
                        td.small { @if let Some(d) = &e.detail { (d) } }
                        td.muted.small.mono { @if let Some(ip) = &e.ip { (ip) } }
                    }
                }
            }
            nav.pagination {
                @if page > 1 {
                    a href={ "?page=" (page - 1) } { "← previous" }
                }
                " "
                span.muted { "page " (page) }
                " "
                @if (entries.len() as i64) == per {
                    a href={ "?page=" (page + 1) } { "next →" }
                }
            }
        }
    };
    layout("admin · audit log", ctx, body)
}
