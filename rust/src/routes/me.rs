//! /me/* and /feed — stubs that render a "coming soon" notice but require
//! login so the topnav links don't 404. Real implementations will be
//! ported from the JS app in a later milestone.

use axum::response::Html;
use tower_sessions::Session;
use maud::html;

use crate::auth::{MaybeUser, RequireUser};
use crate::error::AppResult;
use crate::helpers::build_ctx;
use crate::templates::{layout::layout, PageCtx};

async fn stub(
    session: Session,
    maybe_user: MaybeUser,
    path: &str,
    title: &str,
    note: &str,
) -> AppResult<Html<String>> {
    let mut ctx = build_ctx(&session, maybe_user, path).await;
    ctx.no_index = true;
    let body = html! {
        div.page-header {
            h1 { (title) }
            p.muted { (note) }
        }
        p {
            "Available in the Node.js app while the Rust port reaches parity. "
            a href="http://localhost:3000" rel="noopener" { "Open the Node app →" }
        }
    };
    let markup = layout(title, &ctx, body);
    Ok(Html(markup.into_string()))
}

pub async fn me_edit(
    session: Session, maybe_user: MaybeUser, RequireUser(_): RequireUser,
) -> AppResult<Html<String>> {
    stub(session, maybe_user, "/me/edit", "Edit profile",
         "Profile editing is being ported. For now, edit your profile through the Node.js app.").await
}

pub async fn me_tokens(
    session: Session, maybe_user: MaybeUser, RequireUser(_): RequireUser,
) -> AppResult<Html<String>> {
    stub(session, maybe_user, "/me/tokens", "API tokens",
         "API token management is being ported. For now, manage tokens through the Node.js app.").await
}

pub async fn feed(
    session: Session, maybe_user: MaybeUser, RequireUser(_): RequireUser,
) -> AppResult<Html<String>> {
    stub(session, maybe_user, "/feed", "Your feed",
         "The personalised feed is being ported. For now, browse the home page.").await
}

#[allow(dead_code)]
fn _ctx_unused() -> Option<PageCtx> { None }
