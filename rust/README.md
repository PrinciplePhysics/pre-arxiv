# PreXiv — Rust port

This is the in-progress Rust rewrite of PreXiv, chosen as the long-term
foundation for an agent-native preprint server in the AGI era. The
top-level Node.js app under `..` is the spec we're porting from and will
continue to serve traffic until the Rust port reaches feature parity.

## Why Rust

- **Compile-time memory & thread safety** — eliminates entire classes of
  production failures (use-after-free, data races, null bugs) that JS
  cannot prevent. Matters at scale, and matters more when AI agents are
  contributing PRs.
- **Strong, sound type system with sum types** — the compiler catches what
  a hurried agent misses; fewer runtime surprises.
- **Single static binary, no runtime** — operationally robust deploys.
- **Performance comparable to C** — no GC pauses, predictable tail
  latency under agent-scale traffic.

## Stack

| Concern        | Crate           |
|----------------|-----------------|
| HTTP server    | `axum` 0.8      |
| Async runtime  | `tokio` 1.x     |
| Database       | `sqlx` 0.8 (SQLite) |
| Templates      | `maud` 0.26 (compile-time HTML) |
| Middleware     | `tower`, `tower-http` |
| Tracing        | `tracing`, `tracing-subscriber` |
| Errors         | `anyhow`, `thiserror` |

## Layout

```
rust/
├── Cargo.toml
├── migrations/           — sqlx migrations, mirrored from db.js
│   ├── 0001_initial_schema.sql
│   └── 0002_fts5_and_triggers.sql
└── src/
    ├── main.rs           — entry, env, axum setup, tower layers
    ├── state.rs          — AppState { pool, app_url }
    ├── db.rs             — connect() with WAL + foreign_keys
    ├── error.rs          — AppError, AppResult, IntoResponse
    ├── models/           — typed structs with sqlx::FromRow
    ├── routes/           — handler fns
    └── templates/        — maud render functions
```

## Build & run

```sh
# from rust/
cargo build
DATA_DIR=../data cargo run
# serves on http://localhost:3001 by default (PORT env var to override)
```

The Rust app reads the **same** `prearxiv.db` SQLite file as the JS app —
sqlx's `_sqlx_migrations` tracking table is separate from the JS app's
`schema_version` table, and all migrations use `CREATE TABLE IF NOT
EXISTS` so they are no-ops against a DB the JS app already created.
Running both apps simultaneously against the same DB works (SQLite WAL
mode permits one writer + many readers).

## What works today

- `GET /` — paginated listing of recent manuscripts
- `GET /m/{id}` — manuscript detail page (title, conductor, abstract,
  auditor block, comments) — accepts either the `arxiv_like_id`
  (e.g. `prexiv:2605.45626`) or the numeric `id`
- `GET /search?q=...` — FTS5 search over title/abstract/authors/pdf_text
- Static-file serving for `/static/*` (CSS, favicon, uploaded PDFs)
- HTTP gzip compression
- Structured tracing (`RUST_LOG=debug` for verbose)

## Milestones to parity

1. **✅ Foundation** — scaffold, DB layer, three read-only routes (this PR).
2. **Auth** — register/login/logout, session middleware, CSRF, bcrypt
   passwords. Port the rate-limit setup.
3. **Submission flow** — `POST /submit` with multipart PDF upload, FTS
   indexing of extracted text, synthetic-DOI fallback.
4. **Comments + voting** — write paths for comments, votes, flags.
5. **Account self-service** — `/me/edit`, `/me/2fa`, `/me/export`,
   `/me/delete`, `/me/tokens`, `/me/webhooks`.
6. **Moderation** — `/admin` queue, audit log, withdrawal/tombstone.
7. **Discovery** — profile pages, follows, notifications, feed.
8. **REST API** — `/api/v1/*` with API-token auth (parity with the JS
   `OpenAPI` spec under `../lib/openapi.js`).
9. **Integrations** — Zenodo deposit + PDF upload, OAI-PMH endpoint,
   webhook dispatcher.
10. **SSO** — ORCID + GitHub + Google OAuth, federated account linking.
11. **Abuse heuristics** — brute-force / spam / scraping signals layered
    on top of rate limits.
12. **Promote to root** — once parity is verified end-to-end, delete the
    JS code, move `rust/` to the repo root, and `prexiv` becomes a single
    static binary.

## Conventions

- Templates return `maud::Markup`; handlers wrap them in
  `axum::response::Html<String>` because maud 0.26 binds against
  axum-core 0.4 while axum 0.8 uses axum-core 0.5. The wrap is one line
  and zero-cost.
- SQL queries use runtime checks (`sqlx::query_as::<_, T>(...)`) for now.
  Once the schema stabilises and CI has a seeded DB, we'll switch to the
  compile-time-checked `query_as!` macro for stronger guarantees.
- Booleans live as `i64` in models (SQLite stores `0`/`1`); each struct
  gets an explicit `is_*()` method on `impl` for readability.

## Contributing

See the JS app under `..` for the behavioural spec. When porting a route:

1. Find the route handler in `../routes/<file>.js` and its rendering
   template in `../views/<file>.ejs`.
2. Add a typed handler in `src/routes/`, a maud template in
   `src/templates/`, and any new model fields in `src/models/`.
3. Run `cargo build` — the compiler is your TODO list. If something is
   missing on the JS side that you need typed, add it as a TODO comment
   pointing at the JS file/line.
