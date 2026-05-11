#!/usr/bin/env python3
"""Safety/security test of the agent surface (REST API)."""
import hashlib, json, os, time, urllib.request, urllib.error

BASE = "http://localhost:3000/api/v1"
WEB  = "http://localhost:3000"
PASSWD = "PreXivTest-7yk5N2qWf3-nonbreach-passphrase"
for k in ("http_proxy","HTTP_PROXY","https_proxy","HTTPS_PROXY","all_proxy","ALL_PROXY"):
    os.environ.pop(k, None)
opener = urllib.request.build_opener(urllib.request.ProxyHandler({}))

def req(method, path, body=None, token=None, raw_body=None, ctype=None):
    url = path if path.startswith("http") else BASE + path
    data = None
    headers = {"Accept":"application/json"}
    if raw_body is not None:
        data = raw_body if isinstance(raw_body, bytes) else raw_body.encode()
        if ctype: headers["Content-Type"] = ctype
    elif body is not None:
        data = json.dumps(body).encode()
        headers["Content-Type"] = "application/json"
    if token: headers["Authorization"] = f"Bearer {token}"
    request = urllib.request.Request(url, data=data, method=method, headers=headers)
    try:
        with opener.open(request, timeout=8) as r:
            try:    return r.status, json.loads(r.read() or b"null")
            except: return r.status, None
    except urllib.error.HTTPError as e:
        try:    return e.code, json.loads(e.read())
        except: return e.code, None

passed, failed = [], []
def check(n, c, d=""):
    if c: passed.append(n); print(f"  PASS  {n}")
    else: failed.append((n,d)); print(f"  FAIL  {n}  {d}")

def section(t): print(f"\n=== {t} ===")

def _solve_pow(challenge, difficulty):
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
    _, ch = req("GET", "/register/challenge")
    body = {"username": username, "email": email, "password": password,
            "challenge": ch["challenge"], "nonce": _solve_pow(ch["challenge"], ch["difficulty"])}
    body.update(extra)
    return req("POST", "/register", body=body)

# ── set up two users: alice (mark stuff private), bob (curious other) ──
suffix = str(int(time.time()))
_, body = pow_register(f"alice_{suffix}", f"a_{suffix}@x.com", PASSWD)
ALICE = body["token"]
ALICE_ID = body["user"]["id"]
_, body = pow_register(f"bob_{suffix}", f"b_{suffix}@x.com", PASSWD)
BOB = body["token"]
BOB_ID = body["user"]["id"]
# admin
_, body = req("POST", "/login", body={"username_or_email":"eulerine","password":"demo1234"})
ADMIN = body["token"]

# Alice submits a private-conductor manuscript
_, alice_ms = req("POST", "/manuscripts", token=ALICE, body={
    "title":"Alice's private-conductor manuscript",
    "abstract":"Alice marked both the AI model and her name as private; Bob (an unrelated logged-in user) and an anon viewer should NOT be able to read those fields.",
    "authors":"A. Pseudonym; LLM-X","category":"cs.AI",
    "external_url":"https://example.com/p.pdf",
    "conductor_type":"human-ai","conductor_ai_model":"SECRET-MODEL-XYZ-9000",
    "conductor_human":"Alice's Real Legal Name","conductor_role":"professor",
    "conductor_ai_model_private":True,"conductor_human_private":True,
    "no_auditor_ack":True,
})
ALICE_MS_ID = alice_ms["arxiv_like_id"]
ALICE_MS_NUM = alice_ms["id"]

# ── 1. PRIVACY: anon and bob must NOT see the private fields ──────────────
section("1. Privacy fields actually redacted from non-owners")

_, m_anon = req("GET", f"/manuscripts/{ALICE_MS_ID}")
check("anon: conductor_human is null/undisclosed",
      m_anon.get("conductor_human") in (None, "", "(undisclosed)"),
      f"got: {m_anon.get('conductor_human')!r}")
check("anon: conductor_ai_model is null/undisclosed",
      m_anon.get("conductor_ai_model") in (None, "", "(undisclosed)"),
      f"got: {m_anon.get('conductor_ai_model')!r}")
check("anon: conductor_*_public flags still present so client can label",
      m_anon.get("conductor_human_public") == 0 and m_anon.get("conductor_ai_model_public") == 0)

_, m_bob = req("GET", f"/manuscripts/{ALICE_MS_ID}", token=BOB)
check("bob (other user): conductor_human is null",
      m_bob.get("conductor_human") in (None, "", "(undisclosed)"),
      f"got: {m_bob.get('conductor_human')!r}")
check("bob: conductor_ai_model is null",
      m_bob.get("conductor_ai_model") in (None, "", "(undisclosed)"),
      f"got: {m_bob.get('conductor_ai_model')!r}")

_, m_alice = req("GET", f"/manuscripts/{ALICE_MS_ID}", token=ALICE)
check("alice (owner): sees real conductor_human", m_alice.get("conductor_human") == "Alice's Real Legal Name")
check("alice (owner): sees real conductor_ai_model", m_alice.get("conductor_ai_model") == "SECRET-MODEL-XYZ-9000")

_, m_admin = req("GET", f"/manuscripts/{ALICE_MS_ID}", token=ADMIN)
check("admin: sees real conductor_human", m_admin.get("conductor_human") == "Alice's Real Legal Name")
check("admin: sees real conductor_ai_model", m_admin.get("conductor_ai_model") == "SECRET-MODEL-XYZ-9000")

# Same redaction must happen in list responses
_, listing = req("GET", "/manuscripts?mode=new")
mine = next((x for x in listing.get("items",[]) if x["arxiv_like_id"] == ALICE_MS_ID), None)
if mine:
    check("anon list: privacy redacted in list view",
          mine.get("conductor_human") in (None, "", "(undisclosed)") and mine.get("conductor_ai_model") in (None, "", "(undisclosed)"))

# ── 2. AUTHZ: bob cannot edit/withdraw/delete alice's stuff ────────────────
section("2. Cross-user authorization")

_, body = req("PATCH", f"/manuscripts/{ALICE_MS_ID}", token=BOB, body={"title":"hijacked"})
status_patch_other, _ = req("PATCH", f"/manuscripts/{ALICE_MS_ID}", token=BOB, body={"title":"hijacked"})
# we'll use the status — re-call with raw to capture
_, m_after = req("GET", f"/manuscripts/{ALICE_MS_ID}", token=ALICE)
check("bob's PATCH did NOT change the title", "Alice" in m_after.get("title",""))

_, _ = req("POST", f"/manuscripts/{ALICE_MS_ID}/withdraw", token=BOB, body={"reason":"x"})
_, m_after2 = req("GET", f"/manuscripts/{ALICE_MS_ID}", token=ALICE)
check("bob's withdraw did NOT take effect", m_after2.get("withdrawn") in (0, False))

s, _ = req("DELETE", f"/manuscripts/{ALICE_MS_ID}", token=BOB)
check("bob's DELETE → 403", s == 403)

s, _ = req("GET", "/admin/flags", token=BOB)
check("bob accessing /admin/flags → 403", s == 403)

# bob's comment, alice tries to delete it → should fail
_, c = req("POST", f"/manuscripts/{ALICE_MS_ID}/comments", token=BOB, body={"content":"bob's comment"})
BOB_COMMENT_ID = c["id"]
s, _ = req("DELETE", f"/comments/{BOB_COMMENT_ID}", token=ALICE)
check("alice deleting bob's comment → 403 (only author or admin)", s == 403)

# but admin can delete bob's comment
s, _ = req("DELETE", f"/comments/{BOB_COMMENT_ID}", token=ADMIN)
check("admin can delete any comment → 200", s == 200)

# ── 3. AUTH HYGIENE: token revocation + bad-token responses ─────────────────
section("3. Authentication hygiene")

# create a fresh token, revoke it, ensure it 401s
_, newt = req("POST", "/me/tokens", token=ALICE, body={"name":"to-revoke"})
T = newt["token"]; TID = newt["id"]
s, _ = req("GET", "/me", token=T)
check("freshly minted token works → 200", s == 200)
s, _ = req("DELETE", f"/me/tokens/{TID}", token=ALICE)
check("revoke own token → 200", s == 200)
s, _ = req("GET", "/me", token=T)
check("revoked token → 401", s == 401)

# bob cannot revoke alice's token
_, newt2 = req("POST", "/me/tokens", token=ALICE, body={"name":"bob-cant-revoke"})
TID2 = newt2["id"]
s, _ = req("DELETE", f"/me/tokens/{TID2}", token=BOB)
check("bob revoking alice's token → 403/404", s in (401, 403, 404))
s, _ = req("GET", "/me", token=newt2["token"])
check("alice's token still valid", s == 200)

# garbage tokens
for bad in ["", "not_a_token", "prexiv_too_short", "Bearer prexiv_x", "prexiv_" + "A"*100, "../../etc/passwd"]:
    s, _ = req("GET", "/me", token=bad)
    check(f"garbage token rejected: {bad[:30]!r}", s in (400, 401))

# generic login error (no email enumeration)
s1, b1 = req("POST", "/login", body={"username_or_email":"definitely-does-not-exist","password":"x"})
s2, b2 = req("POST", "/login", body={"username_or_email":f"alice_{suffix}","password":"definitely-wrong"})
check("login error is generic (same message both ways)",
      isinstance(b1, dict) and isinstance(b2, dict) and b1.get("error") == b2.get("error"),
      f"{b1=} vs {b2=}")

# ── 4. INPUT SAFETY: SQL injection attempts ────────────────────────────────
section("4. SQL / injection hygiene")

# search with SQL-injection payload — should not 500, should not return DB errors
for q in ["'; DROP TABLE users; --", "' OR 1=1 --", "%' OR '%' = '%", "xy", "'; SELECT * FROM users; --"]:
    s, b = req("GET", f"/search?q={urllib.parse.quote(q)}")
    check(f"SQL-ish q={q[:25]!r} → 200 not 500", s == 200, f"got {s}: {b}")

# verify users table still has rows
s, alice_check = req("POST", "/login", body={"username_or_email":f"alice_{suffix}","password":PASSWD})
check("users table intact (alice can still login)", s == 200)

# ── 5. XSS / HTML hygiene in comments and conductor_notes ──────────────────
section("5. XSS hygiene — comments are sanitized in HTML render")

# Post a comment with raw <script>
_, _ = req("POST", f"/manuscripts/{ALICE_MS_ID}/comments", token=ALICE,
           body={"content":"<script>alert(1)</script> safe-after"})
# The HTML view is the place that matters. Fetch via the web URL.
import urllib.request as u
html = opener.open(u.Request(f"{WEB}/m/{ALICE_MS_ID}", headers={"Accept":"text/html"})).read().decode()
check("comment <script> stripped in rendered HTML", "<script>alert" not in html, "found a raw <script> in HTML")
check("comment safe text still present in HTML", "safe-after" in html)

# javascript: URL inside markdown link
_, _ = req("POST", f"/manuscripts/{ALICE_MS_ID}/comments", token=ALICE,
           body={"content":"[click](javascript:alert(2)) inert-link"})
html = opener.open(u.Request(f"{WEB}/m/{ALICE_MS_ID}", headers={"Accept":"text/html"})).read().decode()
check("javascript: URL stripped from markdown link",
      'href="javascript:' not in html and 'href=javascript:' not in html)

# attribute injection via crafted markdown — verify no live event handler attribute exists
_, _ = req("POST", f"/manuscripts/{ALICE_MS_ID}/comments", token=ALICE,
           body={"content":'[x](https://example.com/" onmouseover="alert(3))'})
html = opener.open(u.Request(f"{WEB}/m/{ALICE_MS_ID}", headers={"Accept":"text/html"})).read().decode()
import re
# An exploitable injection would look like  <tag ... onmouseover="..." ...>  —
# i.e. onmouseover= immediately following whitespace inside an open tag and
# preceded by no & or quote. Check that no such pattern exists.
exploitable = re.search(r'<[^>]*\son(?:mouseover|click|load|error)\s*=', html, re.IGNORECASE)
check("no live event-handler attribute injected", exploitable is None,
      f"matched: {exploitable.group(0)[:80]!r}" if exploitable else "")

# ── 6. CSRF: cookie-based POSTs still need a token ─────────────────────────
section("6. CSRF still enforced for cookie-auth web routes")

# log in via cookie
import http.cookiejar as cj
jar = cj.CookieJar(); cookie_opener = urllib.request.build_opener(urllib.request.ProxyHandler({}), urllib.request.HTTPCookieProcessor(jar))
# fetch login page to get csrf
loginhtml = cookie_opener.open(f"{WEB}/login").read().decode()
import re
m = re.search(r'name="_csrf" value="([^"]+)"', loginhtml)
csrf1 = m.group(1) if m else None
post_data = urllib.parse.urlencode({"_csrf":csrf1,"username":f"alice_{suffix}","password":PASSWD}).encode()
cookie_opener.open(urllib.request.Request(f"{WEB}/login", data=post_data, method="POST",
                                          headers={"Content-Type":"application/x-www-form-urlencoded"})).read()

# Now try posting a comment WITHOUT csrf token (cookie auth)
post = urllib.parse.urlencode({"content":"sneaky comment"}).encode()
try:
    r = cookie_opener.open(urllib.request.Request(
        f"{WEB}/m/{ALICE_MS_ID}/comment", data=post, method="POST",
        headers={"Content-Type":"application/x-www-form-urlencoded"}))
    csrf_status = r.status
except urllib.error.HTTPError as e:
    csrf_status = e.code
check("cookie-auth POST without CSRF token → 403", csrf_status == 403)

# ── 7. TOKEN STORAGE: tokens are hashed in DB (not plaintext) ──────────────
section("7. Token storage hygiene")
import sqlite3
conn = sqlite3.connect("/Users/dbai/Documents/Research/pre-arxiv/data/prearxiv.db")
rows = conn.execute("SELECT token_hash FROM api_tokens LIMIT 5").fetchall()
check("api_tokens.token_hash is SHA-256 hex (64 chars)", all(len(r[0]) == 64 and all(c in '0123456789abcdef' for c in r[0]) for r in rows), f"got {rows[:1]}")
check("no token in DB starts with prexiv_ (i.e., plaintext leak)", all(not r[0].startswith("prexiv_") for r in rows))

# ── 8. RATE LIMIT: at least exists in production-mode shape (skip in dev) ──
# We check the helper exists and is wired; full enforcement is gated by NODE_ENV
section("8. Rate limit middleware exists on critical endpoints")
import subprocess
hits = subprocess.run(
    ["grep", "-c", "Limiter", "/Users/dbai/Documents/Research/pre-arxiv/lib/api.js"],
    capture_output=True, text=True).stdout.strip()
check("limiter wired at >=4 places in api.js", int(hits or "0") >= 4, f"got {hits}")

# ── 9. SCOPE: API tokens cannot do things owner cannot ─────────────────────
section("9. Tokens never escalate privileges")
# bob is non-admin; bob's token should NOT be able to delete a manuscript
s, _ = req("DELETE", f"/manuscripts/{ALICE_MS_ID}", token=BOB)
check("bob token cannot DELETE → 403", s == 403)

# ── Summary ────────────────────────────────────────────────────────────────
print(f"\n=== Summary ===\n  passed: {len(passed)}\n  failed: {len(failed)}")
if failed:
    print("\nFailures (security-relevant):")
    for n,d in failed: print(f"  - {n}  {d}")
import sys
sys.exit(0 if not failed else 1)
