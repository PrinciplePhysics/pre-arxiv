use maud::{html, Markup};

use super::layout::{layout, PageCtx};
use crate::routes::me_edit::EditValues;

pub fn render(ctx: &PageCtx, v: &EditValues, errors: &[String]) -> Markup {
    let username = ctx.user.as_ref().map(|u| u.username.as_str()).unwrap_or("");
    let body = html! {
        div.page-header {
            h1 { "Edit profile" }
            p.muted {
                "Tweak your public-facing display fields. Changes here also apply to past and future submissions where you appear as the submitter."
            }
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
    };
    layout("Edit profile", ctx, body)
}
