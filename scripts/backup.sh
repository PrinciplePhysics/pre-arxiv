#!/usr/bin/env bash
# Hot snapshot of PreXiv Tier-1 data: PostgreSQL database + uploaded artifacts.
#
# Usage: backup.sh [TIER] [REASON]
#   TIER     pre-deploy | hourly | daily | weekly | manual  (default: manual)
#   REASON   free-form annotation embedded in the filename
#
# Output: $BACKUP_ROOT/$TIER/<ISO-timestamp>[__<reason>].tar.gz[.age|.gpg]
#
# The database snapshot is a PostgreSQL custom-format dump. This can be checked
# with `pg_restore --list` and restored with `pg_restore --clean --if-exists`.

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO="${REPO:-$(cd "$SCRIPT_DIR/.." && pwd)}"

if [ -r "$REPO/.env" ]; then
  set -a
  # shellcheck disable=SC1091
  . "$REPO/.env"
  set +a
fi

TIER="${1:-manual}"
REASON="${2:-}"

DATA_DIR="${DATA_DIR:-/var/lib/prexiv/current}"
BACKUP_ROOT="${BACKUP_ROOT:-/var/lib/prexiv/backups}"
UPLOAD_DIR="${UPLOAD_DIR:-$DATA_DIR/uploads}"

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

command -v pg_dump >/dev/null 2>&1 || { echo "backup: pg_dump not in PATH" >&2; exit 1; }
command -v pg_restore >/dev/null 2>&1 || { echo "backup: pg_restore not in PATH" >&2; exit 1; }
[ -n "${DATABASE_URL:-}" ] || { echo "backup: DATABASE_URL is required" >&2; exit 1; }

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

# 1. PostgreSQL custom-format dump.
pg_dump --format=custom --no-owner --no-acl --file "$TMP_DIR/prexiv.dump" "$DATABASE_URL"

# 2. Verify the dump catalog before keeping the archive.
pg_restore --list "$TMP_DIR/prexiv.dump" >/dev/null

# 3. Copy uploaded artifacts into the same archive so DB rows and files can be
#    restored together. Hard links avoid extra reads when the filesystem allows.
if [ -d "$UPLOAD_DIR" ]; then
  cp -al "$UPLOAD_DIR" "$TMP_DIR/uploads" 2>/dev/null || cp -R "$UPLOAD_DIR" "$TMP_DIR/uploads"
fi

# 4. Tar the staging dir, then encrypt. age is preferred for off-machine
#    public-key backup; GPG symmetric encryption is accepted for small
#    single-host deployments. Production refuses plaintext unless explicitly
#    overridden.
ARCHIVE_PLAIN="$DEST_DIR/${NAME}.tar.gz"
PUBKEY_FILE="${PREXIV_BACKUP_RECIPIENT_FILE:-/etc/prexiv/backup.pub}"
if [ -r "$PUBKEY_FILE" ] && command -v age >/dev/null 2>&1; then
  ARCHIVE="${ARCHIVE_PLAIN}.age"
  tar -C "$TMP_DIR" -cz . | age -r "$(tr -d '[:space:]' < "$PUBKEY_FILE")" > "$ARCHIVE"
  echo "backup: ENCRYPTED with age (recipient: $(awk '{print substr($0,1,16)"..."}' "$PUBKEY_FILE"))" >&2
elif [ -n "${PREXIV_BACKUP_PASSPHRASE_FILE:-}" ] \
  && [ -r "$PREXIV_BACKUP_PASSPHRASE_FILE" ] \
  && command -v gpg >/dev/null 2>&1; then
  ARCHIVE="${ARCHIVE_PLAIN}.gpg"
  tar -C "$TMP_DIR" -cz . \
    | gpg --batch --yes --pinentry-mode loopback \
        --passphrase-file "$PREXIV_BACKUP_PASSPHRASE_FILE" \
        --symmetric --cipher-algo AES256 --output "$ARCHIVE"
  echo "backup: ENCRYPTED with gpg symmetric AES256" >&2
else
  if [ "${NODE_ENV:-}" = "production" ] && [ "${PREXIV_ALLOW_PLAINTEXT_BACKUP:-0}" != "1" ]; then
    echo "backup: refusing plaintext backup in production; configure age or PREXIV_BACKUP_PASSPHRASE_FILE" >&2
    exit 4
  fi
  ARCHIVE="$ARCHIVE_PLAIN"
  tar -C "$TMP_DIR" -czf "$ARCHIVE" .
  [ -r "$PUBKEY_FILE" ] || echo "backup: WARNING — no recipient at $PUBKEY_FILE, writing plaintext" >&2
fi
SIZE="$(du -h "$ARCHIVE" | awk '{print $1}')"

# 5. Rotation.
find "$DEST_DIR" -maxdepth 1 -type f \( -name '*.tar.gz' -o -name '*.tar.gz.age' -o -name '*.tar.gz.gpg' \) -printf '%T@ %p\n' \
  | sort -rn \
  | awk -v k="$KEEP" 'NR > k { print $2 }' \
  | xargs -r rm -f --

echo "backup: wrote $ARCHIVE ($SIZE), tier=$TIER, kept=$KEEP newest"
