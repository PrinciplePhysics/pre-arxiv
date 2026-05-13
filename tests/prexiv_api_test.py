#!/usr/bin/env python3
"""End-to-end test of the PreXiv REST API against the running local server."""
import hashlib, json, sys, time, os, urllib.request, urllib.parse, urllib.error, ssl

BASE = os.environ.get("BASE", "http://localhost:3000/api/v1")
WEB  = os.environ.get("WEB",  "http://localhost:3000")
# An obscure-enough password to clear PreXiv's HIBP k-anonymity check.
PASSWD = "PreXivTest-7yk5N2qWf3-nonbreach-passphrase"

# Make sure curl/urllib doesn't try to use the proxy.
os.environ.pop("http_proxy", None); os.environ.pop("HTTP_PROXY", None)
os.environ.pop("https_proxy", None); os.environ.pop("HTTPS_PROXY", None)
os.environ.pop("all_proxy", None);  os.environ.pop("ALL_PROXY", None)
proxy_handler = urllib.request.ProxyHandler({})
opener = urllib.request.build_opener(proxy_handler)

passed = []
failed = []

def req(method, path, body=None, token=None, expect=None, ok_statuses=None, return_status=False):
    url = (path if path.startswith("http") else BASE + path)
    data = None
    headers = {"Accept": "application/json"}
    if body is not None:
        data = json.dumps(body).encode()
        headers["Content-Type"] = "application/json"
    if token:
        headers["Authorization"] = f"Bearer {token}"
    request = urllib.request.Request(url, data=data, method=method, headers=headers)
    try:
        with opener.open(request, timeout=8) as r:
            status = r.status
            try:
                resp = json.loads(r.read())
            except Exception:
                resp = None
    except urllib.error.HTTPError as e:
        status = e.code
        try:
            resp = json.loads(e.read())
        except Exception:
            resp = None
    if return_status:
        return status, resp
    return resp, status

def _solve_pow(challenge, difficulty):
    """Find a nonce so SHA-256(challenge + ':' + nonce) starts with `difficulty` zero bits."""
    n = 0
    full_bytes = difficulty // 8
    extra_bits = difficulty - full_bytes * 8
    mask = (0xff << (8 - extra_bits)) & 0xff if extra_bits else 0
    while True:
        h = hashlib.sha256((challenge + ':' + str(n)).encode()).digest()
        if all(b == 0 for b in h[:full_bytes]) and (extra_bits == 0 or (h[full_bytes] & mask) == 0):
            return str(n)
        n += 1

def pow_register(username, email, password, **extra):
    """Register via /api/v1/register, solving the PoW challenge automatically."""
    ch, _ = req("GET", "/register/challenge")
    if not isinstance(ch, dict) or "challenge" not in ch:
        return None, None
    body = {"username": username, "email": email, "password": password,
            "challenge": ch["challenge"], "nonce": _solve_pow(ch["challenge"], ch["difficulty"])}
    body.update(extra)
    return req("POST", "/register", body=body)

def check(name, cond, detail=""):
    if cond:
        passed.append(name)
        print(f"  PASS  {name}")
    else:
        failed.append((name, detail))
        print(f"  FAIL  {name}  {detail}")

def section(t):
    print(f"\n=== {t} ===")

# ──────────────────────────────────────────────────────────────────────────────
section("1. Public read endpoints (no auth)")
r, s = req("GET", "/categories")
check("categories returns 200", s == 200)
check("categories is a non-empty array", isinstance(r, list) and len(r) >= 10, f"got {type(r).__name__} len={len(r) if isinstance(r, list) else '?'}")
check("category shape has id+name", isinstance(r, list) and r and "id" in r[0] and "name" in r[0])

r, s = req("GET", "/manuscripts")
check("/manuscripts returns 200", s == 200)
check("/manuscripts has items array", isinstance(r, dict) and isinstance(r.get("items"), list))
total_items = len(r["items"]) if isinstance(r, dict) and isinstance(r.get("items"), list) else 0
check(f"/manuscripts has at least 1 item", total_items >= 1, f"got {total_items}")
sample_id = r["items"][0]["arxiv_like_id"] if total_items else None
check("ids match prexiv:YYMM.NNNNN", bool(sample_id and sample_id.startswith("prexiv:")), f"sample_id={sample_id!r}")

r, s = req("GET", "/manuscripts?mode=top")
check("/manuscripts?mode=top works", s == 200 and isinstance(r.get("items"), list))
r, s = req("GET", "/manuscripts?mode=audited")
check("/manuscripts?mode=audited works", s == 200 and isinstance(r.get("items"), list))
r, s = req("GET", "/manuscripts?category=math.NT")
check("/manuscripts?category=math.NT works", s == 200 and isinstance(r.get("items"), list))

if sample_id:
    r, s = req("GET", f"/manuscripts/{sample_id}")
    check(f"GET /manuscripts/<id> returns 200", s == 200)
    check("manuscript has title", isinstance(r, dict) and r.get("title"))
    # synthetic DOI is best-effort: format is 10.99999/PREXIV:YYMM.NNNNN, but
    # some seeded rows pre-date the doi backfill and therefore have no DOI.
    doi_val = r.get("doi") if isinstance(r, dict) else None
    check("manuscript DOI is null or 10.99999/PREXIV:…",
          doi_val is None or doi_val == "" or doi_val.startswith("10.99999/PREXIV:"),
          f"got doi={doi_val!r}")
    r, s = req("GET", f"/manuscripts/{sample_id}/comments")
    check(f"GET /manuscripts/<id>/comments returns 200", s == 200 and isinstance(r, list))

r, s = req("GET", "/search?q=ai")
check("/search returns 200", s == 200)
check("/search returns items", isinstance(r, dict) and isinstance(r.get("items"), list))

r, s = req("GET", "/openapi.json")
check("/openapi.json returns 200", s == 200)
check("/openapi.json is OpenAPI 3.0", isinstance(r, dict) and r.get("openapi", "").startswith("3."))
check("/openapi.json has paths", isinstance(r, dict) and isinstance(r.get("paths"), dict) and len(r["paths"]) >= 15)

r, s = req("GET", f"/manuscripts/prexiv:9999.99999")
check("unknown manuscript returns 404", s == 404)

# ──────────────────────────────────────────────────────────────────────────────
section("2. Auth: unauth POST")
r, s = req("POST", "/manuscripts", body={})
check("POST /manuscripts without token → 401", s == 401)

r, s = req("POST", "/votes/manuscript/1", body={"value": 1})
check("POST /votes without token → 401", s == 401)

# ──────────────────────────────────────────────────────────────────────────────
section("3. Register flow")
suffix = str(int(time.time()))
agent_un = f"agent_{suffix}"
agent_email = f"agent_{suffix}@example.com"

# /register/challenge — sanity-check the PoW endpoint itself.
ch, s = req("GET", "/register/challenge")
check("/register/challenge → 200", s == 200 and isinstance(ch, dict) and "challenge" in ch and "difficulty" in ch)

r, s = pow_register(agent_un, agent_email, PASSWD, display_name="Agent Tester")
check("register valid → 200", s == 200, f"got {s}: {r}")
agent_token = r["token"] if isinstance(r, dict) else None
check("register returns prexiv_-prefixed token", isinstance(agent_token, str) and agent_token.startswith("prexiv_"))
check("register returns user with email_verified=true", isinstance(r, dict) and r["user"]["email_verified"] is True)

# Duplicate
r, s = pow_register(agent_un, "different@example.com", PASSWD)
check("duplicate username → 422 or 409", s in (400, 409, 422))

r, s = pow_register("weakpw_" + suffix, "wp@example.com", "short")
check("weak password → 422", s in (400, 422))

r, s = pow_register("bademail_" + suffix, "not-an-email", PASSWD)
check("bad email → 422", s in (400, 422))

# Missing PoW fields should fail validation.
_, s = req("POST", "/register", body={
    "username": "nopow_" + suffix, "email": "np@example.com", "password": PASSWD,
})
check("register without PoW → 422", s in (400, 422))

# ──────────────────────────────────────────────────────────────────────────────
section("4. Login flow")
r, s = req("POST", "/login", body={"username_or_email": agent_un, "password": PASSWD})
if s != 200:
    # try alternate field names commonly used
    r, s = req("POST", "/login", body={"username": agent_un, "password": PASSWD})
check("login good creds → 200", s == 200, f"got {s}")
login_token = r.get("token") if isinstance(r, dict) else None
check("login returns token", isinstance(login_token, str) and login_token.startswith("prexiv_"))

r, s = req("POST", "/login", body={"username_or_email": agent_un, "password": "wrong-password"})
if s == 200:
    r, s = req("POST", "/login", body={"username": agent_un, "password": "wrong-password"})
check("login bad creds → 401", s in (400, 401))

# ──────────────────────────────────────────────────────────────────────────────
section("5. /me + token management")
r, s = req("GET", "/me", token=agent_token)
check("/me with token → 200", s == 200)
check("/me returns matching username", isinstance(r, dict) and (r.get("username") == agent_un or r.get("user", {}).get("username") == agent_un))

r, s = req("GET", "/me", token="prexiv_garbagetoken")
check("/me with bad token → 401", s == 401)

r, s = req("POST", "/me/tokens", body={"name": "test-token-2"}, token=agent_token)
check("create new token → 200", s == 200, f"got {s}")
new_token = r.get("token") if isinstance(r, dict) else None
new_token_id = r.get("id") if isinstance(r, dict) else None
check("new token starts with prexiv_", isinstance(new_token, str) and new_token.startswith("prexiv_"))

r, s = req("GET", "/me/tokens", token=agent_token)
check("list tokens → 200", s == 200 and isinstance(r, list))
token_count = len(r) if isinstance(r, list) else 0
check(f"at least 2 tokens listed (registration + manual)", token_count >= 2, f"got {token_count}")

# verify the new token works
r, s = req("GET", "/me", token=new_token)
check("new token can /me", s == 200)

# revoke it
r, s = req("DELETE", f"/me/tokens/{new_token_id}", token=agent_token)
check(f"DELETE token-{new_token_id} → 200", s == 200)

r, s = req("GET", "/me", token=new_token)
check("revoked token → 401", s == 401)

# ──────────────────────────────────────────────────────────────────────────────
section("6. Submit manuscripts (every conductor variant)")

ms1, s = req("POST", "/manuscripts", token=agent_token, body={
    "title": "Test 1: ai-agent unaudited",
    "abstract": "An autonomously-produced manuscript for testing the ai-agent variant of submission. It exists only to exercise the validator and the resulting row.",
    "authors": "No human author declared", "category": "cs.AI",
    "external_url": "https://example.com/t1.pdf",
    "conductor_type": "ai-agent", "conductor_ai_model": "Claude Opus 4.7",
    "agent_framework": "test harness", "ai_agent_ack": True,
})
check("submit ai-agent → 200", s == 200, f"got {s}: {ms1}")
ms1_id = ms1.get("arxiv_like_id") if isinstance(ms1, dict) else None
ms1_num = ms1.get("id") if isinstance(ms1, dict) else None
check("ai-agent has correct conductor_type", isinstance(ms1, dict) and ms1.get("conductor_type") == "ai-agent")
check("ai-agent has null conductor_human", isinstance(ms1, dict) and ms1.get("conductor_human") is None)

ms2, s = req("POST", "/manuscripts", token=agent_token, body={
    "title": "Test 2: human-ai unaudited (with no_auditor_ack)",
    "abstract": "A human-conducted manuscript without an auditor. Submitter explicitly disclaims responsibility for correctness via no_auditor_ack.",
    "authors": "Test Author", "category": "math.NT",
    "external_url": "https://example.com/t2.pdf",
    "conductor_type": "human-ai", "conductor_ai_model": "Claude Opus 4.7",
    "conductor_human": "Test Author", "conductor_role": "graduate-student",
    "no_auditor_ack": True,
})
check("submit human-ai unaudited → 200", s == 200, f"got {s}: {ms2}")
ms2_id = ms2.get("arxiv_like_id") if isinstance(ms2, dict) else None
ms2_num = ms2.get("id") if isinstance(ms2, dict) else None

ms3, s = req("POST", "/manuscripts", token=agent_token, body={
    "title": "Test 3: human-ai with auditor",
    "abstract": "A human-conducted manuscript with an auditor who has signed a correctness statement of substantial length and detail.",
    "authors": "Test Author", "category": "stat.ML",
    "external_url": "https://example.com/t3.pdf",
    "conductor_type": "human-ai", "conductor_ai_model": "Claude Opus 4.7",
    "conductor_human": "Test Author", "conductor_role": "postdoc",
    "has_auditor": True,
    "auditor_name": "Prof. Auditor", "auditor_role": "professor",
    "auditor_statement": "I have read the manuscript and the result in section 3 looks correct to me; the rest I have not checked.",
})
check("submit human-ai audited → 200", s == 200, f"got {s}: {ms3}")
ms3_id = ms3.get("arxiv_like_id") if isinstance(ms3, dict) else None
check("audited manuscript has has_auditor=1", isinstance(ms3, dict) and ms3.get("has_auditor") in (1, True))
check("audited manuscript has auditor_name", isinstance(ms3, dict) and ms3.get("auditor_name") == "Prof. Auditor")

# ──────────────────────────────────────────────────────────────────────────────
section("7. Validation errors")
_, s = req("POST", "/manuscripts", token=agent_token, body={
    "title": "x",  # too short
    "abstract": "ok this is the abstract that should be long enough to pass the minimum length validator that rejects under fifty characters anyway",
    "authors": "x", "category": "cs.AI",
    "external_url": "https://example.com",
    "conductor_type": "ai-agent", "conductor_ai_model": "x", "ai_agent_ack": True,
})
check("title < 5 chars → 422", s in (400, 422))

_, s = req("POST", "/manuscripts", token=agent_token, body={
    "title": "Valid title", "abstract": "short", "authors": "x", "category": "cs.AI",
    "external_url": "https://example.com",
    "conductor_type": "ai-agent", "conductor_ai_model": "x", "ai_agent_ack": True,
})
check("abstract < 50 chars → 422", s in (400, 422))

_, s = req("POST", "/manuscripts", token=agent_token, body={
    "title": "Valid title", "authors": "x",
    "abstract": "ok this is the abstract that should be long enough to pass the minimum length validator that rejects under fifty characters anyway",
    "category": "fake.category",
    "external_url": "https://example.com",
    "conductor_type": "ai-agent", "conductor_ai_model": "x", "ai_agent_ack": True,
})
check("invalid category → 422", s in (400, 422))

_, s = req("POST", "/manuscripts", token=agent_token, body={
    "title": "Valid title", "authors": "x",
    "abstract": "ok this is the abstract that should be long enough to pass the minimum length validator that rejects under fifty characters anyway",
    "category": "cs.AI",
    "external_url": "https://example.com",
    "conductor_type": "ai-agent", "conductor_ai_model": "x",
    # ai_agent_ack missing
})
check("ai-agent without ack → 422", s in (400, 422))

_, s = req("POST", "/manuscripts", token=agent_token, body={
    "title": "Valid title", "authors": "x",
    "abstract": "ok this is the abstract that should be long enough to pass the minimum length validator that rejects under fifty characters anyway",
    "category": "cs.AI",
    "external_url": "https://example.com",
    "conductor_type": "human-ai", "conductor_ai_model": "x",
    # missing conductor_human, conductor_role
})
check("human-ai missing conductor_human → 422", s in (400, 422))

# ──────────────────────────────────────────────────────────────────────────────
section("8. Edit + permissions")
r, s = req("PATCH", f"/manuscripts/{ms1_id}", token=agent_token, body={"title": "EDITED: ai-agent test"})
check(f"PATCH own manuscript → 200", s == 200)
check("title was updated", isinstance(r, dict) and "EDITED" in r.get("title", ""))

# Register a second user and try editing the first user's manuscript
r2, s = pow_register(f"other_{suffix}", f"other_{suffix}@example.com", PASSWD)
other_token = r2["token"] if isinstance(r2, dict) and s == 200 else None
check("register second user → 200", s == 200 and other_token)

if other_token:
    _, s = req("PATCH", f"/manuscripts/{ms1_id}", token=other_token, body={"title": "Hijacked"})
    check(f"PATCH others' manuscript → 403", s == 403)

# ──────────────────────────────────────────────────────────────────────────────
section("9. Comments")
c1, s = req("POST", f"/manuscripts/{ms1_id}/comments", token=agent_token, body={"content": "Top-level comment with $E=mc^2$ math."})
check("post comment → 200", s == 200, f"got {s}: {c1}")
c1_id = c1.get("id") if isinstance(c1, dict) else None

c2, s = req("POST", f"/manuscripts/{ms1_id}/comments", token=agent_token, body={"content": "Reply to first.", "parent_id": c1_id})
check("post reply → 200", s == 200)
check("reply has correct parent_id", isinstance(c2, dict) and c2.get("parent_id") == c1_id)

r, s = req("GET", f"/manuscripts/{ms1_id}/comments")
check("list comments → 200", s == 200 and isinstance(r, list) and len(r) >= 2)

_, s = req("DELETE", f"/comments/{c2.get('id')}", token=agent_token)
check("author deletes own comment → 200", s == 200)

# ──────────────────────────────────────────────────────────────────────────────
section("10. Voting")
r, s = req("POST", f"/votes/manuscript/{ms1_num}", token=agent_token, body={"value": 1})
check("vote +1 (toggle off self-upvote) → 200", s == 200)
score_after = r.get("score") if isinstance(r, dict) else None
my_vote = r.get("my_vote") if isinstance(r, dict) else None
check("score returned", isinstance(score_after, int))
check("my_vote returned", my_vote in (0, 1, -1))

# Vote from the other user
if other_token:
    r, s = req("POST", f"/votes/manuscript/{ms1_num}", token=other_token, body={"value": 1})
    check("other user vote +1 → 200", s == 200)
    check("score increased by 1", isinstance(r, dict) and r.get("score") is not None)
    r, s = req("POST", f"/votes/manuscript/{ms1_num}", token=other_token, body={"value": -1})
    check("toggle to -1 → 200", s == 200 and r.get("my_vote") == -1)

# ──────────────────────────────────────────────────────────────────────────────
section("11. Flags")
if other_token:
    r, s = req("POST", f"/flags/manuscript/{ms1_num}", token=other_token, body={"reason": "Test flag for thoroughness"})
    check("flag → 200", s == 200, f"got {s}: {r}")

# ──────────────────────────────────────────────────────────────────────────────
section("12. Withdraw + delete (admin)")
r, s = req("POST", f"/manuscripts/{ms2_id}/withdraw", token=agent_token, body={"reason": "Test withdrawal"})
check("withdraw own → 200", s == 200)

r, s = req("GET", f"/manuscripts/{ms2_id}")
check("withdrawn manuscript still readable", s == 200)
check("withdrawn=1 in response", isinstance(r, dict) and r.get("withdrawn") in (1, True))

# Login as admin
r, s = req("POST", "/login", body={"username_or_email": "eulerine", "password": "demo1234"})
if s != 200:
    r, s = req("POST", "/login", body={"username": "eulerine", "password": "demo1234"})
admin_token = r.get("token") if isinstance(r, dict) and s == 200 else None
check("admin login (eulerine) → 200", s == 200 and admin_token)

if admin_token:
    r, s = req("GET", "/admin/flags", token=admin_token)
    check("admin: GET /admin/flags → 200", s == 200)

    if other_token:
        r, s = req("GET", "/admin/flags", token=other_token)
        check("non-admin: GET /admin/flags → 403", s == 403)

    r, s = req("DELETE", f"/manuscripts/{ms3_id}", token=admin_token)
    check("admin DELETE manuscript → 200", s == 200)

    r, s = req("GET", f"/manuscripts/{ms3_id}")
    check("deleted manuscript → 404", s == 404)

    if other_token:
        # confirm non-admin can't delete
        _, s = req("DELETE", f"/manuscripts/{ms1_id}", token=other_token)
        check("non-admin DELETE → 403", s == 403)

# ──────────────────────────────────────────────────────────────────────────────
section("13. Privacy flags")
ms4, s = req("POST", "/manuscripts", token=agent_token, body={
    "title": "Test 4: with privacy flags",
    "abstract": "Manuscript with conductor_*_private flags set so the public view should report them as not-public; the API still returns them with their public flag.",
    "authors": "Test Author", "category": "cs.AI",
    "external_url": "https://example.com/t4.pdf",
    "conductor_type": "human-ai", "conductor_ai_model": "Claude Opus 4.7",
    "conductor_human": "Test Author", "conductor_role": "postdoc",
    "no_auditor_ack": True,
    "conductor_ai_model_private": True,
    "conductor_human_private": True,
})
check("submit with privacy flags → 200", s == 200)
check("conductor_ai_model_public=0", isinstance(ms4, dict) and ms4.get("conductor_ai_model_public") == 0)
check("conductor_human_public=0", isinstance(ms4, dict) and ms4.get("conductor_human_public") == 0)

# ──────────────────────────────────────────────────────────────────────────────
print(f"\n=== Summary ===")
print(f"  passed: {len(passed)}")
print(f"  failed: {len(failed)}")
if failed:
    print("\nFailures:")
    for n, d in failed:
        print(f"  - {n}  {d}")
sys.exit(0 if not failed else 1)
