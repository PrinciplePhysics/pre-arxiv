#!/usr/bin/env bash
# Restore PreXiv's Tier-1 data from a snapshot tarball produced by backup.sh.
#
# Usage:
#   ./scripts/restore.sh /var/lib/prexiv/backups/pre-deploy/2026-05-12T00-15-22Z__deploy-abc1234.tar.gz
#
# Safety net: the current contents of $DATA_DIR are NOT deleted. They are
# moved aside to $DATA_DIR.<timestamp>.replaced so a wrong restore is
# still recoverable by renaming that directory back. Operator decides
# when to delete the .replaced directory.

set -euo pipefail

DATA_DIR="${DATA_DIR:-/var/lib/prexiv/current}"
PID_FILE="${PID_FILE:-$HOME/prexiv-deploy/prexiv-rust.pid}"
START_SCRIPT="${START_SCRIPT:-/tmp/start-rust.sh}"

ARCHIVE="${1:-}"
if [ -z "$ARCHIVE" ] || [ ! -f "$ARCHIVE" ]; then
  echo "usage: restore.sh <archive.tar.gz>" >&2
  echo "  available snapshots (last 20 in last 30 days):" >&2
  find /var/lib/prexiv/backups -name "*.tar.gz" -mtime -30 2>/dev/null | sort -r | head -20 >&2
  exit 2
fi

abort() { echo "restore: ABORT — $*" >&2; exit 1; }

# 1. Sanity-check the tarball before going any further.
echo "[1/7] verifying archive…"
tar -tzf "$ARCHIVE" >/dev/null 2>&1 || abort "tarball is unreadable"

# 2. Extract to a staging dir, verify the DB inside is intact.
echo "[2/7] extracting + integrity-checking the snapshot DB…"
STAGE="$(mktemp -d)"
trap 'rm -rf "$STAGE"' EXIT
tar -C "$STAGE" -xzf "$ARCHIVE"
[ -f "$STAGE/prearxiv.db" ] || abort "snapshot has no prearxiv.db"
INTEG="$(sqlite3 "$STAGE/prearxiv.db" 'PRAGMA integrity_check;' | head -1)"
[ "$INTEG" = "ok" ] || abort "snapshot integrity_check returned: $INTEG (refusing to restore corrupt data)"

# 3. Stop the running server (graceful, then forceful).
echo "[3/7] stopping the running binary…"
if [ -f "$PID_FILE" ]; then
  OLD_PID="$(cat "$PID_FILE")"
  if kill -0 "$OLD_PID" 2>/dev/null; then
    kill -TERM "$OLD_PID" 2>/dev/null || true
    for _ in 1 2 3 4 5; do
      kill -0 "$OLD_PID" 2>/dev/null || break
      sleep 1
    done
    kill -KILL "$OLD_PID" 2>/dev/null || true
    echo "      → stopped pid $OLD_PID"
  fi
fi

# 4. Move current data aside (NEVER deleted automatically).
TS="$(date -u +%Y-%m-%dT%H-%M-%SZ)"
REPLACED="${DATA_DIR}.${TS}.replaced"
echo "[4/7] moving current $DATA_DIR → $REPLACED"
[ ! -d "$DATA_DIR" ] || mv "$DATA_DIR" "$REPLACED"

# 5. Install the snapshot in its place.
echo "[5/7] installing snapshot at $DATA_DIR"
mkdir -p "$DATA_DIR"
tar -C "$DATA_DIR" -xzf "$ARCHIVE"

# 6. Final integrity check on the installed copy.
echo "[6/7] integrity-checking the installed DB…"
FINAL_INTEG="$(sqlite3 "$DATA_DIR/prearxiv.db" 'PRAGMA integrity_check;' | head -1)"
[ "$FINAL_INTEG" = "ok" ] || abort "post-restore integrity_check returned: $FINAL_INTEG (the previous data is preserved at $REPLACED)"

# 7. Restart the server.
echo "[7/7] starting the server…"
if [ -x "$START_SCRIPT" ]; then
  setsid bash "$START_SCRIPT" < /dev/null > /dev/null 2>&1
  sleep 2
  NEW_PID="$(cat "$PID_FILE" 2>/dev/null || echo '?')"
  echo "      → new pid $NEW_PID"
else
  echo "      → no start script at $START_SCRIPT — you'll need to start manually"
fi

echo ""
echo "restore: OK. $DATA_DIR now reflects $ARCHIVE."
echo "         previous data preserved at: $REPLACED"
echo "         delete it once you're satisfied: rm -rf $REPLACED"
