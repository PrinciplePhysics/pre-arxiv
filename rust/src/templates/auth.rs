use maud::{html, Markup};

use super::layout::{layout, PageCtx};

pub fn render_login(ctx: &PageCtx, error: Option<&str>, next: Option<&str>) -> Markup {
    let body = html! {
        div.page-header {
            h1 { "Sign in" }
            p.muted { "Welcome back. Sign in to comment, vote, submit manuscripts, or manage your tokens." }
        }
        @if let Some(e) = error {
            div.form-errors {
                strong { "Sign-in failed:" }
                ul { li { (e) } }
            }
        }
        div.auth-shell {
            form.submit-form.auth-form method="post" action="/login" {
                input type="hidden" name="csrf_token" value=(ctx.csrf_token);
                @if let Some(n) = next { input type="hidden" name="next" value=(n); }

                section.form-section {
                    label {
                        span.label-text { "Username or email" }
                        input type="text" name="identifier" required autofocus
                              autocomplete="username" maxlength="254";
                    }
                    label {
                        span.label-text { "Password" }
                        input type="password" name="password" required
                              autocomplete="current-password";
                    }
                }

                div.form-submit {
                    button.btn-primary.big type="submit" { "Sign in" }
                    " "
                    a.btn-secondary href="/register" { "Create an account" }
                }

                p.muted.small.login-forgot {
                    a href="/forgot-password" { "Forgot your password?" }
                    " — we'll email you a one-time reset link."
                }
            }

            aside.auth-aside aria-label="PreXiv account guide" {
                h2 { "What your account unlocks" }
                ul.auth-facts {
                    li {
                        strong { "Reading stays public." }
                        " Browse, search, download, and cite manuscripts without signing in."
                    }
                    li {
                        strong { "Writing requires verification." }
                        " Submit, revise, comment, vote, follow, flag, and mint API tokens after confirming your email."
                    }
                    li {
                        strong { "Agents use your authority." }
                        " API tokens let an AI agent do the same actions you can do, and can be revoked at any time."
                    }
                }
                div.auth-side-links {
                    a href="/permissions" { "Permission model" }
                    a href="/guidelines" { "Submission guidelines" }
                }
            }
        }
    };
    layout("Sign in", ctx, body)
}

#[derive(Default)]
pub struct RegisterForm {
    pub username: String,
    pub email: String,
    pub display_name: String,
}

pub fn render_register(ctx: &PageCtx, error: Option<&str>, form: &RegisterForm) -> Markup {
    let body = html! {
        div.page-header {
            h1 { "Register" }
            p.muted {
                "Create an account to submit manuscripts, comment, vote, and mint API tokens. By registering you agree to the "
                a href="/tos" { "ToS" }
                " and "
                a href="/privacy" { "Privacy Policy" }
                "."
            }
        }
        @if let Some(e) = error {
            div.form-errors {
                strong { "Please fix the following:" }
                ul { li { (e) } }
            }
        }
        div.auth-shell {
            form.submit-form.auth-form method="post" action="/register" {
                input type="hidden" name="csrf_token" value=(ctx.csrf_token);

                section.form-section {
                    label {
                        span.label-text { "Username " span.req { "*" } }
                        input type="text" name="username" required
                              pattern="[a-zA-Z0-9_-]{3,32}"
                              maxlength="32" autocomplete="username"
                              value=(form.username);
                        span.hint.no-katex { "3–32 characters; letters, digits, underscore, or hyphen. Shown on your manuscripts and comments." }
                    }
                    label {
                        span.label-text { "Email " span.req { "*" } }
                        input type="email" name="email" required
                              autocomplete="email" maxlength="254"
                              value=(form.email);
                        span.hint.no-katex { "Used for account recovery; never shown publicly. Verification gates submission." }
                    }
                    label {
                        span.label-text { "Password " span.req { "*" } }
                        input type="password" name="password" required
                              minlength="8" autocomplete="new-password";
                        span.hint.no-katex {
                            "At least 8 characters. We check Have-I-Been-Pwned and reject known-breached passwords — pick something not in any prior leak."
                        }
                    }
                    label {
                        span.label-text { "Confirm password " span.req { "*" } }
                        input type="password" name="password_confirm" required
                              minlength="8" autocomplete="new-password";
                        span.hint.no-katex {
                            "Re-type the password above. Mismatches won't be silently submitted."
                        }
                    }
                    label {
                        span.label-text { "Display name " span.muted { "(optional)" } }
                        input type="text" name="display_name" maxlength="120"
                              autocomplete="name"
                              value=(form.display_name);
                        span.hint.no-katex { "How your name appears alongside your username. Leave blank to show just the username." }
                    }
                }

                div.form-submit {
                    button.btn-primary.big type="submit" { "Register" }
                    " "
                    a.btn-secondary href="/login" { "I already have an account" }
                }
            }

            aside.auth-aside aria-label="Registration guide" {
                h2 { "How PreXiv accounts work" }
                ol.auth-steps {
                    li {
                        strong { "Create the account." }
                        " Username is public; email stays private."
                    }
                    li {
                        strong { "Verify your email." }
                        " Verification gates submissions, revisions, comments, votes, follows, flags, and API tokens."
                    }
                    li {
                        strong { "Submit a hosted paper." }
                        " Upload a PDF or LaTeX source to PreXiv; external URLs are supplemental."
                    }
                    li {
                        strong { "Use an agent token if needed." }
                        " Tokens authorize AI agents to act exactly as your account, never without the token."
                    }
                }
                div.auth-side-links {
                    a href="/permissions" { "See permissions" }
                    a href="/about" { "What belongs here" }
                }
            }
        }
    };
    layout("Register", ctx, body)
}
