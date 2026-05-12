//! /me/2fa enrollment + status, /login/2fa second-step form.

use maud::{html, Markup, PreEscaped};

use super::layout::{layout, PageCtx};

/// Status panel + enroll / confirm / disable flows on /me/2fa.
pub fn render_status(
    ctx: &PageCtx,
    email: &str,
    enabled: bool,
    enrollment: Option<&(String, String)>,
    error: Option<&str>,
) -> Markup {
    let body = html! {
        div.page-header {
            h1 { "Two-factor authentication" }
            p.muted {
                "Time-based one-time passwords (TOTP). Adds a 6-digit code from your phone to every sign-in. Works with any standard authenticator app — Google Authenticator, 1Password, Authy, Bitwarden, etc."
            }
        }

        @if let Some(e) = error {
            div.form-errors {
                strong { "Please fix:" }
                ul { li { (e) } }
            }
        }

        section.form-section {
            h2 { "Status" }
            p {
                @if enabled {
                    span.account-badge.account-badge-ok { "✓ enabled" }
                    " — your account requires a code at sign-in."
                } @else if enrollment.is_some() {
                    span.account-badge.account-badge-warn { "⚠ enrollment pending" }
                    " — scan the QR below and confirm to finish."
                } @else {
                    span.account-badge.account-badge-warn { "off" }
                    " — sign-in only requires your password."
                }
            }
        }

        @if let Some((secret, qr_html)) = enrollment {
            section.form-section {
                h2 { "Step 1 — scan the QR" }
                p.muted.small {
                    "Open your authenticator app, tap \"Add account\" or the + icon, and scan this code. Your app will show \"PreXiv (" (email) ")\" with a 6-digit code that rotates every 30 seconds."
                }
                div.totp-qr { (PreEscaped(qr_html)) }
                details.totp-manual {
                    summary { "Can't scan? Enter the secret manually" }
                    p.muted.small { "Some apps prefer manual entry. Copy this into your app's \"Setup key\" field, with type Time-based / TOTP / SHA1 / 6 digits / 30s period:" }
                    pre.copy-pre.no-katex { (secret) }
                }
            }
            section.form-section {
                h2 { "Step 2 — confirm" }
                form.submit-form method="post" action="/me/2fa/confirm" autocomplete="off" {
                    input type="hidden" name="csrf_token" value=(ctx.csrf_token);
                    label {
                        span.label-text { "Current 6-digit code " span.req { "*" } }
                        input type="text" name="code" required pattern="[0-9]{6}" inputmode="numeric" autocomplete="one-time-code" maxlength="6" autofocus;
                        span.hint { "Type the code your app is showing right now. Codes rotate every 30 seconds — if it doesn't match, wait for the next one." }
                    }
                    div.form-submit {
                        button.btn-primary.big type="submit" { "Confirm and enable 2FA" }
                    }
                }
            }
        } @else if enabled {
            section.form-section {
                h2 { "Disable 2FA" }
                p.muted.small { "Requires your current password. Removes the TOTP secret from the server; you'll re-enroll if you turn 2FA back on later." }
                form.submit-form method="post" action="/me/2fa/disable" autocomplete="off" {
                    input type="hidden" name="csrf_token" value=(ctx.csrf_token);
                    label {
                        span.label-text { "Current password " span.req { "*" } }
                        input type="password" name="current_password" required autocomplete="current-password";
                    }
                    div.form-submit {
                        button.btn-secondary type="submit" { "Disable 2FA" }
                    }
                }
            }
        } @else {
            section.form-section {
                h2 { "Enable 2FA" }
                p.muted.small {
                    "Generates a fresh secret, shows you a QR code, and asks you to confirm by entering the first code. The secret is stored server-side; we never see the codes themselves."
                }
                form method="post" action="/me/2fa/enable" {
                    input type="hidden" name="csrf_token" value=(ctx.csrf_token);
                    button.btn-primary type="submit" { "Enable 2FA →" }
                }
            }
        }

        p.muted.small style="margin-top: 28px" {
            "Lost access to your authenticator? Recovery is manual — contact the operator. Backup codes are tracked as a future feature."
        }
    };
    layout("Two-factor authentication", ctx, body)
}

/// /login/2fa — second-step form during the sign-in flow.
pub fn render_login_step(ctx: &PageCtx, next: Option<&str>, error: Option<&str>) -> Markup {
    let body = html! {
        div.page-header {
            h1 { "Two-factor sign-in" }
            p.muted {
                "Your account has 2FA enabled. Enter the 6-digit code from your authenticator app to finish signing in."
            }
        }
        @if let Some(e) = error {
            div.form-errors {
                strong { "Couldn't sign in:" }
                ul { li { (e) } }
            }
        }
        form.submit-form method="post" action="/login/2fa" autocomplete="off" {
            input type="hidden" name="csrf_token" value=(ctx.csrf_token);
            @if let Some(n) = next { input type="hidden" name="next" value=(n); }
            label {
                span.label-text { "6-digit code " span.req { "*" } }
                input type="text" name="code" required pattern="[0-9]{6}" inputmode="numeric" autocomplete="one-time-code" maxlength="6" autofocus;
                span.hint { "Codes rotate every 30 seconds. If your phone shows a code that doesn't work, wait for the next one." }
            }
            div.form-submit {
                button.btn-primary.big type="submit" { "Sign in" }
                " "
                a.btn-secondary href="/login" { "Cancel" }
            }
        }
    };
    layout("Two-factor sign-in", ctx, body)
}
