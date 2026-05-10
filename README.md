# pre-arxiv

A community archive for **AI-authored, human-conducted manuscripts** — work that doesn't yet meet the bar for arXiv but deserves to be seen, discussed, and (sometimes) corrected. The site is a mixture of arXiv (taxonomy, abstract-first manuscript pages, plain prose) and Hacker News (ranked list, threaded comments, voting).

Each manuscript names a **conductor** (the human + AI who produced it) and, optionally, an **auditor** (a named human expert who has signed a correctness statement). If no auditor is listed, the submitter explicitly disclaims responsibility for correctness, and the manuscript page carries a prominent *unaudited* warning.

## Run it

```sh
npm install
npm run seed     # one-time: creates demo users and manuscripts
npm start        # http://localhost:3000
```

Demo accounts (password `demo1234` for all):
`eulerine`, `noether42`, `feynmann`, `bayesgirl`, `undergrad17`, `hobbyist`.

`npm run reset` wipes the database and re-seeds.

## Stack

- Node 20+, Express 4
- SQLite via `better-sqlite3` (file at `data/prearxiv.db`)
- EJS templates, plain CSS (no build step)
- KaTeX (CDN) for math in abstracts and comments
- Sessions stored in SQLite; passwords hashed with bcrypt
- `helmet` + Content-Security-Policy, `express-rate-limit` on auth/submit/comment/vote, hand-rolled CSRF tokens on all POST forms

## Configuration

Environment variables (all optional in development; `SESSION_SECRET` is required when `NODE_ENV=production`):

| var | default | purpose |
|---|---|---|
| `PORT` | `3000` | port to listen on |
| `SESSION_SECRET` | dev fallback | session-cookie HMAC secret. Required in production. |
| `NODE_ENV` | unset | set to `production` to enforce secure cookies, `trust proxy`, and rate limiting |
| `DATA_DIR` | `./data` | where the SQLite DB and session store live (use a persistent disk in production) |
| `UPLOAD_DIR` | `./public/uploads` | where uploaded PDFs are stored |
| `RATE_LIMIT` | unset | set to `1` to enable rate limiting in development |
| `APP_URL` | derived from the request | absolute base URL used in emailed verify/reset links and citation `url` fields |
| `SMTP_HOST` | unset | if set, the verify/reset flows send real email via nodemailer; otherwise links are surfaced in-page (dev mode) |
| `SMTP_PORT` | `587` | SMTP port (set `465` for implicit TLS) |
| `SMTP_USER` / `SMTP_PASS` | unset | SMTP credentials (optional, depends on the relay) |
| `SMTP_SECURE` | `0` | `1` to force implicit TLS |
| `MAIL_FROM` | `pre-arxiv <no-reply@pre-arxiv.local>` | From: address for outgoing mail |
| `ADMIN_USERNAMES` | unset | comma-separated list; matching users are auto-promoted to admin on every server start |
| `ZENODO_TOKEN` | unset | personal access token from zenodo.org / sandbox.zenodo.org. When set, submissions get real Zenodo DOIs |
| `ZENODO_USE_PRODUCTION` | `0` | set to `1` to use production Zenodo (permanent DOIs) instead of sandbox |

## Layout

```
server.js              all routes
db.js                  SQLite schema, categories, roles
seed.js                demo data
lib/util.js            helpers (timeAgo, ranking, markdown)
lib/auth.js            password hashing, session middleware
views/                 EJS templates
public/css/style.css   the entire stylesheet
public/js/app.js       voting + reply progressive enhancement
public/uploads/        submitted PDFs (git-ignored)
data/                  SQLite DB (git-ignored)
```

## What it does

- **Submit**: title, authors, abstract, category, optional PDF or external URL; required conductor (AI model + human + role); optional auditor (with signed statement) — if absent, an explicit acknowledgement of disclaimed correctness. Submitting requires a verified email; PDF body text is extracted on upload via `pdf-parse` and indexed for full-text search.
- **Read**: arXiv-style manuscript page with abstract, conductor table, auditor table or no-auditor banner, threaded discussion with markdown + math. Each manuscript gets a stable `pa.YYMM.NNNNN` id and a synthetic DOI in the test prefix `10.99999/…` for citation-shaped identifiers (not registered with any DOI registrar).
- **Rank**: home page uses an HN-style score / age decay; `/new`, `/top`, `/audited`, and per-category views are also available.
- **Vote / comment**: any logged-in user; karma accumulates from upvotes.
- **Search**: SQLite FTS5 over title, abstract, authors, and extracted PDF body, with exact-id and DOI matches surfaced first. Try `/search?q=…`.
- **Cite**: every manuscript page has a *Cite* button; `/m/:id/cite` shows BibTeX, RIS, and plain-text formats; `/m/:id/cite.bib` and `/m/:id/cite.ris` return the raw files.
- **Account hygiene**: email verification on register (gates submission); password reset via emailed token; both flows surface the link in-page when no SMTP is configured so the site is usable as a self-hosted demo without mail infra.
- **Anti-bot**: simple math CAPTCHA on `/register`, regenerated on every failed attempt.
- **Moderation**: any submitter can withdraw their own manuscript (replaced with a tombstone preserving id + DOI for citation continuity); any logged-in user can flag manuscripts or comments; admins (configured via `ADMIN_USERNAMES` env) get an `/admin` queue and can permanently delete spam. Comment authors can delete their own comments.
- **Real DOIs (optional)**: if `ZENODO_TOKEN` is set, each new submission is deposited and published on Zenodo (sandbox by default; set `ZENODO_USE_PRODUCTION=1` for permanent DOIs). Without the token, submissions get a synthetic `10.99999/<id>` identifier.

## What it does not do (yet)

- No IP/account-level abuse heuristics beyond rate limits and CAPTCHA.
- No `nofollow`-style search-engine policies, no robots.txt tuning.
- No federated identity / SSO. Local accounts only.
- The Zenodo integration is metadata-only — it doesn't upload the PDF to Zenodo. (Adding `PUT /files/...` before `actions/publish` would, but it shifts the storage burden.)

The site is itself a "manuscript of a website" — written by a human-conductor and an AI co-author and offered without warranty. Issues and pull requests welcome.
