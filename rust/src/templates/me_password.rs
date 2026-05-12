//! /me/password — change-password form template.

use maud::{html, Markup};

use super::layout::{layout, PageCtx};

pub fn render(ctx: &PageCtx, error: Option<&str>) -> Markup {
    let username = ctx.user.as_ref().map(|u| u.username.as_str()).unwrap_or("");
    let body = html! {
        div.page-header {
            h1 { "Change password" }
            p.muted {
                "Enter your current password and choose a new one. The new password is checked against the public Have-I-Been-Pwned breach corpus before it's accepted."
            }
        }

        @if let Some(e) = error {
            div.form-errors {
                strong { "Please fix:" }
                ul { li { (e) } }
            }
        }

        form.submit-form method="post" action="/me/password" autocomplete="off" {
            input type="hidden" name="csrf_token" value=(ctx.csrf_token);

            section.form-section {
                h2 { "Current password" }
                label {
                    span.label-text { "Current password " span.req { "*" } }
                    input type="password" name="current_password" required autocomplete="current-password";
                    span.hint { "Required to confirm it's you — defends against someone with your session hijacking the change." }
                }
            }

            section.form-section {
                h2 { "New password" }
                label {
                    span.label-text { "New password " span.req { "*" } }
                    input type="password" name="new_password" required minlength="8" autocomplete="new-password";
                    span.hint { "At least 8 characters. Must differ from your current one. Pick something not in any prior data breach." }
                }
                label {
                    span.label-text { "Confirm new password " span.req { "*" } }
                    input type="password" name="new_password_confirm" required minlength="8" autocomplete="new-password";
                    span.hint { "Re-type the new password above. Mismatches won't be silently submitted." }
                }
            }

            div.form-submit {
                button.btn-primary.big type="submit" { "Update password" }
                " "
                a.btn-secondary href="/me/edit" { "Cancel" }
            }
        }

        p.muted.small style="margin-top: 28px;" {
            "Forgot your current password? "
            a href="/forgot-password" { "Reset it by email" }
            " — you'll be sent a one-time link valid for 1 hour."
            " (You're @" (username) ", so this applies to your account.)"
        }
    };
    layout("Change password", ctx, body)
}
