#!/usr/bin/env bash
# PreXiv backup — bundle the SQLite DB plus the upload tree into a single
# timestamped tar.gz under backups/, and keep the most-recent 14 archives.
#
# Idempotent: safe to run more than once a day. Each invocation creates a
# new tarball with its own timestamp; the rotation step just deletes
# everything older than the 14-th newest archive in backups/.
#
# Usage: scripts/backup.sh
set -euo pipefail

# Resolve repo root from this script's location (works regardless of cwd).
HERE="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ROOT="$(cd "$HERE/.." && pwd)"

DATA_DIR="${DATA_DIR:-$ROOT/data}"
UPLOAD_DIR="${UPLOAD_DIR:-$ROOT/public/uploads}"
BACKUP_DIR="${BACKUP_DIR:-$ROOT/backups}"
KEEP="${KEEP:-14}"

mkdir -p "$BACKUP_DIR"

DB_PATH="$DATA_DIR/prearxiv.db"
if [ ! -f "$DB_PATH" ]; then
  echo "backup: DB file not found at $DB_PATH" >&2
  exit 1
fi
if ! command -v sqlite3 >/dev/null 2>&1; then
  echo "backup: sqlite3 binary not found in PATH" >&2
  exit 1
fi

STAMP="$(date +%Y%m%d-%H%M%S)"
ARCHIVE="$BACKUP_DIR/prexiv-$STAMP.tar.gz"

# Stage everything in a per-run tmpdir, so an interrupted run leaves no
# half-built artefacts behind.
TMP="$(mktemp -d)"
trap 'rm -rf "$TMP"' EXIT

# Consistent hot-snapshot of the live DB. Using sqlite3's .backup is required
# because better-sqlite3 may have an open WAL — `cp` could capture an
# inconsistent point-in-time view of the main file vs the WAL.
sqlite3 "$DB_PATH" ".backup '$TMP/prearxiv.db'"

# Stage the uploads alongside (may not exist on a brand-new install).
if [ -d "$UPLOAD_DIR" ]; then
  mkdir -p "$TMP/uploads"
  # cp -a preserves perms/timestamps. Trailing /. copies contents only.
  if [ -n "$(ls -A "$UPLOAD_DIR" 2>/dev/null)" ]; then
    cp -a "$UPLOAD_DIR"/. "$TMP/uploads/"
  fi
else
  mkdir -p "$TMP/uploads"
fi

# Build the tarball atomically: write to a .partial first, rename when done.
tar -C "$TMP" -czf "$ARCHIVE.partial" prearxiv.db uploads
mv "$ARCHIVE.partial" "$ARCHIVE"

echo "backup: wrote $ARCHIVE"

# Rotate: keep only the newest $KEEP archives matching prexiv-*.tar.gz.
# `ls -1t` orders by mtime descending; tail -n +N skips the first N-1.
cd "$BACKUP_DIR"
ls -1t prexiv-*.tar.gz 2>/dev/null | tail -n +"$((KEEP + 1))" | while read -r old; do
  rm -f -- "$old"
  echo "backup: pruned $old"
done

exit 0
