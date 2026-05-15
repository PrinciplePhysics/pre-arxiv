//! Templates for the forgot-password flow:
//!
//!   /forgot-password         — email/username form
//!   /forgot-password/sent    — generic confirmation page (no enumeration)
//!   /reset-password/{token}  — new-password form (or "link invalid" view)

use maud::{html, Markup};

use super::layout::{layout, PageCtx};

/// /forgot-password
pub fn render_forgot(ctx: &PageCtx, error: Option<&str>) -> Markup {
    let body = html! {
        div.page-header {
            h1 { "Reset your password" }
            p.muted {
                "Enter the email address or username on your account. We'll email you a one-time link to set a new password. The link is good for 1 hour."
            }
        }

        @if let Some(e) = error {
            div.form-errors {
                strong { "Please fix:" }
                ul { li { (e) } }
            }
        }

        form.submit-form method="post" action="/forgot-password" autocomplete="off" {
            input type="hidden" name="csrf_token" value=(ctx.csrf_token);
            label {
                span.label-text { "Email or username " span.req { "*" } }
                input type="text" name="identifier" required maxlength="254" autofocus
                      autocomplete="email" placeholder="you@example.com or your_username";
                span.hint { "We won't tell you whether the address is in our system — that's deliberate (it stops drive-by enumeration). Either way you'll see the same confirmation." }
            }
            div.form-submit {
                button.btn-primary.big type="submit" { "Send reset link" }
                " "
                a.btn-secondary href="/login" { "Back to login" }
            }
        }

        p.muted.small style="margin-top: 28px;" {
            "Remember your password? "
            a href="/login" { "Sign in" }
            ". Don't have an account yet? "
            a href="/register" { "Register" }
            "."
        }
    };
    layout("Reset password", ctx, body)
}

/// /forgot-password/sent  — generic confirmation, no enumeration leak.
pub fn render_sent(ctx: &PageCtx) -> Markup {
    let body = html! {
        div.page-header {
            h1 { "Check your inbox" }
            p.muted {
                "If an account exists for that email or username, we've sent a password-reset link to the address on file. The link is valid for 1 hour."
            }
        }

        section.form-section {
            h2 { "Didn't receive it?" }
            ul {
                li { "Check your spam folder — the message comes from " strong { "PreXiv" } " at " code { "noreply@prexiv.net" } "." }
                li { "Make sure you used the email tied to your account (not an alias)." }
                li {
                    "If you registered very recently and haven't verified your email yet, the address may still resolve — but the reset email will land at whatever address you typed in at register time."
                }
                li {
                    a href="/forgot-password" { "Request another link" }
                    " — the new one supersedes the previous; only the most recent is redeemable."
                }
            }
        }

        p {
            a.btn-secondary href="/login" { "Back to login" }
        }
    };
    layout("Check your inbox", ctx, body)
}

/// /reset-password/{token}
pub fn render_reset(ctx: &PageCtx, token: &str, token_valid: bool, error: Option<&str>) -> Markup {
    if !token_valid {
        let body = html! {
            div.page-header {
                h1 { "Link invalid or expired" }
                p.muted {
                    "This password-reset link doesn't match a pending request, or it's older than 1 hour. Request a fresh one."
                }
            }
            p {
                a.btn-primary href="/forgot-password" { "Request a new link" }
                " "
                a.btn-secondary href="/login" { "Back to login" }
            }
        };
        return layout("Link invalid", ctx, body);
    }

    let body = html! {
        div.page-header {
            h1 { "Set a new password" }
            p.muted {
                "You're setting a new password for the account that requested this reset. The link will be consumed after you submit; if you need it again you'll request a new one."
            }
        }

        @if let Some(e) = error {
            div.form-errors {
                strong { "Please fix:" }
                ul { li { (e) } }
            }
        }

        form.submit-form method="post" action={ "/reset-password/" (token) } autocomplete="off" {
            input type="hidden" name="csrf_token" value=(ctx.csrf_token);
            label {
                span.label-text { "New password " span.req { "*" } }
                input type="password" name="new_password" required minlength="8" autocomplete="new-password" autofocus;
                span.hint { "At least 8 characters. Checked against the public Have-I-Been-Pwned breach corpus — pick something not in any prior leak." }
            }
            label {
                span.label-text { "Confirm new password " span.req { "*" } }
                input type="password" name="new_password_confirm" required minlength="8" autocomplete="new-password";
                span.hint { "Re-type the new password above." }
            }
            div.form-submit {
                button.btn-primary.big type="submit" { "Set new password and sign in" }
            }
        }

        p.muted.small style="margin-top: 28px;" {
            "After you submit, your old password stops working immediately and you're signed in on this browser. Sessions on other devices remain active until you sign out from them — log out from "
            a href="/me/edit" { "/me/edit" }
            " on each if you suspect anyone else may have had access."
        }
    };
    layout("Set new password", ctx, body)
}
