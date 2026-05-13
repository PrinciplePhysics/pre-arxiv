use maud::{html, Markup};

use super::layout::{layout, PageCtx};
use crate::routes::me_edit::EditValues;

// `orcid_flash` carries inline feedback from a recent ORCID OAuth or
// public-name-match attempt — `(message, is_error)`, or None when
// nothing's fresh. Rendered INSIDE the verified-scholar status panel so
// users don't have to scroll back up to discover what happened.
pub fn render(
    ctx: &PageCtx,
    v: &EditValues,
    errors: &[String],
    pending_new_email: Option<&str>,
    orcid_flash: Option<(&str, bool)>,
    orcid_oauth_unavailable: Option<&str>,
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
                    ". Verification means either an authenticated ORCID OAuth binding or a verified email on an institutional domain (.edu, .ac.<cc>, .edu.<cc>, or our R&D-org allowlist). The older manual ORCID name-match below is displayed on profiles but is not ownership proof. Unverified work is still reachable via "
                    code { "/browse" }
                    " and search; it just doesn't get the front-page slot."
                }
                (verified_scholar_status_panel(user, &v.orcid, ctx.csrf_token.as_str(), orcid_flash))
            }

            section.form-section {
                h2 { "ORCID " span.muted { "(optional)" } }
                p.muted.small {
                    "For real ORCID verification, connect through ORCID OAuth. Manual paste-and-name-match remains available as a profile hint only; it does not prove ownership."
                }
                div.orcid-oauth-card {
                    div {
                        strong { "Authenticated ORCID binding" }
                        p.muted.small.no-katex {
                            @if let Some(msg) = orcid_oauth_unavailable {
                                (msg)
                            } @else {
                                "You will be sent to orcid.org, sign in there, and authorize PreXiv to receive your authenticated ORCID iD."
                            }
                        }
                    }
                    @if orcid_oauth_unavailable.is_some() {
                        button.btn-secondary type="button" disabled { "ORCID not configured" }
                    } @else {
                        a.btn-primary href="/me/orcid/connect" { "Connect with ORCID" }
                    }
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
                        "Save & name-match ORCID"
                    }
                    span.muted.small.no-katex {
                        "Legacy check only: saves edits, fetches the public ORCID record, and compares the public name to your PreXiv display name."
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

/// Status panel — read-only summary of the ownership-grade
/// verified-scholar signals plus the legacy public-name-match hint.
/// The actions live down in the ORCID section: OAuth for ownership,
/// and `formaction` name matching for the legacy public-record check.
fn verified_scholar_status_panel(
    user: Option<&crate::models::User>,
    _orcid_in_form: &str,
    _csrf_token: &str,
    orcid_flash: Option<(&str, bool)>,
) -> Markup {
    let orcid_name_matched = user.map(|u| u.is_orcid_verified()).unwrap_or(false);
    let orcid_oauth = user.map(|u| u.is_orcid_oauth_verified()).unwrap_or(false);
    let inst_email = user
        .map(|u| u.is_verified() && u.is_institutional_email())
        .unwrap_or(false);
    let stored_orcid = user.and_then(|u| u.orcid.as_deref()).unwrap_or("");
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
                        "Requires a verified email address on an institutional / R&D-org domain."
                    }
                }
                div.vsp-row-status {
                    @if inst_email {
                        span.vsp-pill.vsp-pill-ok { "✓ verified" }
                    } @else {
                        span.vsp-pill.vsp-pill-pending { "not yet" }
                    }
                }
            }
            div.vsp-row {
                div.vsp-row-label {
                    strong { "Authenticated ORCID" }
                    @if !stored_orcid.is_empty() {
                        " " code.no-katex { (stored_orcid) }
                    }
                    span.muted.small.no-katex {
                        @if orcid_oauth {
                            "Connected through ORCID OAuth. This proves account control and grants verified-scholar status."
                        } @else {
                            "Not connected. Use "
                            strong { "Connect with ORCID" }
                            " below for ownership-grade verification."
                        }
                    }
                }
                div.vsp-row-status {
                    @if orcid_oauth {
                        span.vsp-pill.vsp-pill-ok { "authenticated" }
                    } @else {
                        span.vsp-pill.vsp-pill-pending { "not connected" }
                    }
                }
            }
            div.vsp-row {
                div.vsp-row-label {
                    strong { "ORCID public-name match" }
                    span.muted.small.no-katex {
                        @if stored_orcid.is_empty() {
                            "Optional legacy check: paste an iD below and compare its public name with your display name."
                        } @else if orcid_name_matched {
                            "Public ORCID name matched your PreXiv display name. This is displayed on your profile but is not ownership proof."
                        } @else {
                            "Saved but not name-matched."
                        }
                    }
                }
                div.vsp-row-status {
                    @if orcid_name_matched {
                        span.vsp-pill.vsp-pill-ok { "name matched" }
                    } @else {
                        span.vsp-pill.vsp-pill-pending { "not matched" }
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
