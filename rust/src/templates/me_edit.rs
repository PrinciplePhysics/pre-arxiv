use maud::{html, Markup};

use super::layout::{layout, PageCtx};
use crate::routes::me_edit::EditValues;

// `orcid_flash` carries inline feedback from a recent POST
// /me/verify-orcid attempt — `(message, is_error)`, or None when
// nothing's fresh. Rendered INSIDE the verified-scholar status panel
// so users don't have to scroll back up to discover what happened.
pub fn render(
    ctx: &PageCtx,
    v: &EditValues,
    errors: &[String],
    pending_new_email: Option<&str>,
    orcid_flash: Option<(&str, bool)>,
) -> Markup {
    let user = ctx.user.as_ref();
    let username = user.map(|u| u.username.as_str()).unwrap_or("");
    let email = user.map(|u| u.email.as_str()).unwrap_or("");
    let verified = user.map(|u| u.is_verified()).unwrap_or(false);
    let display_name_current = user.and_then(|u| u.display_name.as_deref()).unwrap_or("");
    let affiliation_current = user.and_then(|u| u.affiliation.as_deref()).unwrap_or("");
    let created_at = user.and_then(|u| u.created_at);
    let created_at_str = created_at
        .map(|d| d.format("%Y-%m-%d").to_string())
        .unwrap_or_default();

    let body = html! {
        div.page-header {
            h1 { "Edit profile" }
            p.muted {
                "Tweak your account settings. Display fields apply to past and future submissions where you appear as the submitter."
            }
        }

        // arxiv-style summary panel: read-only key fields + action links.
        section.account-info-panel aria-label="Account information" {
            h2.account-info-title { "Account information" }
            dl.account-info-grid {
                dt { "E-mail" }
                dd {
                    span.account-info-email.no-katex { (email) }
                    @if verified {
                        span.account-badge.account-badge-ok { "✓ verified" }
                    } @else {
                        span.account-badge.account-badge-warn { "⚠ not verified" }
                    }
                }
                dt { "Username" }
                dd { code.no-katex { (username) } }
                @if !display_name_current.is_empty() {
                    dt { "Display name" }
                    dd { (display_name_current) }
                }
                @if !affiliation_current.is_empty() {
                    dt { "Affiliation" }
                    dd { (affiliation_current) }
                }
                @if !created_at_str.is_empty() {
                    dt { "Member since" }
                    dd { (created_at_str) }
                }
            }
            nav.account-info-actions aria-label="Account actions" {
                a.account-info-link href="/me/email"    { "Change email" }
                span.account-info-sep { "|" }
                a.account-info-link href="/me/password" { "Change password" }
                span.account-info-sep { "|" }
                a.account-info-link href="/me/2fa"      { "Two-factor auth" }
                span.account-info-sep { "|" }
                a.account-info-link href="/me/tokens"   { "API tokens" }
                span.account-info-sep { "|" }
                a.account-info-link href="/me/export"   { "Export data" }
                span.account-info-sep { "|" }
                a.account-info-link.account-info-danger href="/me/delete-account" { "Delete account" }
            }
        }

        @if !verified {
            (verify_banner(&ctx.csrf_token, email, ctx.pending_verify_token.as_deref()))
        }

        @if let Some(pending) = pending_new_email {
            (email_change_banner(&ctx.csrf_token, pending, ctx.pending_email_change_token.as_deref()))
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
                h2 {
                    "Verified-scholar status"
                }
                p.muted.small.no-katex {
                    "PreXiv's default ranked listings (" code { "/" } " " code { "/new" } " "
                    code { "/top" } " " code { "/audited" }
                    ") only surface manuscripts from "
                    strong { "verified scholars" }
                    ". Verification means one of: (a) an ORCID iD whose public name on "
                    a href="https://orcid.org" target="_blank" rel="noopener" { "orcid.org" }
                    " matches your PreXiv display name, OR (b) a verified email on an institutional domain (.edu, .ac.<cc>, .edu.<cc>, or our R&D-org allowlist). Unverified work is still reachable via "
                    code { "/browse" }
                    " and search; it just doesn't get the front-page slot."
                }
                (verified_scholar_status_panel(user, &v.orcid, ctx.csrf_token.as_str(), orcid_flash))
            }

            section.form-section {
                h2 { "ORCID " span.muted { "(optional)" } }
                p.muted.small {
                    "Paste your ORCID iD here and click "
                    strong { "Save & Verify" }
                    ". We fetch the public record from "
                    code { "pub.orcid.org" }
                    " and compare the name on file with your PreXiv display name (above). The verification is a one-step name match — no OAuth — so make sure your display name matches what's on your ORCID page first."
                }
                label {
                    span.label-text { "ORCID iD" }
                    input type="text" name="orcid" maxlength="19"
                          pattern="\\d{4}-\\d{4}-\\d{4}-\\d{3}[\\dX]"
                          value=(v.orcid)
                          placeholder="0000-0002-1825-0097";
                    span.hint { "Format: " code { "XXXX-XXXX-XXXX-XXXX" } " (last char may be X). Editing this clears any prior verification." }
                }
                // Inline action — `formaction` overrides the outer
                // form's `action`, so this single button both saves
                // pending field edits AND triggers verification in one
                // network round-trip. Visually de-emphasized vs the
                // primary "Save changes" at the bottom so the user
                // understands this is the verification path, not the
                // generic save.
                div.orcid-inline-actions {
                    button.btn-primary type="submit" formaction="/me/verify-orcid" {
                        "Save & Verify ORCID"
                    }
                    span.muted.small.no-katex {
                        "Submits this whole form to "
                        code { "/me/verify-orcid" }
                        " — saves edits, then checks your ORCID record."
                    }
                }
            }

            div.form-submit {
                button.btn-primary.big type="submit" { "Save changes" }
                " "
                a.btn-secondary href={ "/u/" (username) } { "Cancel" }
            }
        }
    };
    layout("Edit profile", ctx, body)
}

/// Status panel — read-only summary of the two verified-scholar
/// signals (institutional email + ORCID). The verification *action*
/// lives down in the ORCID section as "Save & Verify ORCID", which
/// uses `formaction` to submit the whole edit form to /me/verify-orcid
/// in one click. Putting the action next to the input it acts on is
/// the clearer UX; this panel is for "where am I right now."
fn verified_scholar_status_panel(
    user: Option<&crate::models::User>,
    _orcid_in_form: &str,
    _csrf_token: &str,
    orcid_flash: Option<(&str, bool)>,
) -> Markup {
    let orcid_verified = user.map(|u| u.is_orcid_verified()).unwrap_or(false);
    let inst_email     = user.map(|u| u.is_institutional_email()).unwrap_or(false);
    let stored_orcid   = user.and_then(|u| u.orcid.as_deref()).unwrap_or("");
    html! {
        div.verified-scholar-panel {
            @if let Some((msg, is_err)) = orcid_flash {
                @let cls = if is_err { "vsp-flash vsp-flash-err" } else { "vsp-flash vsp-flash-ok" };
                div.(cls) role="status" aria-live="polite" {
                    @if is_err {
                        span.vsp-flash-icon aria-hidden="true" { "⚠" }
                    } @else {
                        span.vsp-flash-icon aria-hidden="true" { "✓" }
                    }
                    span.vsp-flash-msg { (msg) }
                }
            }
            div.vsp-row {
                div.vsp-row-label {
                    strong { "Institutional email" }
                    span.muted.small.no-katex {
                        "Auto-detected at register / email change from your verified email's domain."
                    }
                }
                div.vsp-row-status {
                    @if inst_email {
                        span.vsp-pill.vsp-pill-ok { "✓ verified" }
                    } @else {
                        span.vsp-pill.vsp-pill-pending { "not on file" }
                    }
                }
            }
            div.vsp-row {
                div.vsp-row-label {
                    strong { "ORCID iD" }
                    @if !stored_orcid.is_empty() {
                        " " code.no-katex { (stored_orcid) }
                    }
                    span.muted.small.no-katex {
                        @if stored_orcid.is_empty() {
                            "Paste an iD in the ORCID section below, then click "
                            strong { "Save & Verify ORCID" }
                            "."
                        } @else if orcid_verified {
                            "Public ORCID name matched your display name."
                        } @else {
                            "Saved but not verified. Use "
                            strong { "Save & Verify ORCID" }
                            " in the ORCID section below."
                        }
                    }
                }
                div.vsp-row-status {
                    @if orcid_verified {
                        span.vsp-pill.vsp-pill-ok { "✓ verified" }
                    } @else {
                        span.vsp-pill.vsp-pill-pending { "not yet" }
                    }
                }
            }
        }
    }
}

/// Banner shown at the top of /me/edit (and /submit) when the current
/// user's email isn't verified.
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

/// Banner shown when an email-change is pending. Carries the inline
/// confirm button if the session has the plaintext token (just-minted
/// after submitting the form), plus a Cancel control.
pub fn email_change_banner(
    csrf_token: &str,
    new_email: &str,
    pending_token: Option<&str>,
) -> Markup {
    html! {
        div.verify-banner.verify-banner-info role="status" {
            @if let Some(token) = pending_token {
                div.verify-banner-text {
                    strong { "Pending email change to " (new_email) }
                    " — click below to confirm. We've also queued a confirmation email to that address; whichever you click first wins."
                }
                div.verify-banner-actions {
                    a.btn-primary href={ "/confirm-email-change/" (token) } { "Confirm new email →" }
                    form.verify-banner-resend method="post" action="/me/email/cancel" {
                        input type="hidden" name="csrf_token" value=(csrf_token);
                        button.btn-secondary type="submit" { "Cancel" }
                    }
                }
            } @else {
                div.verify-banner-text {
                    strong { "Pending email change to " (new_email) }
                    ". A confirmation link is in your inbox — click it to finish. To get a fresh link instead, "
                    a href="/me/email" { "re-request the change" }
                    "."
                }
                form.verify-banner-resend method="post" action="/me/email/cancel" {
                    input type="hidden" name="csrf_token" value=(csrf_token);
                    button.btn-secondary type="submit" { "Cancel change" }
                }
            }
        }
    }
}
