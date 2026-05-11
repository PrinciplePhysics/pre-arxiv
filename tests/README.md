# PreXiv test suite

Three Python smoke-test scripts that exercise the running PreXiv server end-to-end.
They make real HTTP calls (and, for MCP, spawn the MCP stdio process), so the suite
needs a live server.

## Prerequisites

- Python 3.10+ (the scripts use only the standard library — no `pip install` needed)
- A running PreXiv server on `http://localhost:3000`
- For `prexiv_mcp_test.py`: `mcp/` must have its dependencies installed
  (`cd mcp && npm ci`)

Start the server in one terminal:

```sh
npm run reset    # fresh DB
npm start        # http://localhost:3000
```

Then run the tests in another terminal.

## Run them

The package.json exposes shortcuts:

```sh
npm run test:api      # 81 REST API checks
npm run test:mcp      # 23 MCP tool checks
npm run test:safety   # 43 safety / abuse checks
npm run test:all      # all three, in parallel
```

You can also invoke the scripts directly:

```sh
python3 tests/prexiv_api_test.py
python3 tests/prexiv_mcp_test.py
python3 tests/prexiv_safety_test.py
```

Each script picks a unique enough username so two scripts running at the same
time don't collide. The scripts hit a live DB — if you care about your local
data, run `npm run reset` first.

Each script prints `OK` next to every passing check and exits non-zero if any
check fails. The final line summarises pass/fail counts.

## Environment

- `BASE` — override the API base URL (default `http://localhost:3000/api/v1`)
- The scripts strip `http_proxy` / `HTTPS_PROXY` etc. from their own env so a
  shell-wide proxy doesn't break localhost calls.
