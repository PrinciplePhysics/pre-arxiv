#!/usr/bin/env bash
# PreXiv deploy — pull the latest commit from origin/main, rebuild the Rust
# binary, restart it. Always snapshots Tier-1 data BEFORE doing anything
# that could damage it; aborts cleanly on any failure step.
#
# Usage:
#   ./scripts/deploy.sh                       # normal deploy
#   ./scripts/deploy.sh skip-build            # skip cargo build (CSS-only)
#
# The script assumes:
#   • The repo is checked out at $HOME/prexiv-deploy/prexiv (override REPO).
#   • The data lives at /var/lib/prexiv/current (override DATA_DIR).
#   • Backups go to /var/lib/prexiv/backups (override BACKUP_ROOT).
#   • The Rust binary writes its PID to $DATA_DIR/../prexiv-rust.pid
#     (override PID_FILE).
#
# What we do, in order — each step aborts if it fails:
#   1. Pre-deploy snapshot of Tier-1 (DB + uploads).
#   2. Integrity-check the snapshot.
#   3. git fetch + reset --hard origin/main.
#   4. cargo build --release.  (skipped if first arg is "skip-build")
#   5. Stop the old binary (SIGTERM, then SIGKILL after 5s).
#   6. Start the new binary detached via setsid.
#   7. Verify it answers 200 on /  within 10 s.
#
# On any failure from step 2 onward, the snapshot from step 1 is the
# recovery point. `scripts/restore.sh` documents how to roll back.

set -euo pipefail

REPO="${REPO:-$HOME/prexiv-deploy/prexiv}"
DATA_DIR="${DATA_DIR:-/var/lib/prexiv/current}"
BACKUP_ROOT="${BACKUP_ROOT:-/var/lib/prexiv/backups}"
PID_FILE="${PID_FILE:-$HOME/prexiv-deploy/prexiv-rust.pid}"
LOG_FILE="${LOG_FILE:-$HOME/prexiv-deploy/prexiv-rust.log}"
START_SCRIPT="${START_SCRIPT:-/tmp/start-rust.sh}"
PORT="${PORT:-3000}"
HEALTHCHECK_URL="${HEALTHCHECK_URL:-http://127.0.0.1:$PORT/}"

SKIP_BUILD=0
if [ "${1:-}" = "skip-build" ]; then SKIP_BUILD=1; fi

abort() {
  echo "deploy: ABORT — $*" >&2
  echo "deploy: data is untouched. last good binary is still running (if it was)." >&2
  exit 1
}

cd "$REPO" || abort "repo dir $REPO not found"

# 1. Snapshot Tier-1 BEFORE touching anything.
echo "[1/7] pre-deploy snapshot…"
REASON="deploy-$(git rev-parse --short HEAD 2>/dev/null || echo unknown)"
DATA_DIR="$DATA_DIR" BACKUP_ROOT="$BACKUP_ROOT" \
  bash "$REPO/scripts/backup.sh" pre-deploy "$REASON" \
  || abort "backup.sh failed (Tier-1 NOT snapshotted; refusing to proceed)"

# 2. Integrity-check the snapshot we just took.
echo "[2/7] verifying snapshot integrity…"
LATEST="$(ls -t "$BACKUP_ROOT/pre-deploy/"*.tar.gz 2>/dev/null | head -1)"
[ -n "$LATEST" ] || abort "could not locate the snapshot just written"
TMP="$(mktemp -d)"
trap 'rm -rf "$TMP"' EXIT
tar -C "$TMP" -xzf "$LATEST" prearxiv.db
INTEG="$(sqlite3 "$TMP/prearxiv.db" 'PRAGMA integrity_check;' | head -1)"
[ "$INTEG" = "ok" ] || abort "snapshot integrity_check returned: $INTEG"
echo "      → $LATEST ok"

# 3. Sync source.
echo "[3/7] git fetch + reset --hard origin/main…"
git fetch origin --quiet
OLD_HEAD="$(git rev-parse HEAD)"
git reset --hard origin/main --quiet
NEW_HEAD="$(git rev-parse HEAD)"
if [ "$OLD_HEAD" = "$NEW_HEAD" ]; then
  echo "      → already at $NEW_HEAD (no changes)"
else
  echo "      → $OLD_HEAD → $NEW_HEAD"
fi

# 4. Build.
if [ "$SKIP_BUILD" -eq 0 ]; then
  echo "[4/7] cargo build --release (this can take a few minutes)…"
  ( cd rust && cargo build --release 2>&1 | tail -3 ) \
    || abort "cargo build --release failed — old binary still running"
else
  echo "[4/7] skipping cargo build (skip-build requested)"
fi

# 5. Stop old binary, gracefully then forcefully.
echo "[5/7] stop old binary…"
if [ -f "$PID_FILE" ]; then
  OLD_PID="$(cat "$PID_FILE")"
  if kill -0 "$OLD_PID" 2>/dev/null; then
    kill -TERM "$OLD_PID" 2>/dev/null || true
    for _ in 1 2 3 4 5; do
      if ! kill -0 "$OLD_PID" 2>/dev/null; then break; fi
      sleep 1
    done
    if kill -0 "$OLD_PID" 2>/dev/null; then
      echo "      → SIGTERM ignored after 5 s, sending SIGKILL"
      kill -KILL "$OLD_PID" 2>/dev/null || true
    fi
    echo "      → old pid $OLD_PID stopped"
  else
    echo "      → pid $OLD_PID was not running"
  fi
fi

# 6. Start new binary detached.
echo "[6/7] start new binary…"
[ -x "$START_SCRIPT" ] || abort "start script $START_SCRIPT missing"
setsid bash "$START_SCRIPT" < /dev/null > /dev/null 2>&1
sleep 2
[ -f "$PID_FILE" ] || abort "no PID file after start"
NEW_PID="$(cat "$PID_FILE")"
kill -0 "$NEW_PID" 2>/dev/null || abort "new binary (pid $NEW_PID) is not running — check $LOG_FILE"
echo "      → new pid $NEW_PID"

# 7. Health check.
echo "[7/7] health check $HEALTHCHECK_URL…"
OK=0
for i in 1 2 3 4 5 6 7 8 9 10; do
  if curl -sS -o /dev/null -w "%{http_code}" "$HEALTHCHECK_URL" 2>/dev/null | grep -q "^200$"; then
    OK=1; break
  fi
  sleep 1
done
if [ "$OK" -ne 1 ]; then
  echo "deploy: health check failed after 10 s — last log lines:" >&2
  tail -20 "$LOG_FILE" >&2 || true
  abort "new binary not responding 200. consider: scripts/restore.sh $LATEST"
fi
echo "      → 200 OK"

echo ""
echo "deploy: OK. running $NEW_HEAD as pid $NEW_PID. pre-deploy snapshot:"
echo "        $LATEST"
