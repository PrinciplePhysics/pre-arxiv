#!/usr/bin/env bash
# Off-machine snapshot push (S-5).
#
# Mirrors PreXiv's already-encrypted backup files from victoria to a
# remote target. Run AFTER `backup.sh` so the freshest snapshot is
# what travels off the box. Idempotent — re-runs only ship what's new.
#
# Usage:
#   offmachine-backup.sh                       # use $PREXIV_OFFMACHINE_DEST
#   offmachine-backup.sh user@host:/path/      # explicit override
#
# Env vars:
#   PREXIV_OFFMACHINE_DEST  rsync-style destination, e.g.
#                             "dbai-mac:~/PrexivBackups/"
#                             "user@nas.local:/volume1/prexiv/"
#   BACKUP_ROOT             local backups dir (default /var/lib/prexiv/backups)
#   PREXIV_OFFMACHINE_SSH   override ssh program (e.g. "ssh -i /path/key")
#
# Design notes:
#   * No `--delete`. Retention is enforced on the source by `backup.sh`;
#     the destination is an *archive*, not a mirror. Keeping deleted
#     files around defends against "ransomware wipes the source then
#     we sync the empty state offsite".
#   * Only `.tar.gz.age` files are shipped — plaintext tarballs (local
#     dev fallback in §6a "Fallback for local dev") never leave the
#     box. If you've set PREXIV_BACKUP_RECIPIENT_FILE but it points at
#     a missing key, backup.sh would have already produced plaintext
#     and this script would skip everything: that's intentional.
#   * Exit 0 on partial transfer so a transient SSH/network failure
#     does not break the next-stage cron. Exit non-zero only when the
#     destination is unset or unreachable in a way we can't ignore.

set -euo pipefail

DEST="${1:-${PREXIV_OFFMACHINE_DEST:-}}"
BACKUP_ROOT="${BACKUP_ROOT:-/var/lib/prexiv/backups}"
SSH_PROG="${PREXIV_OFFMACHINE_SSH:-ssh -o ConnectTimeout=20 -o BatchMode=yes}"

if [[ -z "$DEST" ]]; then
  echo "offmachine-backup: PREXIV_OFFMACHINE_DEST not set and no destination" \
       "passed on the command line. Set it in /etc/default/prexiv or" \
       "pass user@host:/path/ as argv[1]." >&2
  exit 2
fi

if [[ ! -d "$BACKUP_ROOT" ]]; then
  echo "offmachine-backup: BACKUP_ROOT '$BACKUP_ROOT' missing — nothing to do" >&2
  exit 0
fi

# Build a relative file list (encrypted snapshots only). Using
# --files-from keeps the directory layout on the destination identical
# to what victoria has under $BACKUP_ROOT, so restore.sh can be pointed
# at the off-host copy with no path rewrites.
FILELIST="$(mktemp)"
trap 'rm -f "$FILELIST"' EXIT
cd "$BACKUP_ROOT"
# -print0 / xargs -0 keep us safe on the unlikely paths that contain
# spaces or newlines; tier names use only [a-z-]+ so this is belt-and-
# suspenders.
find . -type f -name '*.tar.gz.age' -print | sed 's|^\./||' > "$FILELIST"

if [[ ! -s "$FILELIST" ]]; then
  echo "offmachine-backup: no encrypted snapshots in $BACKUP_ROOT — nothing to push"
  exit 0
fi

COUNT="$(wc -l < "$FILELIST" | tr -d ' ')"
echo "offmachine-backup: pushing $COUNT encrypted snapshot(s) → $DEST"

# -a archive, -v verbose progress, -z compress on the wire (gzip-of-
# already-encrypted-data still saves bytes because age leaves headers).
# --ignore-existing means we don't re-upload files already on the
# destination — important when the destination is a slow link.
# --partial keeps half-uploaded files so the next run can resume.
# -e "$SSH_PROG" lets the operator override the ssh invocation.
rsync_status=0
rsync -avz \
      --ignore-existing \
      --partial \
      --files-from="$FILELIST" \
      -e "$SSH_PROG" \
      "$BACKUP_ROOT"/ "$DEST" \
  || rsync_status=$?

case "$rsync_status" in
  0)   echo "offmachine-backup: ok" ;;
  23|24) echo "offmachine-backup: partial transfer ($rsync_status) — will retry next run" ;;
  *)   echo "offmachine-backup: rsync exited $rsync_status" >&2 ; exit "$rsync_status" ;;
esac
