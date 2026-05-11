#!/usr/bin/env python3
"""End-to-end test of every MCP tool via stdio against the running local API."""
import hashlib, json, os, subprocess, sys, time, urllib.request, urllib.error

BASE = "http://localhost:3000/api/v1"
PASSWD = "PreXivTest-7yk5N2qWf3-nonbreach-passphrase"

# Locate the mcp/ directory relative to this test script, so the suite is
# portable across machines (CI, dev laptops, victoria, etc.).
MCP_CWD = os.path.abspath(os.path.join(os.path.dirname(__file__), "..", "mcp"))

# Disable any system proxies for our HTTP calls
for k in ("http_proxy", "HTTP_PROXY", "https_proxy", "HTTPS_PROXY", "all_proxy", "ALL_PROXY"):
    os.environ.pop(k, None)
proxy_handler = urllib.request.ProxyHandler({})
opener = urllib.request.build_opener(proxy_handler)

def http(method, path, body=None, token=None):
    url = path if path.startswith("http") else BASE + path
    data = json.dumps(body).encode() if body is not None else None
    headers = {"Accept": "application/json"}
    if data is not None: headers["Content-Type"] = "application/json"
    if token: headers["Authorization"] = f"Bearer {token}"
    request = urllib.request.Request(url, data=data, method=method, headers=headers)
    try:
        with opener.open(request, timeout=8) as r:
            return r.status, json.loads(r.read() or b"null")
    except urllib.error.HTTPError as e:
        try:    body = json.loads(e.read())
        except: body = None
        return e.code, body

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
    s, ch = http("GET", "/register/challenge")
    assert s == 200 and isinstance(ch, dict), f"challenge fetch failed: {s} {ch}"
    body = {"username": username, "email": email, "password": password,
            "challenge": ch["challenge"], "nonce": _solve_pow(ch["challenge"], ch["difficulty"])}
    body.update(extra)
    return http("POST", "/register", body=body)

# Mint a fresh agent account + token via the API
suffix = str(int(time.time()))
agent_un = f"mcptest_{suffix}"
status, body = pow_register(agent_un, f"{agent_un}@example.com", PASSWD)
assert status == 200, f"register failed: {status} {body}"
TOKEN = body["token"]
print(f"agent: {agent_un}, token: {TOKEN[:20]}...")

# Submit one ai-agent manuscript via the API so we have a known id to read in MCP
status, ms = http("POST", "/manuscripts", token=TOKEN, body={
    "title": "MCP test seed manuscript",
    "abstract": "A manuscript created via the API specifically so the MCP tool tests can fetch it back, comment on it, vote on it, etc. It must be at least fifty characters long.",
    "authors": "Claude Opus 4.7", "category": "cs.AI",
    "external_url": "https://example.com/mcp-seed.pdf",
    "conductor_type": "ai-agent", "conductor_ai_model": "Claude Opus 4.7",
    "agent_framework": "MCP test harness", "ai_agent_ack": True,
})
assert status == 200, f"seed submit failed: {status} {ms}"
SEED_ARX = ms["arxiv_like_id"]
SEED_NUM = ms["id"]
print(f"seed manuscript: {SEED_ARX} (#{SEED_NUM})")

# ─── MCP stdio harness ────────────────────────────────────────────────────────
class MCPClient:
    def __init__(self, token=None):
        env = {**os.environ, "PREXIV_API_URL": BASE}
        if token: env["PREXIV_TOKEN"] = token
        self.proc = subprocess.Popen(
            ["node", "server.js"],
            cwd=MCP_CWD,
            env=env,
            stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=subprocess.PIPE,
            text=True, bufsize=1,
        )
        self.next_id = 1
        self._init()

    def _send(self, msg):
        self.proc.stdin.write(json.dumps(msg) + "\n")
        self.proc.stdin.flush()

    def _recv_until(self, want_id, timeout=8.0):
        start = time.time()
        while time.time() - start < timeout:
            line = self.proc.stdout.readline()
            if not line: break
            line = line.strip()
            if not line: continue
            try: msg = json.loads(line)
            except: continue
            if msg.get("id") == want_id: return msg
        raise TimeoutError(f"no response for id={want_id} (stderr: {self.proc.stderr.read() if self.proc.poll() is not None else ''})")

    def _init(self):
        self._send({"jsonrpc":"2.0","id":self.next_id,"method":"initialize",
                    "params":{"protocolVersion":"2024-11-05","capabilities":{},
                              "clientInfo":{"name":"test","version":"0.1"}}})
        self._recv_until(self.next_id); self.next_id += 1
        self._send({"jsonrpc":"2.0","method":"notifications/initialized"})
        time.sleep(0.1)

    def list_tools(self):
        i = self.next_id; self.next_id += 1
        self._send({"jsonrpc":"2.0","id":i,"method":"tools/list"})
        return self._recv_until(i)["result"]["tools"]

    def call(self, name, args):
        i = self.next_id; self.next_id += 1
        self._send({"jsonrpc":"2.0","id":i,"method":"tools/call",
                    "params":{"name":name,"arguments":args}})
        msg = self._recv_until(i)
        return msg.get("result"), msg.get("error")

    def close(self):
        try: self.proc.stdin.close()
        except: pass
        self.proc.terminate()
        try: self.proc.wait(timeout=2)
        except: self.proc.kill()

passed, failed = [], []
def check(name, cond, detail=""):
    if cond: passed.append(name); print(f"  PASS  {name}")
    else:    failed.append((name, detail)); print(f"  FAIL  {name}  {detail}")

def tool_text(result):
    """Extract the (single) JSON text payload from a tool result."""
    if not result: return None
    content = result.get("content", [])
    if content and content[0].get("type") == "text":
        try: return json.loads(content[0]["text"])
        except: return content[0]["text"]
    return None

# ─── Round 1: read tools, no token ────────────────────────────────────────────
print("\n=== READ tools (no PREXIV_TOKEN required) ===")
mcp = MCPClient(token=None)
try:
    tools = mcp.list_tools()
    check("tools/list returns 12 tools", len(tools) == 12, f"got {len(tools)}")
    expected = {"prexiv_search","prexiv_browse","prexiv_get","prexiv_get_comments",
                "prexiv_list_categories","prexiv_submit","prexiv_edit","prexiv_withdraw",
                "prexiv_add_comment","prexiv_vote","prexiv_flag","prexiv_delete_comment"}
    got = {t["name"] for t in tools}
    check("all expected tool names present", got == expected, f"missing={expected-got} extra={got-expected}")

    res, err = mcp.call("prexiv_list_categories", {})
    cats = tool_text(res)
    check("prexiv_list_categories returns array", isinstance(cats, list) and len(cats) > 10)
    check("category shape ok", isinstance(cats, list) and cats and "id" in cats[0])

    res, err = mcp.call("prexiv_browse", {"mode": "ranked"})
    listing = tool_text(res)
    check("prexiv_browse(ranked) returns dict with items", isinstance(listing, dict) and "items" in listing)
    check("browse has at least 1 item", isinstance(listing, dict) and len(listing.get("items", [])) >= 1)

    res, err = mcp.call("prexiv_browse", {"mode": "audited", "category": "math.NT"})
    check("prexiv_browse(audited+category) returns 200-ish", isinstance(tool_text(res), dict))

    res, err = mcp.call("prexiv_search", {"q": "manuscript"})
    sr = tool_text(res)
    check("prexiv_search returns a list", isinstance(sr, list))
    check("prexiv_search finds the seed manuscript", isinstance(sr, list) and any(m.get("arxiv_like_id") == SEED_ARX for m in sr))

    res, err = mcp.call("prexiv_get", {"id": SEED_ARX})
    m = tool_text(res)
    check("prexiv_get returns the seed manuscript", isinstance(m, dict) and m.get("arxiv_like_id") == SEED_ARX)
    check("seed manuscript title matches", isinstance(m, dict) and "MCP test seed" in m.get("title",""))

    res, err = mcp.call("prexiv_get_comments", {"id": SEED_ARX})
    comments = tool_text(res)
    check("prexiv_get_comments returns array", isinstance(comments, list))

    # Write tool with no token → friendly error
    res, err = mcp.call("prexiv_submit", {
        "title":"x","abstract":"y","authors":"z","category":"cs.AI",
        "external_url":"https://example.com/x.pdf",
        "conductor_type":"ai-agent","conductor_ai_model":"x","ai_agent_ack":True,
    })
    is_err = bool(res and res.get("isError"))
    err_text = ""
    if res and res.get("content"):
        err_text = res["content"][0].get("text", "") if res["content"] else ""
    check("write tool without token → isError=true", is_err, f"res={res} err={err}")
    check("error mentions PREXIV_TOKEN", "PREXIV_TOKEN" in err_text or "token" in err_text.lower(), f"got: {err_text[:200]}")
finally:
    mcp.close()

# ─── Round 2: write tools, with token ─────────────────────────────────────────
print("\n=== WRITE tools (PREXIV_TOKEN set) ===")
mcp = MCPClient(token=TOKEN)
try:
    # submit a fresh manuscript
    res, err = mcp.call("prexiv_submit", {
        "title": "MCP-submitted manuscript",
        "abstract": "This manuscript was submitted directly via the prexiv_submit MCP tool, not through the REST API or the web. It exists for thorough testing.",
        "authors": "Claude Opus 4.7 (autonomous)", "category": "cs.LG",
        "external_url": "https://example.com/mcp-submit.pdf",
        "conductor_type": "ai-agent", "conductor_ai_model": "Claude Opus 4.7",
        "agent_framework": "MCP integration test", "ai_agent_ack": True,
    })
    submitted = tool_text(res)
    is_err = bool(res and res.get("isError"))
    check("prexiv_submit succeeds", not is_err and isinstance(submitted, dict) and submitted.get("arxiv_like_id"),
          f"res={res!r}")
    new_id  = submitted.get("arxiv_like_id") if isinstance(submitted, dict) else None
    new_num = submitted.get("id") if isinstance(submitted, dict) else None

    # edit it
    res, err = mcp.call("prexiv_edit", {"id": new_id, "title": "MCP-submitted manuscript [edited via MCP]"})
    edited = tool_text(res)
    check("prexiv_edit succeeds", not (res and res.get("isError")) and isinstance(edited, dict) and "edited" in edited.get("title", "").lower())

    # add a comment with markdown + math
    res, err = mcp.call("prexiv_add_comment", {
        "manuscript_id": new_id,
        "content": "Comment via MCP. Math: $\\nabla \\cdot E = \\rho/\\varepsilon_0$",
    })
    c = tool_text(res)
    check("prexiv_add_comment top-level succeeds", not (res and res.get("isError")) and isinstance(c, dict) and c.get("id"))
    parent_cid = c.get("id") if isinstance(c, dict) else None

    # add a reply
    res, err = mcp.call("prexiv_add_comment", {
        "manuscript_id": new_id, "content": "Reply via MCP.", "parent_id": parent_cid,
    })
    reply = tool_text(res)
    check("prexiv_add_comment reply succeeds", not (res and res.get("isError")) and isinstance(reply, dict) and reply.get("parent_id") == parent_cid)
    reply_cid = reply.get("id") if isinstance(reply, dict) else None

    # delete the reply
    res, err = mcp.call("prexiv_delete_comment", {"comment_id": reply_cid})
    check("prexiv_delete_comment succeeds", not (res and res.get("isError")))

    # vote on the new manuscript (toggle off self-upvote, then re-vote)
    res, err = mcp.call("prexiv_vote", {"target_type": "manuscript", "target_id": new_num, "value": 1})
    v = tool_text(res)
    check("prexiv_vote (toggle off self-upvote)", not (res and res.get("isError")) and isinstance(v, dict) and "score" in v)

    # flag the seed manuscript
    res, err = mcp.call("prexiv_flag", {"target_type": "manuscript", "target_id": SEED_NUM, "reason": "Test flag via MCP integration test"})
    check("prexiv_flag succeeds", not (res and res.get("isError")), f"res={res!r}")

    # withdraw the new manuscript
    res, err = mcp.call("prexiv_withdraw", {"id": new_id, "reason": "Test withdrawal via MCP"})
    check("prexiv_withdraw succeeds", not (res and res.get("isError")))

    # confirm withdrawal via prexiv_get
    res, err = mcp.call("prexiv_get", {"id": new_id})
    after = tool_text(res)
    check("withdrawn=1 after prexiv_withdraw", isinstance(after, dict) and after.get("withdrawn") in (1, True))
finally:
    mcp.close()

print(f"\n=== Summary ===\n  passed: {len(passed)}\n  failed: {len(failed)}")
if failed:
    print("\nFailures:")
    for n, d in failed:
        print(f"  - {n}  {d}")
sys.exit(0 if not failed else 1)
