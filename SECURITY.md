# Security & data-persistence model

This document describes (a) what user data PreXiv stores, where, and under what
threats; (b) the defenses we have in place; (c) the deploy and backup
procedures that exist specifically to make sure a code update — buggy or
otherwise — cannot damage that data; and (d) the security-audit findings to
date.

It is meant to be readable by a future operator (human or AI agent) coming to
PreXiv cold.

## 1. The data, classified

PreXiv stores three classes of data, in strict descending order of value:

| Class | What | Where | Recoverable if lost? |
|---|---|---|---|
| **Tier 1 — user content** | Manuscripts, authors, abstracts, conductor metadata, auditor statements, comments, votes, follows, flags, accounts (bcrypt hashes, ORCID, display names, bio, affiliation), API tokens (hashed), audit log | `data/prearxiv.db` (SQLite) + `data/uploads/` (uploaded PDFs) | **No.** This is the entire reason PreXiv exists. |
| **Tier 2 — session state** | Active logins, CSRF tokens, flash messages, the one-shot just-minted-token state | `data/sessions.db` (or, on the JS app, `data/sessions.db`) | Yes — losing it just logs everyone out and rotates CSRF tokens. |
| **Tier 3 — derivable** | FTS5 search index, view counts, per-manuscript computed scores, the `data/prearxiv.seed.db` snapshot | Inside `prearxiv.db` | Yes — rebuilt from the source data. |

**The invariant:** Tier 1 data is preserved no matter what happens to the
source tree, the binary, the cache, or the migration system. A `git reset
--hard`, a `cargo clean`, a `kill -9` of the server, an OS package upgrade,
a botched deploy — none of those touch Tier 1 data.

## 2. Where data lives (production layout)

```
/var/lib/prexiv/                       (root: 0755 dbai:dbai)
├── current/                           (the data the running binary uses)
│   ├── prearxiv.db                   ← Tier 1, the SQLite database
│   ├── prearxiv.db-wal               ← SQLite WAL (commits land here first)
│   ├── prearxiv.db-shm               ← SQLite shared-memory
│   ├── sessions.db                   ← Tier 2
│   ├── uploads/                      ← Tier 1, uploaded PDFs
│   └── prearxiv.seed.db              ← Tier 3, optional demo seed snapshot
└── backups/
    ├── pre-deploy/                   ← snapshot before every deploy, kept ≥ 30
    │   └── 2026-05-12T00-15-22.tar.gz
    ├── hourly/                       ← systemd timer (TODO), last 48
    ├── daily/                        ← systemd timer (TODO), last 30
    └── weekly/                       ← systemd timer (TODO), last 12
```

**Critical rules:**

1. **Data never lives inside the source-tree clone.** The repo is at
   `~/prexiv-deploy/prexiv/`; the data is at `/var/lib/prexiv/current/`. The
   binary reads it via `DATA_DIR=/var/lib/prexiv/current` and
   `UPLOAD_DIR=/var/lib/prexiv/current/uploads`. A `rm -rf
   ~/prexiv-deploy/prexiv` does not touch user data.

2. **The deploy script always snapshots Tier 1 before touching the binary.**
   `scripts/deploy.sh` runs `scripts/backup.sh pre-deploy <reason>` as its
   first action; only after the snapshot is on disk does it pull, build,
   restart. If anything fails, the pre-deploy snapshot is still there.

3. **Snapshots use SQLite's atomic backup (`.backup` command), not `cp`.**
   `cp` of a live WAL-mode database can capture a torn read. `.backup` is
   the supported online-backup API and produces a consistent snapshot
   regardless of concurrent writes.

4. **WAL mode is on.** `journal_mode=WAL`, `synchronous=NORMAL`. The WAL
   file is part of the database — both `prearxiv.db` and `prearxiv.db-wal`
   are snapshotted together when we tar the directory.

5. **Migrations are append-only by convention.** No migration drops a
   column or renames a table without an explicit, reviewed exception. New
   columns get added with `ALTER TABLE ... ADD COLUMN ... DEFAULT ...` so
   legacy rows get safe defaults instead of being orphaned.

## 3. The deploy procedure

```sh
ssh victoria
cd ~/prexiv-deploy/prexiv
./scripts/deploy.sh                    # snapshots → pulls → builds → restarts
```

What `scripts/deploy.sh` does, in order:

1. **Snapshot** the live database + uploads to `/var/lib/prexiv/backups/pre-deploy/<timestamp>.tar.gz`, using `sqlite3 ... '.backup'` for the DB (atomic) and tar for the uploads dir.
2. **Sanity-check** the snapshot: `sqlite3 <snapshot> 'PRAGMA integrity_check;'` must print `ok`. If it doesn't, ABORT — do not proceed with the deploy.
3. **Fetch + reset** the source tree to `origin/main`.
4. **Build** the release binary. If `cargo build --release` fails, ABORT (no need to roll back — the running binary hasn't been touched).
5. **Stop** the old binary by sending SIGTERM (`kill $(cat prexiv-rust.pid)`).
6. **Start** the new binary via `setsid bash /tmp/start-rust.sh`. Confirm it answers `curl http://127.0.0.1:3000/` with 200 within 10s. If not, ABORT and tell the operator to manually restart the old binary (the data is still safe in the snapshot).
7. **Smoke-test** a known good URL — `/` and `/api/v1/me` (the latter should 401 without auth).

If any step from 2 onward fails, the pre-deploy snapshot at step 1 is the recovery point: `scripts/restore.sh <snapshot-file>` restores Tier 1 to exactly the state before the deploy started.

## 4. The backup procedure

`scripts/backup.sh [hourly|daily|weekly|pre-deploy] [reason]`:

- Atomic SQLite snapshot via `.backup`, then tarred with the uploads dir.
- Output: `/var/lib/prexiv/backups/<tier>/<ISO-timestamp>.tar.gz`.
- Rotation policy (per directory):
  - `pre-deploy/` — keep 30 most recent
  - `hourly/`     — keep 48 most recent (2 days)
  - `daily/`      — keep 30 most recent (1 month)
  - `weekly/`     — keep 12 most recent (3 months)

A snapshot is ~150 KB today (the data is small). Even keeping all 120 archives, total backup footprint is <20 MB. No reason to be stingy.

## 5. Restore

```sh
scripts/restore.sh /var/lib/prexiv/backups/pre-deploy/2026-05-12T00-15-22.tar.gz
```

What it does:

1. Verifies the tarball is intact (`tar tzf`).
2. Stops the running binary (graceful, then forceful).
3. Moves the current Tier 1 data to `/var/lib/prexiv/current.<timestamp>.replaced` (never deleted — paranoia).
4. Extracts the tarball into `/var/lib/prexiv/current/`.
5. Runs `sqlite3 prearxiv.db 'PRAGMA integrity_check;'` — must print `ok`.
6. Starts the binary back up.
7. Tells you the path of the replaced directory so you can `diff` it if needed.

Step 3 is the safety net: even a wrong restore leaves the previous live data intact under `current.<timestamp>.replaced`. Operator can rename it back if the restore was wrong.

## 6. Off-machine backup (TODO)

Currently snapshots live only on victoria's disk. If victoria's disk dies, we lose everything between the last on-machine snapshot and the disaster. Planned mitigation: a daily `rsync /var/lib/prexiv/backups/` to the operator's Mac (or to an S3 bucket).

## 7. Code-level security audit — findings to date

Audit run 2026-05-12. Grepped for known antipatterns; verified the high-risk surfaces.

### Findings

| ID | Severity | Status |
|---|---|---|
| **S-1.** Open redirect on `/login?next=` | Medium (phishing-aid) | **FIXED** |
| **S-2.** Session cookie missing explicit `SameSite=Lax` | Low | **FIXED** |
| **S-3.** Defense-in-depth: dynamic table name in `routes/votes.rs` | Informational | **FIXED** |
| **S-4.** No rate limiting in the Rust port | Medium (abuse-aid) | Open — planned, tower-governor |
| **S-5.** No off-machine backup | High (durability) | Open — planned, see §6 |

### Verified clean

- **SQL injection.** Every query in the codebase uses `.bind()` with placeholders. Zero `format!("…{user_input}…")` into SQL.
- **CSRF.** Every `POST` handler that takes `Form<…>` verifies `csrf_token` via `verify_csrf(&session, &form.csrf_token)` before mutating state. No exceptions.
- **Path traversal on PDF upload.** `sanitize_filename` strips everything except `[a-zA-Z0-9._-]`, capping at 80 chars; the result is concatenated with a timestamp and 6-digit random nonce, and saved into `UPLOAD_DIR/<sanitized>`. `..` is impossible because `/` and `\` are stripped.
- **XSS in user content.** All free-text fields (title, abstract, comments, conductor notes, auditor statement) flow through `pulldown_cmark::Parser` → `ammonia::Builder::default().clean()`. ammonia's default policy strips `<script>`, event handlers, `javascript:` URLs, and dangerous CSS.
- **XSS in templates.** maud auto-escapes interpolated values. The only `PreEscaped` calls are for ammonia-sanitized markdown output, layout-static SVG, and explicit static HTML in `pages_content/*.html` (which is operator-authored, not user-supplied).
- **Error response leakage.** `AppError::IntoResponse` maps every sqlx/anyhow error to a generic "Internal error" string; the full error is `tracing::error!`-logged server-side. No schema names, no row counts, no stack traces leak to the HTTP response.
- **Password storage.** bcrypt cost 10, byte-identical format with the JS app's bcryptjs hashes. HIBP k-anonymity check on register and change-password.
- **API token storage.** Plaintext is `prexiv_` + 36 base64url chars (27 random bytes of entropy). Stored as SHA-256 hex; the plaintext is shown to the caller exactly once at creation and never persisted.
- **Authorization.** `RequireUser` / `RequireAdmin` extractors gate every private route. `/admin` and `/admin/audit` reject non-admins with 403. The `withdraw` endpoint verifies `viewer.id == submitter_id || viewer.is_admin()` before mutating.

### Caveats

- **Email verification is not enforced for API submission** in the Rust port — the JS app gates `POST /api/v1/manuscripts` on `email_verified`, the Rust port doesn't yet. Documented in the parity table in README.md. The mitigation is bearer-token auth: an unverified account that wants to abuse the API still has to mint a token first, and tokens are revocable by the user or by admin via the `audit_log` / `api_tokens` table.
- **No abuse-heuristic layer yet.** Beyond rate limiting (which is itself missing), there's no shadow-banning, no captcha for known-spam IPs, no submission-frequency dampening. The deployment is small enough today (single-digit users) that this is acceptable; revisit when traffic grows.

## 8. Operator runbook

**Routine deploy:** `ssh victoria && cd ~/prexiv-deploy/prexiv && ./scripts/deploy.sh`. Watch the script's output — it tells you which snapshot it took and which step it's on. If it aborts, the previous binary is still running and the data is still in `current/`.

**Manual snapshot before risky work:** `./scripts/backup.sh pre-deploy "before-Y"`.

**Restore:** `./scripts/restore.sh /var/lib/prexiv/backups/pre-deploy/<timestamp>.tar.gz`. The script keeps your current data under `current.<timestamp>.replaced` so a wrong restore is still recoverable.

**Daily integrity check:** `sqlite3 /var/lib/prexiv/current/prearxiv.db 'PRAGMA integrity_check;'` should print exactly `ok`. Anything else means file corruption or a SQLite bug — restore from the most recent good backup and investigate.

**See what data lives there right now:** `du -sh /var/lib/prexiv/current/*` and `sqlite3 /var/lib/prexiv/current/prearxiv.db 'SELECT COUNT(*) FROM users; SELECT COUNT(*) FROM manuscripts;'`.
