//! /me/email — change-email form.

use maud::{html, Markup};

use super::layout::{layout, PageCtx};

pub fn render(
    ctx: &PageCtx,
    current_email: &str,
    pending_new_email: Option<&str>,
    error: Option<&str>,
) -> Markup {
    let body = html! {
        div.page-header {
            h1 { "Change email" }
            p.muted {
                "Update the address used for password recovery and notifications. To prove you control the new address, we'll send a confirmation link there — the change only takes effect once you click it."
            }
        }

        @if let Some(pending) = pending_new_email {
            div.verify-banner role="status" {
                div.verify-banner-text {
                    strong { "Pending change: " (pending) }
                    " — already requested. Click the confirmation link we sent to that address (or use the inline link on "
                    a href="/me/edit" { "your profile" }
                    "). Submitting the form below will discard the current pending change and start a new one."
                }
                form.verify-banner-resend method="post" action="/me/email/cancel" {
                    input type="hidden" name="csrf_token" value=(ctx.csrf_token);
                    button.btn-secondary type="submit" { "Cancel pending change" }
                }
            }
        }

        @if let Some(e) = error {
            div.form-errors {
                strong { "Please fix:" }
                ul { li { (e) } }
            }
        }

        form.submit-form method="post" action="/me/email" autocomplete="off" {
            input type="hidden" name="csrf_token" value=(ctx.csrf_token);

            section.form-section {
                h2 { "Current email" }
                p.muted.small.no-katex { (current_email) }
            }

            section.form-section {
                h2 { "New email" }
                label {
                    span.label-text { "New email address " span.req { "*" } }
                    input type="email" name="new_email" required maxlength="254" autocomplete="email";
                    span.hint { "We'll send a confirmation link to this address; you'll need to click it to finish the change." }
                }
            }

            section.form-section {
                h2 { "Confirm with current password" }
                label {
                    span.label-text { "Current password " span.req { "*" } }
                    input type="password" name="current_password" required autocomplete="current-password";
                    span.hint { "Required to confirm it's you — defends against someone with your session changing the address out from under you." }
                }
            }

            div.form-submit {
                button.btn-primary.big type="submit" { "Send confirmation link" }
                " "
                a.btn-secondary href="/me/edit" { "Cancel" }
            }
        }
    };
    layout("Change email", ctx, body)
}
