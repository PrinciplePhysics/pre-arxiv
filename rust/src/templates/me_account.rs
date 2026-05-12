//! /me/delete-account confirmation form.

use maud::{html, Markup};

use super::layout::{layout, PageCtx};

pub fn render_delete(ctx: &PageCtx, error: Option<&str>) -> Markup {
    let username = ctx.user.as_ref().map(|u| u.username.as_str()).unwrap_or("");
    let body = html! {
        div.page-header {
            h1 { "Delete your account" }
            p.muted {
                "Permanent. Removes your profile, API tokens, follows, votes, notifications, and any pending email-verification or password-reset state. "
                strong { "Your manuscripts and comments stay on the site" }
                " — they're re-attributed to a "
                code.no-katex { "[deleted]" }
                " placeholder account so existing citations don't break."
            }
        }

        @if let Some(e) = error {
            div.form-errors {
                strong { "Please fix:" }
                ul { li { (e) } }
            }
        }

        section.form-section {
            h2 { "What gets deleted" }
            ul {
                li { "Your profile (username, email, display name, affiliation, bio, ORCID, password)" }
                li { "All active sessions (you'll be signed out everywhere)" }
                li { "All API tokens (any agent using them stops working immediately)" }
                li { "Your follow graph (you stop following anyone; anyone following you stops)" }
                li { "Your votes (the vote totals on manuscripts adjust accordingly)" }
                li { "Your notifications + any pending email-verification or password-reset state" }
                li { "Two-factor authentication enrollment, if you had it" }
            }
        }

        section.form-section {
            h2 { "What stays" }
            ul {
                li {
                    strong { "Your manuscripts. "}
                    "Withdraw them first if you want them tombstoned; otherwise they remain at their permanent URLs, now showing "
                    code.no-katex { "[deleted]" }
                    " as the submitter."
                }
                li {
                    strong { "Your comments. " }
                    "Same — re-attributed to "
                    code.no-katex { "[deleted]" }
                    "."
                }
                li { "The DOI and audit-log references to your past actions, with the actor field nulled." }
            }
        }

        form.submit-form method="post" action="/me/delete-account" autocomplete="off" {
            input type="hidden" name="csrf_token" value=(ctx.csrf_token);

            section.form-section {
                h2 { "Confirm" }
                label {
                    span.label-text { "Type your username " span.muted.small { "(\"" (username) "\")" } " to confirm " span.req { "*" } }
                    input type="text" name="confirm_username" required autocomplete="off";
                }
                label {
                    span.label-text { "Current password " span.req { "*" } }
                    input type="password" name="current_password" required autocomplete="current-password";
                }
            }

            div.form-submit {
                button.btn-primary.big.danger type="submit" { "Delete my account permanently" }
                " "
                a.btn-secondary href="/me/edit" { "Cancel" }
            }
        }

        p.muted.small style="margin-top: 28px" {
            "Want a copy of your data before deleting? "
            a href="/me/export" { "Download a JSON export →" }
            " — it includes your profile, every manuscript, every comment, every vote, follows, and token metadata."
        }
    };
    layout("Delete account", ctx, body)
}
