//! Shared helpers for handlers: building `PageCtx`, flash messages.

use tower_sessions::Session;

use crate::auth::{csrf_token, MaybeUser};
use crate::state::AppState;
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

pub async fn build_ctx(
    session: &Session,
    MaybeUser(user): MaybeUser,
    current_path: impl Into<String>,
) -> PageCtx {
    let csrf_token = csrf_token(session).await;
    let flash = take_flash(session).await;
    // Persistent across requests until the user verifies. We *don't*
    // remove it here (unlike `flash`) — we want the inline link to
    // remain available if the user reloads or navigates between
    // unverified pages. Cleared by the /verify/{token} handler on
    // successful redeem.
    let pending_verify_token: Option<String> = session
        .get::<String>("pending_verify_token")
        .await
        .ok()
        .flatten();
    let pending_email_change_token: Option<String> = session
        .get::<String>("pending_email_change_token")
        .await
        .ok()
        .flatten();
    PageCtx {
        user,
        csrf_token,
        no_index: false,
        flash,
        current_path: current_path.into(),
        pending_verify_token,
        pending_email_change_token,
        unread_notifications: 0,
        og: None,
        jsonld: None,
        canonical_url: None,
    }
}

/// Variant of build_ctx that also fetches the unread-notification count
/// for the logged-in user, so the topbar bell badge is populated. Use
/// from routes that pass through state and want the bell visible.
pub async fn build_ctx_with_state(
    state: &AppState,
    session: &Session,
    maybe_user: MaybeUser,
    current_path: impl Into<String>,
) -> PageCtx {
    let user_id = maybe_user.0.as_ref().map(|u| u.id);
    let mut ctx = build_ctx(session, maybe_user, current_path).await;
    if let Some(uid) = user_id {
        ctx.unread_notifications = crate::notifications::unread_count(&state.pool, uid)
            .await
            .unwrap_or(0);
    }
    ctx
}
