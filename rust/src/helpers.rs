//! Shared helpers for handlers: building `PageCtx`, flash messages.

use tower_sessions::Session;

use crate::auth::{csrf_token, MaybeUser};
use crate::templates::PageCtx;

const SESSION_FLASH_KEY: &str = "flash";

/// Read a one-shot flash message from the session — also clears it.
pub async fn take_flash(session: &Session) -> Option<String> {
    let msg: Option<String> = session.get(SESSION_FLASH_KEY).await.ok().flatten();
    if msg.is_some() {
        let _ = session.remove::<String>(SESSION_FLASH_KEY).await;
    }
    msg
}

pub async fn set_flash(session: &Session, msg: impl Into<String>) {
    let _ = session.insert(SESSION_FLASH_KEY, msg.into()).await;
}

pub async fn build_ctx(session: &Session, MaybeUser(user): MaybeUser) -> PageCtx {
    let csrf_token = csrf_token(session).await;
    let flash = take_flash(session).await;
    PageCtx { user, csrf_token, no_index: false, flash }
}
