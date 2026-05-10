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

- **Submit**: title, authors, abstract, category, optional PDF or external URL; required conductor (AI model + human + role); optional auditor (with signed statement) — if absent, an explicit acknowledgement of disclaimed correctness.
- **Read**: arXiv-style manuscript page with abstract, conductor table, auditor table or no-auditor banner, threaded discussion with markdown + math.
- **Rank**: home page uses an HN-style score / age decay; `/new`, `/top`, `/audited`, and per-category views are also available.
- **Vote / comment**: any logged-in user; karma accumulates from upvotes.

## What it does not do (yet)

- No moderation tools, no flagging, no withdrawal flow.
- No email verification, no password reset.
- No full-text search of PDFs (search is title/abstract/author/id only).
- No DOI minting, no citation export.
- No CAPTCHA on registration, no IP/account-level abuse heuristics. CSRF protection and per-route rate limits *are* in place; CAPTCHA is the remaining gap before posting the URL anywhere a determined bot will find it.

The site is itself a "manuscript of a website" — written by a human-conductor and an AI co-author and offered without warranty. Issues and pull requests welcome.
