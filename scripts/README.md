# scripts/

Operational helpers for running PreXiv in production. The production Rust app
expects persistent data outside the git checkout, typically
`DATABASE_URL=postgres://...`,
`DATA_DIR=/var/lib/prexiv/current`, and
`UPLOAD_DIR=/var/lib/prexiv/current/uploads`.

## `backup.sh`

Creates a hot snapshot of the PostgreSQL database plus uploaded artifacts into
`$BACKUP_ROOT/<tier>/<timestamp>.tar.gz.age` or `.tar.gz.gpg`, then rotates
that tier according to the retention policy in the script. Plaintext `.tar.gz`
archives are allowed only outside production unless explicitly overridden.

Inside, it uses `pg_dump --format=custom` and verifies the resulting dump with
`pg_restore --list`. The archive includes the dump and the hosted upload tree.

If `/etc/prexiv/backup.pub` exists and `age` is installed, the archive is
public-key encrypted. Otherwise, set `PREXIV_BACKUP_PASSPHRASE_FILE` to a
mode-0600 file and the script will use GPG symmetric AES256. Local development
can fall back to plaintext archives.

### Run it

```sh
./scripts/backup.sh
```

Override the defaults via env if needed:

| var | default |
|---|---|
| `DATABASE_URL` | required, usually from `.env` |
| `DATA_DIR` | `/var/lib/prexiv/current` |
| `UPLOAD_DIR` | `$DATA_DIR/uploads` |
| `BACKUP_ROOT` | `/var/lib/prexiv/backups` |
| `PREXIV_BACKUP_RECIPIENT_FILE` | `/etc/prexiv/backup.pub` |
| `PREXIV_BACKUP_PASSPHRASE_FILE` | unset |

### Cron

To take a daily snapshot at 04:00 UTC, add a line like this to the host's
crontab (`crontab -e`):

```
0 4 * * * cd /home/prexiv && ./scripts/backup.sh daily cron >> /var/lib/prexiv/backups/backup.log 2>&1
```

(This file documents the entry — install it manually when you're ready; the
backup script does not install itself into cron.)
