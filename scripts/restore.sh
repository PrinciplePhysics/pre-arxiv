#!/usr/bin/env bash
# Restore PreXiv Tier-1 data from a snapshot produced by backup.sh.
#
# Usage:
#   ./scripts/restore.sh /var/lib/prexiv/backups/pre-deploy/2026-05-12T00-15-22Z__deploy-abc1234.tar.gz.age
#   ./scripts/restore.sh /var/lib/prexiv/backups/pre-deploy/2026-05-12T00-15-22Z__deploy-abc1234.tar.gz.gpg
#
# Safety net: before applying the requested snapshot, this script writes a
# rollback PostgreSQL dump and copies current uploads into
# $DATA_DIR.<timestamp>.replaced. Operator decides when to delete it.

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO="${REPO:-$(cd "$SCRIPT_DIR/.." && pwd)}"

if [ -r "$REPO/.env" ]; then
  set -a
  # shellcheck disable=SC1091
  . "$REPO/.env"
  set +a
fi

PID_FILE="${PID_FILE:-$HOME/prexiv-deploy/prexiv-rust.pid}"
DATA_DIR="${DATA_DIR:-$REPO/data}"
UPLOAD_DIR="${UPLOAD_DIR:-$DATA_DIR/uploads}"
START_SCRIPT="${START_SCRIPT:-$REPO/scripts/start-rust.sh}"

ARCHIVE="${1:-}"
if [ -z "$ARCHIVE" ] || [ ! -f "$ARCHIVE" ]; then
  echo "usage: restore.sh <archive.tar.gz[.age|.gpg]>" >&2
  echo "  available snapshots (last 20 in last 30 days):" >&2
  find /var/lib/prexiv/backups \( -name "*.tar.gz" -o -name "*.tar.gz.age" -o -name "*.tar.gz.gpg" \) -mtime -30 2>/dev/null | sort -r | head -20 >&2
  exit 2
fi

abort() { echo "restore: ABORT — $*" >&2; exit 1; }

command -v pg_dump >/dev/null 2>&1 || abort "pg_dump not in PATH"
command -v pg_restore >/dev/null 2>&1 || abort "pg_restore not in PATH"
command -v psql >/dev/null 2>&1 || abort "psql not in PATH"
[ -n "${DATABASE_URL:-}" ] || abort "DATABASE_URL is required"

echo "[1/7] extracting + verifying archive…"
STAGE="$(mktemp -d)"
trap 'rm -rf "$STAGE"' EXIT
if [[ "$ARCHIVE" == *.age ]]; then
  KEYFILE="${BACKUP_KEY:-/etc/prexiv/backup-key.txt}"
  [ -r "$KEYFILE" ] || abort "encrypted archive but private key $KEYFILE is not readable"
  age -d -i "$KEYFILE" "$ARCHIVE" | tar -C "$STAGE" -xz
elif [[ "$ARCHIVE" == *.gpg ]]; then
  KEYFILE="${PREXIV_BACKUP_PASSPHRASE_FILE:-}"
  [ -n "$KEYFILE" ] && [ -r "$KEYFILE" ] || abort "gpg archive but PREXIV_BACKUP_PASSPHRASE_FILE is not readable"
  gpg --batch --yes --pinentry-mode loopback --passphrase-file "$KEYFILE" --decrypt "$ARCHIVE" | tar -C "$STAGE" -xz
else
  tar -C "$STAGE" -xzf "$ARCHIVE"
fi
[ -f "$STAGE/prexiv.dump" ] || abort "snapshot has no prexiv.dump"
pg_restore --list "$STAGE/prexiv.dump" >/dev/null || abort "snapshot dump cannot be read"

echo "[2/7] stopping the running binary…"
if [ -f "$PID_FILE" ]; then
  OLD_PID="$(cat "$PID_FILE")"
  if kill -0 "$OLD_PID" 2>/dev/null; then
    kill -TERM "$OLD_PID" 2>/dev/null || true
    for _ in 1 2 3 4 5; do
      kill -0 "$OLD_PID" 2>/dev/null || break
      sleep 1
    done
    kill -KILL "$OLD_PID" 2>/dev/null || true
    echo "      -> stopped pid $OLD_PID"
  fi
fi

TS="$(date -u +%Y-%m-%dT%H-%M-%SZ)"
REPLACED="${DATA_DIR}.${TS}.replaced"
echo "[3/7] writing rollback copy at $REPLACED"
mkdir -p "$REPLACED"
pg_dump --format=custom --no-owner --no-acl --file "$REPLACED/prexiv.dump" "$DATABASE_URL"
pg_restore --list "$REPLACED/prexiv.dump" >/dev/null
if [ -d "$UPLOAD_DIR" ]; then
  cp -a "$UPLOAD_DIR" "$REPLACED/uploads"
fi

echo "[4/7] restoring PostgreSQL dump…"
pg_restore --clean --if-exists --no-owner --no-acl --dbname "$DATABASE_URL" "$STAGE/prexiv.dump"

echo "[5/7] restoring uploaded artifacts…"
mkdir -p "$DATA_DIR"
rm -rf "$UPLOAD_DIR"
if [ -d "$STAGE/uploads" ]; then
  cp -a "$STAGE/uploads" "$UPLOAD_DIR"
else
  mkdir -p "$UPLOAD_DIR"
fi

echo "[6/7] checking restored database…"
psql "$DATABASE_URL" -v ON_ERROR_STOP=1 -tAc "SELECT COUNT(*) FROM _sqlx_migrations;" >/dev/null

echo "[7/7] starting the server…"
if [ -x "$START_SCRIPT" ]; then
  setsid bash "$START_SCRIPT" < /dev/null > /dev/null 2>&1
  sleep 2
  NEW_PID="$(cat "$PID_FILE" 2>/dev/null || echo '?')"
  echo "      -> new pid $NEW_PID"
else
  echo "      -> no start script at $START_SCRIPT; start manually"
fi

echo ""
echo "restore: OK. PostgreSQL and uploads now reflect $ARCHIVE."
echo "         rollback copy preserved at: $REPLACED"
