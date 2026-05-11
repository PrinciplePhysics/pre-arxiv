use axum::response::Html;
use tower_sessions::Session;

use crate::auth::MaybeUser;
use crate::error::AppResult;
use crate::helpers::build_ctx;
use crate::templates::{self, pages};

macro_rules! static_page {
    ($name:ident, $title:expr, $path:expr, $content:expr) => {
        pub async fn $name(session: Session, maybe_user: MaybeUser) -> AppResult<Html<String>> {
            let ctx = build_ctx(&session, maybe_user, $path).await;
            Ok(Html(pages::render(&ctx, $title, $content).into_string()))
        }
    };
}

static_page!(about,      "About",       "/about",      templates::pages::ABOUT);
static_page!(guidelines, "Guidelines",  "/guidelines", templates::pages::GUIDELINES);
static_page!(tos,        "Terms",       "/tos",        templates::pages::TOS);
static_page!(privacy,    "Privacy",     "/privacy",    templates::pages::PRIVACY);
static_page!(dmca,       "DMCA",        "/dmca",       templates::pages::DMCA);
static_page!(policies,   "Policies",    "/policies",   templates::pages::POLICIES);
