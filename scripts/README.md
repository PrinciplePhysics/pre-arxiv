# scripts/

Operational helpers for running PreXiv in production.

## `backup.sh`

Tarballs a hot snapshot of the SQLite DB plus the upload tree into
`backups/prexiv-YYYYMMDD-HHMMSS.tar.gz`, then prunes everything older than the
14 newest archives.

Inside, it uses `sqlite3 ... ".backup ..."` rather than `cp` so the snapshot is
consistent across the main DB file and any pending WAL pages.

It's idempotent: running it more than once a day just produces additional
timestamped archives, and the rotation keeps the newest 14.

### Run it

```sh
./scripts/backup.sh
```

Override the defaults via env if you keep things outside the repo:

| var          | default                        |
|--------------|--------------------------------|
| `DATA_DIR`   | `<repo>/data`                  |
| `UPLOAD_DIR` | `<repo>/public/uploads`        |
| `BACKUP_DIR` | `<repo>/backups`               |
| `KEEP`       | `14`                           |

### Cron

To take a daily snapshot at 04:00 UTC, add a line like this to the host's
crontab (`crontab -e`):

```
0 4 * * * /home/dbai/pre-arxiv/scripts/backup.sh >> /home/dbai/pre-arxiv/backups/backup.log 2>&1
```

(This file documents the entry — install it manually when you're ready; the
backup script does not install itself into cron.)
