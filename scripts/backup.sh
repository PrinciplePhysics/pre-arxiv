#!/usr/bin/env bash
# Atomic snapshot of PreXiv's Tier-1 data (SQLite DB + uploads dir).
#
# Usage: backup.sh [TIER] [REASON]
#   TIER     pre-deploy | hourly | daily | weekly | manual  (default: manual)
#   REASON   free-form annotation embedded in the filename
#
# Output: $BACKUP_ROOT/$TIER/<ISO-timestamp>[__<reason>].tar.gz
#
# Why .backup instead of cp: a WAL-mode SQLite database is consistent only
# when the WAL is checkpointed at the byte the read happens. `cp` can copy
# a torn read. `sqlite3 ... '.backup ...'` is the supported online-backup
# API, atomic w.r.t. concurrent writers.
#
# Why tar around the .backup output: we want the uploads directory captured
# in the same archive so a restore returns the system to a consistent
# (DB + files) state.

set -euo pipefail

TIER="${1:-manual}"
REASON="${2:-}"

DATA_DIR="${DATA_DIR:-/var/lib/prexiv/current}"
BACKUP_ROOT="${BACKUP_ROOT:-/var/lib/prexiv/backups}"
DB_PATH="$DATA_DIR/prearxiv.db"
UPLOAD_DIR="$DATA_DIR/uploads"

case "$TIER" in
  pre-deploy|hourly|daily|weekly|manual) ;;
  *) echo "backup: unknown tier '$TIER'; valid: pre-deploy|hourly|daily|weekly|manual" >&2; exit 2 ;;
esac

case "$TIER" in
  pre-deploy) KEEP=30 ;;
  hourly)     KEEP=48 ;;
  daily)      KEEP=30 ;;
  weekly)     KEEP=12 ;;
  manual)     KEEP=20 ;;
esac

command -v sqlite3 >/dev/null 2>&1 || { echo "backup: sqlite3 not in PATH" >&2; exit 1; }
[ -f "$DB_PATH" ] || { echo "backup: DB not found at $DB_PATH" >&2; exit 1; }

DEST_DIR="$BACKUP_ROOT/$TIER"
mkdir -p "$DEST_DIR"

TS="$(date -u +%Y-%m-%dT%H-%M-%SZ)"
SUFFIX=""
if [ -n "$REASON" ]; then
  CLEAN_REASON="$(printf '%s' "$REASON" | tr -c '[:alnum:]_-' '_' | cut -c1-40)"
  SUFFIX="__${CLEAN_REASON}"
fi
NAME="${TS}${SUFFIX}"
TMP_DIR="$(mktemp -d)"
trap 'rm -rf "$TMP_DIR"' EXIT

# 1. Atomic SQLite snapshot.
sqlite3 "$DB_PATH" ".backup '$TMP_DIR/prearxiv.db'"

# 2. Integrity-check the snapshot — anything other than 'ok' means we
#    just captured a corrupt DB. Refuse to keep it; exit non-zero so
#    deploy.sh ABORTs the deploy.
INTEG="$(sqlite3 "$TMP_DIR/prearxiv.db" 'PRAGMA integrity_check;' | head -1)"
if [ "$INTEG" != "ok" ]; then
  echo "backup: integrity_check FAILED on snapshot ($INTEG); refusing to keep" >&2
  exit 3
fi

# 3. Optionally snapshot the sessions DB (Tier-2). Cheap; lets a restore
#    keep users logged-in across the restore boundary.
if [ -f "$DATA_DIR/sessions.db" ]; then
  sqlite3 "$DATA_DIR/sessions.db" ".backup '$TMP_DIR/sessions.db'" || true
fi

# 4. Hard-link the uploads dir into the staging dir (cheap on the same fs;
#    avoids reading every PDF byte just to tar it back up).
if [ -d "$UPLOAD_DIR" ]; then
  cp -al "$UPLOAD_DIR" "$TMP_DIR/uploads" 2>/dev/null || cp -r "$UPLOAD_DIR" "$TMP_DIR/uploads"
fi

# 5. Tar the staging dir, then encrypt with age if a recipient is
#    available. If /etc/prexiv/backup.pub doesn't exist (e.g., a local
#    dev box where the operator hasn't set up encryption), fall back
#    to plaintext .tar.gz so dev still works — restore.sh handles
#    both extensions.
ARCHIVE_PLAIN="$DEST_DIR/${NAME}.tar.gz"
PUBKEY_FILE="${PREXIV_BACKUP_RECIPIENT_FILE:-/etc/prexiv/backup.pub}"
if [ -r "$PUBKEY_FILE" ] && command -v age >/dev/null 2>&1; then
  ARCHIVE="${ARCHIVE_PLAIN}.age"
  tar -C "$TMP_DIR" -cz . | age -r "$(tr -d '[:space:]' < "$PUBKEY_FILE")" > "$ARCHIVE"
  echo "backup: ENCRYPTED with age (recipient: $(awk '{print substr($0,1,16)"..."}' "$PUBKEY_FILE"))" >&2
else
  ARCHIVE="$ARCHIVE_PLAIN"
  tar -C "$TMP_DIR" -czf "$ARCHIVE" .
  [ -r "$PUBKEY_FILE" ] || echo "backup: WARNING — no recipient at $PUBKEY_FILE, writing plaintext" >&2
fi
SIZE="$(du -h "$ARCHIVE" | awk '{print $1}')"

# 6. Rotation: keep the $KEEP newest archives in this tier; delete
#    the rest. Use `find` rather than `ls *.glob *.glob` because the
#    shell errors out under `set -e` if either glob matches nothing
#    (which is now common — after the encrypt-existing migration,
#    most tiers have only .tar.gz.age and no .tar.gz).
find "$DEST_DIR" -maxdepth 1 -type f \( -name '*.tar.gz' -o -name '*.tar.gz.age' \) -printf '%T@ %p\n' \
  | sort -rn \
  | awk -v k="$KEEP" 'NR > k { print $2 }' \
  | xargs -r rm -f --

echo "backup: wrote $ARCHIVE ($SIZE), tier=$TIER, kept=$KEEP newest"
