use maud::{html, Markup};

use super::layout::{layout, time_ago, PageCtx};
use crate::routes::admin::{AuditRow, FlagRow};

pub fn render_queue(ctx: &PageCtx, flags: &[FlagRow]) -> Markup {
    let body = html! {
        div.page-header {
            h1 { "Flag queue" }
            p.muted {
                "Open reports from users. Resolve when handled — the flag stays in the DB but drops out of this list. "
                a href="/admin/audit" { "View audit log →" }
            }
        }
        @if flags.is_empty() {
            p.muted { "No open flags. Nice." }
        } @else {
            ul.flag-list {
                @for f in flags {
                    li.flag-row {
                        div.flag-head {
                            span.flag-type-pill { (f.target_type) }
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
                    th { "When" } th { "Actor" } th { "Action" }
                    th { "Target" } th { "Detail" } th { "IP" }
                }
                @for e in entries {
                    tr {
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
