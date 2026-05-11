use maud::{html, Markup};

use super::layout::{layout, PageCtx};

pub fn render_login(ctx: &PageCtx, error: Option<&str>, next: Option<&str>) -> Markup {
    let body = html! {
        h1 { "Sign in" }
        @if let Some(e) = error {
            div.error { (e) }
        }
        form method="post" action="/login" {
            input type="hidden" name="csrf_token" value=(ctx.csrf_token);
            @if let Some(n) = next { input type="hidden" name="next" value=(n); }
            label {
                "Username or email"
                input type="text" name="identifier" required autofocus;
            }
            label {
                "Password"
                input type="password" name="password" required;
            }
            button type="submit" { "Sign in" }
        }
        p.minor { a href="/register" { "Need an account? Register." } }
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
        h1 { "Register" }
        @if let Some(e) = error {
            div.error { (e) }
        }
        form method="post" action="/register" {
            input type="hidden" name="csrf_token" value=(ctx.csrf_token);
            label {
                "Username"
                input type="text" name="username" required pattern="[a-zA-Z0-9_-]{3,32}" value=(form.username);
                small.minor { "3–32 chars, letters/digits/_-" }
            }
            label {
                "Email"
                input type="email" name="email" required value=(form.email);
            }
            label {
                "Password"
                input type="password" name="password" required minlength="8";
                small.minor { "8+ chars. We check Have-I-Been-Pwned and reject known-breached passwords." }
            }
            label {
                "Display name (optional)"
                input type="text" name="display_name" value=(form.display_name);
            }
            button type="submit" { "Register" }
        }
        p.minor { a href="/login" { "Already have an account? Sign in." } }
    };
    layout("Register", ctx, body)
}
