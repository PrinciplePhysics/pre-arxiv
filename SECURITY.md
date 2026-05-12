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

## 6. Off-machine backup

Encrypted snapshots are rsynced off victoria to a second host on every backup run, defending against single-disk failure and ransomware of the live host. Because the snapshots are age-encrypted (see §6a), the off-machine target is allowed to be lower-trust storage (S3 with default IAM, the operator's NAS, another VPS) — only the holder of the *private* age key can recover plaintext.

**Script**: `scripts/offmachine-backup.sh` — invoked from cron after `backup.sh` completes. It rsyncs `/var/lib/prexiv/backups/*.tar.gz.age` (no `--delete` — retention is managed on the source, not by the destination) to `$PREXIV_OFFMACHINE_DEST` (e.g. `mac:~/PrexivBackups/`). Exits 0 even on partial transfer so a transient network blip doesn't take down the backup chain; the next run picks up missing files.

**Verification cadence**: the operator MUST run a real `restore.sh` from an off-machine snapshot at least once per quarter. A backup you have never restored is not a backup.

## 6a. Encryption at rest

Personal data on PreXiv falls into four categories. Treating them all the same way would be wrong — the right protection depends on whether recovery is *possible* in the threat model, or *prevented by design*.

| Category | What's in it | Protection | Why this is right |
|---|---|---|---|
| **Irrecoverable by design** | `users.password_hash` (bcrypt cost 10); `api_tokens.token_hash` (SHA-256 hex) | The plaintext is never persisted. Bcrypt hashes are intentionally slow one-way functions; SHA-256 of a 27-byte random token has no practical preimage. | **Stronger than encryption.** Encryption implies a key holder can recover the plaintext; coercing the operator or compromising the key gets that plaintext back. With irrecoverable hashes, *no one* can derive the original — not the operator, not a court order, not a database leaker. The user is the only one who ever knew the plaintext. |
| **Intentionally public** | `users.username`, `display_name`, `affiliation`, `bio`, `orcid`; manuscript title/abstract/authors/conductor; comments; votes | Plaintext on disk and on the wire. | Public by user choice. The user filled in these fields *to be seen*; encrypting them would prevent the only legitimate use. |
| **Encrypted at rest** | `users.email` (login + verification) | Column-level **AES-256-GCM** with a server-side master key in `PREXIV_DATA_KEY` (32 random bytes, hex- or base64-encoded), plus a deterministic 32-byte HMAC-SHA256 *blind index* `email_hash` so we can `WHERE email_hash = ?` without decrypting every row. Implementation: `rust/src/crypto.rs`; schema in migration `0012_email_at_rest_encryption.sql`; backfill on app startup is idempotent. | A DB dump alone yields ciphertext + an opaque hash. To reverse to a known email address an attacker needs the master key, which lives only in the systemd-managed env file (mode 0600). Future-extendable to `totp_secret` and webhook signing secrets with the same primitives. |
| **Pending encryption** | `users.totp_secret` (2FA); `webhooks.secret` (HMAC signing key) | Plaintext today. | Same threat profile as email; same `crypto.rs` primitives apply. Tracked as the next encryption pass after `users.email` proves itself in production. |
| **Backup tarballs leaving the box** | The tar.gz that bundles the DB + sessions + uploads, snapshotted before every deploy and on cron schedules | **age-encrypted** via X25519 to a recipient public key. The plaintext never touches disk; backup.sh pipes `tar -cz` straight through `age -r <pub>` into the final `.tar.gz.age`. | Backups are the most-portable copies of all user data: they get rsynced off-machine, sit in cron-driven archive directories, and may end up in places the live DB never goes. Encrypting *them* specifically defends the threat model where the box itself stays trusted but a backup copy gets out. |

### Key management for `PREXIV_DATA_KEY` (column-level encryption)

The master key for `users.email` (and future encrypted columns) is loaded once at app startup from `PREXIV_DATA_KEY`.

- **Format**: 32 bytes encoded as either 64 hex chars or 44 base64 chars (standard alphabet, padded). Anything else exits non-zero before the server binds the port.

- **Generation** (do this exactly once, before first deploy):

  ```bash
  openssl rand -hex 32                    # produces a 64-char hex string
  # or
  head -c 32 /dev/urandom | base64        # produces a 44-char base64 string
  ```

- **Where it lives on victoria**: `/etc/default/prexiv` (mode 0600, owned `root:dbai`) as the line `PREXIV_DATA_KEY=…`. systemd loads `/etc/default/prexiv` via `EnvironmentFile=` in the unit. Never check this value into git. Never `echo` it. It does not appear in `tracing` output (the crypto module hands it directly to AES + HMAC and never logs the bytes).

- **Loss consequences**: an irretrievable loss of `PREXIV_DATA_KEY` bricks every encrypted column. The `email_enc` ciphertext becomes undecryptable, login-by-email stops working (the blind index is also keyed by the master key, so an attacker who steals only the DB can't link rows to known emails, but neither can a legit operator after key loss). Usernames + password hashes survive — users can still log in with their username and the password they remember. Plan accordingly: keep an off-line copy of this key the same way you keep the age backup private key.

- **Rotation** (TODO; not yet implemented): on rotation, the server would dual-key for one deploy cycle (accept both old and new key for decryption, write only with the new key), re-encrypt every row using a startup pass, then drop the old key. Until that lands, treat `PREXIV_DATA_KEY` as set-once-and-keep-forever.

- **Compromise**: leaking `PREXIV_DATA_KEY` alone does not directly compromise user accounts — passwords are bcrypt-hashed, session cookies are server-side. It does, however, allow an attacker who *also* gets a DB dump to recover every email address. Treat it as confidential-tier secret material.

- **Operator hardening pass** (do after the first deploy proves stable): once you've watched a few logins/registrations work end-to-end on the encrypted path, run

  ```sql
  UPDATE users SET email = '' WHERE email_hash IS NOT NULL;
  ```

  to clear the plaintext `email` column. Reads already prefer `email_enc`, so this is invisible to users. The plaintext column is *kept by the migration as a rollback safety net*; clearing it removes the last on-disk plaintext copy without dropping the column shape.

### Key management for backup encryption

- **Algorithm**: age 1.x — X25519 + ChaCha20-Poly1305 + scrypt for passphrase variants. Audited; widely deployed. We use the file-keyfile mode (not passphrase) so backups can be encrypted by an unattended cron job and decrypted by deploy.sh without interaction.

- **Private key**: lives at `/etc/prexiv/backup-key.txt` on victoria, mode 0640 owned `root:dbai`. Readable by the `dbai` service account *and* by root. Not readable by anyone else on the box.

- **Public recipient**: lives at `/etc/prexiv/backup.pub`, mode 0644. backup.sh reads this to encrypt; restore.sh and deploy.sh need the *private* key to decrypt.

- **Override locations** (for local dev): `PREXIV_BACKUP_RECIPIENT_FILE` and `BACKUP_KEY` env vars override the default paths.

- **Off-machine key copy** *(operator responsibility — do this before you trust the system)*: copy `/etc/prexiv/backup-key.txt` to a second location not on victoria. A password manager that holds files works; a printed paper copy in a locked drawer works; an offline USB stick works. Without an off-machine copy of the *private* key, an off-machine *backup* is useless — you can't decrypt your own data after a disk-loss event.

- **Rotation**: generate a new keypair, encrypt new backups to the new public key, keep the old private key around long enough to decrypt the still-rotating retention windows (≤3 months given current retention policy). Old encrypted backups don't need re-encryption — they're decryptable as long as the old private key still exists somewhere.

- **Compromise of the private key**: revoke it from victoria, rotate to a new keypair, and treat every backup encrypted to the old key as potentially-leaked. The live DB on victoria is *not* compromised by a private-key leak alone — the key only protects archives at rest.

### Fallback for local dev

If the recipient file (`/etc/prexiv/backup.pub`) doesn't exist, `backup.sh` falls back to writing plaintext `.tar.gz` and prints a warning. This is intentional: local dev boxes don't necessarily need encryption set up, and forcing a key-management story on every developer would be friction with no real protection (a local dev DB has no real users in it). Production deploys should always have the recipient file present.

`restore.sh` and `deploy.sh` handle both extensions — they decrypt `.tar.gz.age` with the private key and treat `.tar.gz` as plaintext.

## 7. Code-level security audit — findings to date

Audit run 2026-05-12 (re-audited same day). Grepped for known antipatterns; verified the high-risk surfaces.

### Findings

| ID | Severity | Status |
|---|---|---|
| **S-1.** Open redirect on `/login?next=` | Medium (phishing-aid) | **FIXED** |
| **S-2.** Session cookie missing explicit `SameSite=Lax` | Low | **FIXED** |
| **S-3.** Defense-in-depth: dynamic table name in `routes/votes.rs` | Informational | **FIXED** |
| **S-4.** No rate limiting in the Rust port | Medium (abuse-aid) | Open — planned, tower-governor |
| **S-5.** No off-machine backup | High (durability) | **FIXED** — `scripts/offmachine-backup.sh` rsyncs encrypted snapshots to `$PREXIV_OFFMACHINE_DEST` after every `backup.sh`; see §6 |
| **S-6.** Backup tarballs plaintext on disk | Medium (leakage) | **FIXED** — age-encrypted to /etc/prexiv/backup.pub, see §6a |
| **S-7.** `users.email` plaintext in DB | Medium (leakage) | **FIXED for email** — AES-256-GCM column-level encryption with HMAC-SHA256 blind index; `rust/src/crypto.rs`, migration `0012_email_at_rest_encryption.sql`, app-startup backfill. `totp_secret` and `webhooks.secret` still pending — see §6a |
| **S-8.** Session fixation: `login_session` did not rotate the session id, so a planted pre-login cookie remained valid post-login | High | **FIXED** — `auth.rs` now calls `session.cycle_id().await` before writing `user_id` |
| **S-9.** PDF written to disk *before* CSRF check on `/submit` — a forged multipart POST left an orphan upload | High | **FIXED** — `routes/submit.rs` buffers the PDF in memory, validates CSRF + all fields, only then writes to disk |
| **S-10.** User-enumeration: `/login` returned different messages for "no such user" vs "wrong password", and the no-such-user branch returned in microseconds vs bcrypt-time for wrong-password | Medium | **FIXED** — `verify_password_timing_safe` runs bcrypt against a fixed dummy hash when the user is missing; both branches return the same `"Incorrect username/email or password."` message |
| **S-11.** Vote/comment endpoints (both HTML form and `/api/v1/manuscripts/{id}/{vote,comments}`) accepted writes against withdrawn manuscripts | Medium | **FIXED** — every write-side handler now reads `withdrawn` along with the lookup and short-circuits with a flash/409 |
| **S-12.** PDF uploads were accepted on filename extension alone (`.pdf`); content was not inspected | Medium | **FIXED** — first 5 bytes must equal `%PDF-` (the PDF magic header). Defense-in-depth: combined with `X-Content-Type-Options: nosniff`, browsers won't render a disguised HTML payload |
| **S-13.** No application-level security response headers (HSTS, X-Frame-Options, X-Content-Type-Options, Referrer-Policy, Permissions-Policy) | Low–Medium | **FIXED** — set globally in `main.rs` via `tower_http::set_header`. HSTS gated on `NODE_ENV=production` to avoid pinning over plaintext HTTP in dev |
| **S-14.** Latent open redirect: vote handler used the `Referer` header verbatim for its redirect target | Low | **FIXED** — `routes/votes.rs::safe_back_path` strips scheme+host and only accepts same-origin paths, with the same hardening rules as `sanitize_next` |
| **S-15.** API token `last_used_at` was bumped before confirming the linked user still exists | Low | **FIXED** — `api_auth.rs::find_user_by_bearer` now updates `last_used_at` only after the user-row fetch succeeds |
| **S-16.** API endpoints returned 200 OK for validation failures (`vote_manuscript`) and "no such token" (`revoke_token`) | Informational | **FIXED** — now 422 and 404 respectively, matching the rest of the API |

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
