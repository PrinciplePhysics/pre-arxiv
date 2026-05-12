-- In-product notifications.
--
-- Each row is a single event addressed to one recipient. Triggers are
-- fired by the comment + follow handlers at insert time; mark-read +
-- mark-all-read are POST handlers under /me/notifications.
--
-- Kinds:
--   comment_on_my_manuscript  — someone commented on a manuscript you submitted
--   reply_to_my_comment       — someone replied to a comment you wrote
--   followed                  — someone started following you
--
-- More kinds (vote milestones, revision of a followed manuscript, mentions)
-- can land later without schema change — `kind` is free TEXT.

CREATE TABLE notifications (
    id            INTEGER PRIMARY KEY AUTOINCREMENT,
    recipient_id  INTEGER NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    actor_id      INTEGER          REFERENCES users(id) ON DELETE SET NULL,
    kind          TEXT    NOT NULL,
    target_type   TEXT,            -- 'manuscript' | 'comment' | 'user' | NULL
    target_id     INTEGER,
    detail        TEXT,            -- short snippet for the row body
    read_at       DATETIME,        -- NULL = unread
    created_at    DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP
);

CREATE INDEX idx_notif_recipient_unread  ON notifications(recipient_id, read_at);
CREATE INDEX idx_notif_recipient_created ON notifications(recipient_id, created_at DESC);
