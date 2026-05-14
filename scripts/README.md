# scripts/

Operational helpers for running PreXiv in production. The production Rust app
expects persistent data outside the git checkout, typically
`DATA_DIR=/var/lib/prexiv/current` and
`UPLOAD_DIR=/var/lib/prexiv/current/uploads`.

## `backup.sh`

Creates a hot snapshot of the SQLite DB plus uploaded artifacts into
`$BACKUP_ROOT/<tier>/<timestamp>.tar.gz` or `.tar.gz.age`, then rotates that
tier according to the retention policy in the script.

Inside, it uses `sqlite3 ... ".backup ..."` rather than `cp` so the snapshot is
consistent across the main DB file and any pending WAL pages.

If `/etc/prexiv/backup.pub` exists and `age` is installed, the archive is
encrypted before it is written to its final path. Local development can fall
back to plaintext archives.

### Run it

```sh
./scripts/backup.sh
```

Override the defaults via env if needed:

| var | default |
|---|---|
| `DATA_DIR` | `/var/lib/prexiv/current` |
| `BACKUP_ROOT` | `/var/lib/prexiv/backups` |
| `PREXIV_BACKUP_RECIPIENT_FILE` | `/etc/prexiv/backup.pub` |

### Cron

To take a daily snapshot at 04:00 UTC, add a line like this to the host's
crontab (`crontab -e`):

```
0 4 * * * cd /home/dbai/prexiv-deploy/prexiv && DATA_DIR=/var/lib/prexiv/current ./scripts/backup.sh daily cron >> /var/lib/prexiv/backups/backup.log 2>&1
```

(This file documents the entry — install it manually when you're ready; the
backup script does not install itself into cron.)
