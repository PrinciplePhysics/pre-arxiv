use maud::{html, Markup};

use super::layout::{layout, PageCtx};
use crate::routes::me_edit::EditValues;

pub fn render(ctx: &PageCtx, v: &EditValues, errors: &[String]) -> Markup {
    let username = ctx.user.as_ref().map(|u| u.username.as_str()).unwrap_or("");
    let unverified = ctx.user.as_ref().map(|u| !u.is_verified()).unwrap_or(false);
    let email = ctx.user.as_ref().map(|u| u.email.as_str()).unwrap_or("");
    let body = html! {
        div.page-header {
            h1 { "Edit profile" }
            p.muted {
                "Tweak your public-facing display fields. Changes here also apply to past and future submissions where you appear as the submitter."
            }
        }

        @if unverified {
            (verify_banner(&ctx.csrf_token, email, ctx.pending_verify_token.as_deref()))
        }

        @if !errors.is_empty() {
            div.form-errors {
                strong { "Please fix the following:" }
                ul { @for e in errors { li { (e) } } }
            }
        }

        form.submit-form method="post" action="/me/edit" {
            input type="hidden" name="csrf_token" value=(ctx.csrf_token);

            section.form-section {
                h2 { "Display" }
                label {
                    span.label-text { "Display name" }
                    input type="text" name="display_name" maxlength="200" value=(v.display_name)
                          placeholder="Jane Doe";
                    span.hint { "Optional. Shown alongside your username on manuscripts and comments." }
                }
                label {
                    span.label-text { "Affiliation" }
                    input type="text" name="affiliation" maxlength="200" value=(v.affiliation)
                          placeholder="MIT — Lab for Plausible Theorems";
                    span.hint { "Free-form. Where (or if) you do this work." }
                }
                label {
                    span.label-text { "Bio" }
                    textarea name="bio" maxlength="2000" rows="4"
                             placeholder="Optional. A few sentences about yourself." { (v.bio) }
                }
            }

            section.form-section {
                h2 { "ORCID " span.muted { "(optional, unverified)" } }
                p.muted.small {
                    "ORCID is a public identifier for researchers (orcid.org). PreXiv displays the value you enter "
                    strong { "without verification" }
                    " — there is no OAuth handshake yet. Treat the badge readers see as a self-claim. Real verification is a planned future step."
                }
                label {
                    span.label-text { "ORCID iD" }
                    input type="text" name="orcid" maxlength="19"
                          pattern="\\d{4}-\\d{4}-\\d{4}-\\d{3}[\\dX]"
                          value=(v.orcid)
                          placeholder="0000-0002-1825-0097";
                    span.hint { "Format: " code { "XXXX-XXXX-XXXX-XXXX" } " (last char may be X)." }
                }
            }

            div.form-submit {
                button.btn-primary.big type="submit" { "Save changes" }
                " "
                a.btn-secondary href={ "/u/" (username) } { "Cancel" }
            }
        }

        section.form-section.me-edit-security {
            h2 { "Security" }
            p.muted.small {
                "Update your password. We'll confirm your current password and check the new one against the public Have-I-Been-Pwned breach corpus before saving."
            }
            p {
                a.btn-secondary href="/me/password" { "Change your password →" }
            }
        }
    };
    layout("Edit profile", ctx, body)
}

/// Banner shown at the top of /me/edit (and /submit) when the current
/// user's email isn't verified.
///
/// When the session carries a `pending_verify_token` (set by
/// /register and /me/resend-verification), the banner promotes that
/// token to a one-click "Verify my email →" button — same /verify/{token}
/// endpoint the email link would target, no inbox round-trip needed.
/// This is the fallback that keeps PreXiv usable while the upstream
/// mail provider's anti-abuse activation is pending.
///
/// Without a session token, the banner falls back to the original
/// "check your inbox / resend" affordance.
pub fn verify_banner(csrf_token: &str, email: &str, pending_token: Option<&str>) -> Markup {
    html! {
        div.verify-banner role="status" {
            @if let Some(token) = pending_token {
                div.verify-banner-text {
                    strong { "Email not verified yet." }
                    " "
                    "Click the button to verify your email and unlock manuscript submission. "
                    "A copy is also queued for delivery to "
                    strong { (email) }
                    " (email delivery is in setup; the in-browser link is the fast path)."
                }
                div.verify-banner-actions {
                    a.btn-primary href={ "/verify/" (token) } { "Verify my email →" }
                    form.verify-banner-resend method="post" action="/me/resend-verification" {
                        input type="hidden" name="csrf_token" value=(csrf_token);
                        button.btn-secondary type="submit" { "New link" }
                    }
                }
            } @else {
                div.verify-banner-text {
                    strong { "Email not verified yet." }
                    " "
                    "We sent a verification link to "
                    strong { (email) }
                    " when you registered. Click the link in that email to enable manuscript submission. If you didn't get it, resend it now:"
                }
                form.verify-banner-resend method="post" action="/me/resend-verification" {
                    input type="hidden" name="csrf_token" value=(csrf_token);
                    button.btn-secondary type="submit" { "Resend verification" }
                }
            }
        }
    }
}
