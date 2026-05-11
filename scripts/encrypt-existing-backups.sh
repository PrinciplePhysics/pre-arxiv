#!/usr/bin/env bash
# One-shot: convert every plaintext .tar.gz under /var/lib/prexiv/backups/
# into an age-encrypted .tar.gz.age, then delete the plaintext.
#
# Intended to be run once, after the keypair at /etc/prexiv/backup-key.txt
# has been generated and after backup.sh has been updated to emit .age
# archives. Safe to re-run — skips files that already have a .age twin.

set -euo pipefail

BACKUP_ROOT="${BACKUP_ROOT:-/var/lib/prexiv/backups}"
PUBKEY_FILE="${PREXIV_BACKUP_RECIPIENT_FILE:-/etc/prexiv/backup.pub}"

command -v age >/dev/null 2>&1 || { echo "encrypt-existing-backups: age not in PATH" >&2; exit 1; }
[ -r "$PUBKEY_FILE" ] || { echo "encrypt-existing-backups: no recipient at $PUBKEY_FILE" >&2; exit 1; }

RECIPIENT="$(tr -d '[:space:]' < "$PUBKEY_FILE")"
CONVERTED=0
SKIPPED=0
FAILED=0

while IFS= read -r plain; do
  enc="${plain}.age"
  if [ -f "$enc" ]; then
    SKIPPED=$((SKIPPED + 1))
    continue
  fi
  # age 1.x doesn't accept `age -r KEY INPUT -o OUTPUT` — the input
  # must be the only non-flag positional, OR fed on stdin. Use the
  # stream form for consistency with backup.sh.
  if age -r "$RECIPIENT" -o "$enc" < "$plain"; then
    rm -f -- "$plain"
    CONVERTED=$((CONVERTED + 1))
    echo "  + $enc"
  else
    FAILED=$((FAILED + 1))
    rm -f -- "$enc" 2>/dev/null || true
    echo "  ! $plain (encryption failed; plaintext kept)" >&2
  fi
done < <(find "$BACKUP_ROOT" -type f -name '*.tar.gz' ! -name '*.tar.gz.age')

echo ""
echo "encrypt-existing-backups: converted=$CONVERTED  skipped=$SKIPPED  failed=$FAILED"
