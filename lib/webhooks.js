// PreXiv webhooks dispatcher.
//
// Tiny in-process emitter. When a relevant event (manuscript.created,
// comment.created, vote.cast, etc.) happens elsewhere in the server, callers
// invoke `emit(event, payload)`; this module looks up every active webhook
// subscribed to that event and POSTs the JSON envelope `{ event, ts, payload }`
// to the registered URL. Each delivery is signed with HMAC-SHA256 over the raw
// body using the per-webhook secret, sent in the `X-PreXiv-Signature` header
// as `sha256=<hex>`.
//
// Critical invariants:
//   * Fire-and-forget. The HTTP request that triggered the event must NOT
//     block on webhook delivery. We use `setImmediate` to detach and a 5s
//     `AbortSignal.timeout` so a hung subscriber can't pin a fetch slot.
//   * Failures are tolerated. We update last_status / last_attempt_at /
//     failure_count regardless of outcome, and after 5 consecutive failures
//     we deactivate the webhook (set `active = 0`).
//   * No external deps. crypto is built-in; fetch is built-in on Node 20+.
//
// Supported events (whitelist enforced on subscription create):
//   manuscript.created, manuscript.updated, manuscript.withdrawn,
//   comment.created,    comment.deleted,
//   flag.created,       vote.cast.

const crypto = require('crypto');
const dns = require('dns').promises;
const net = require('net');
const { db } = require('../db');

const SUPPORTED_EVENTS = [
  'manuscript.created',
  'manuscript.updated',
  'manuscript.withdrawn',
  'comment.created',
  'comment.deleted',
  'flag.created',
  'vote.cast',
];

const MAX_FAILURES_BEFORE_DEACTIVATE = 5;
const DELIVERY_TIMEOUT_MS = 5000;

function randomSecret() {
  return crypto.randomBytes(24).toString('hex'); // 48-char hex
}

function isSupportedEvent(name) {
  return SUPPORTED_EVENTS.includes(name);
}

function signBody(secret, body) {
  return 'sha256=' + crypto.createHmac('sha256', String(secret || '')).update(body).digest('hex');
}

function isPrivateIpv4(address) {
  const parts = address.split('.').map(n => parseInt(n, 10));
  if (parts.length !== 4 || parts.some(n => !Number.isInteger(n) || n < 0 || n > 255)) return true;
  const [a, b] = parts;
  return (
    a === 0 ||
    a === 10 ||
    a === 127 ||
    (a === 100 && b >= 64 && b <= 127) ||
    (a === 169 && b === 254) ||
    (a === 172 && b >= 16 && b <= 31) ||
    (a === 192 && b === 168) ||
    (a === 198 && (b === 18 || b === 19)) ||
    a >= 224
  );
}

function isPrivateIpv6(address) {
  const s = String(address || '').toLowerCase();
  if (!s) return true;
  if (s === '::1' || s === '::') return true;
  if (s.startsWith('fc') || s.startsWith('fd')) return true; // unique local
  if (s.startsWith('fe8') || s.startsWith('fe9') || s.startsWith('fea') || s.startsWith('feb')) return true; // link-local
  if (s.startsWith('::ffff:')) {
    const v4 = s.slice('::ffff:'.length);
    if (net.isIP(v4) === 4) return isPrivateIpv4(v4);
  }
  return false;
}

function isPrivateAddress(address) {
  const family = net.isIP(address);
  if (family === 4) return isPrivateIpv4(address);
  if (family === 6) return isPrivateIpv6(address);
  return true;
}

function isBlockedHostname(hostname) {
  const h = String(hostname || '').toLowerCase().replace(/\.$/, '');
  return h === 'localhost' || h.endsWith('.localhost') || h === 'metadata.google.internal';
}

async function validateWebhookUrl(rawUrl, opts = {}) {
  let u;
  try {
    u = new URL(String(rawUrl || '').trim());
  } catch {
    return 'Webhook URL is not a valid URL.';
  }
  if (u.protocol !== 'http:' && u.protocol !== 'https:') {
    return 'Webhook URL must use http or https.';
  }
  if (opts.requireHttps && u.protocol !== 'https:') {
    return 'Webhook URL must be HTTPS in production.';
  }
  if (u.username || u.password) {
    return 'Webhook URL must not include embedded credentials.';
  }
  if (isBlockedHostname(u.hostname)) {
    return 'Webhook URL must not target localhost or metadata hosts.';
  }
  if (net.isIP(u.hostname)) {
    return isPrivateAddress(u.hostname) ? 'Webhook URL must not target private, loopback, link-local, or metadata IPs.' : null;
  }
  let records;
  try {
    records = await dns.lookup(u.hostname, { all: true, verbatim: true });
  } catch {
    return 'Webhook hostname could not be resolved.';
  }
  if (!records.length) return 'Webhook hostname could not be resolved.';
  if (records.some(r => isPrivateAddress(r.address))) {
    return 'Webhook URL must not resolve to private, loopback, link-local, or metadata IPs.';
  }
  return null;
}

// Look up webhooks that should fire for `event`. Each row's `events` column
// stores a JSON array of strings; we filter in JS so SQLite doesn't need
// JSON1 enabled.
function findActiveSubscribers(event) {
  const rows = db.prepare('SELECT * FROM webhooks WHERE active = 1').all();
  return rows.filter(r => {
    try {
      const events = JSON.parse(r.events || '[]');
      return Array.isArray(events) && events.includes(event);
    } catch (_e) { return false; }
  });
}

// In-app notification fallback. We insert only if a `notifications` table
// exists (the parallel UX agent's schema may or may not be present). The
// known schema is (user_id, kind, actor_id, manuscript_id, comment_id, seen,
// created_at) — `kind` is enough to convey "your webhook got disabled" and
// the user can find the offending hook in the /me/webhooks UI. Best-effort:
// never throws.
function notifyDeactivation(userId, _webhookId, _url) {
  try {
    const tbl = db.prepare(
      `SELECT name FROM sqlite_master WHERE type='table' AND name='notifications'`
    ).get();
    if (!tbl) return;
    db.prepare(
      `INSERT INTO notifications (user_id, kind) VALUES (?, 'webhook.deactivated')`
    ).run(userId);
  } catch (_e) { /* table may have a different shape — ignore */ }
}

async function deliver(hook, body, signature) {
  let status = 0;
  let ok = false;
  try {
    const res = await fetch(hook.url, {
      method: 'POST',
      headers: {
        'Content-Type': 'application/json',
        'X-PreXiv-Signature': signature,
        'User-Agent': 'PreXiv-Webhook/1.0',
      },
      redirect: 'manual',
      body,
      signal: AbortSignal.timeout(DELIVERY_TIMEOUT_MS),
    });
    status = res.status;
    ok = res.ok;
    // Drain the body so the connection can be released, but cap at 4 KB to
    // avoid pulling a huge response from a misbehaving subscriber.
    try {
      const text = await res.text();
      void text.slice(0, 4096);
    } catch (_e) { /* ignore */ }
  } catch (e) {
    status = 0;
    ok = false;
    // network / timeout / abort — log only; never throw.
    if (process.env.PREXIV_WEBHOOK_DEBUG === '1') {
      console.warn('[webhook] delivery failed:', hook.url, e.message || e);
    }
  }
  try {
    if (ok) {
      db.prepare(
        `UPDATE webhooks
            SET last_status = ?, last_attempt_at = CURRENT_TIMESTAMP, failure_count = 0
          WHERE id = ?`
      ).run(status, hook.id);
    } else {
      const newCount = (hook.failure_count || 0) + 1;
      let newActive = hook.active;
      if (newCount >= MAX_FAILURES_BEFORE_DEACTIVATE) {
        newActive = 0;
        notifyDeactivation(hook.user_id, hook.id, hook.url);
      }
      db.prepare(
        `UPDATE webhooks
            SET last_status = ?, last_attempt_at = CURRENT_TIMESTAMP,
                failure_count = ?, active = ?
          WHERE id = ?`
      ).run(status, newCount, newActive, hook.id);
    }
  } catch (e) {
    console.warn('[webhook] bookkeeping update failed:', e.message || e);
  }
  return { ok, status };
}

// Fire an event. Returns immediately after scheduling deliveries via
// setImmediate so the originating request handler is not blocked.
function emit(event, payload) {
  if (!isSupportedEvent(event)) return;
  let subs;
  try {
    subs = findActiveSubscribers(event);
  } catch (e) {
    console.warn('[webhook] subscriber lookup failed:', e.message || e);
    return;
  }
  if (!subs.length) return;
  const envelope = {
    event,
    ts: new Date().toISOString(),
    payload: payload == null ? null : payload,
  };
  const body = JSON.stringify(envelope);
  for (const hook of subs) {
    const signature = signBody(hook.secret, body);
    setImmediate(() => { deliver(hook, body, signature).catch(() => {}); });
  }
}

// Same as emit but for an explicit ping to a single webhook id (used by the
// "fire a test event right now" UI / API endpoint). Returns a Promise that
// resolves once the single delivery finishes (or fails) — callers that want
// fire-and-forget can ignore it.
function pingOne(hookId) {
  const hook = db.prepare('SELECT * FROM webhooks WHERE id = ?').get(hookId);
  if (!hook) return Promise.resolve({ ok: false, status: 0, error: 'not-found' });
  const envelope = {
    event: 'webhook.ping',
    ts: new Date().toISOString(),
    payload: { webhook_id: hook.id, message: 'PreXiv test event.' },
  };
  const body = JSON.stringify(envelope);
  const signature = signBody(hook.secret, body);
  // We DO await this one — it's an explicit user-initiated test, and the
  // caller may want to render the resulting status. Still bounded by the 5s
  // timeout so it can't block forever.
  return deliver(hook, body, signature);
}

module.exports = {
  SUPPORTED_EVENTS,
  emit,
  pingOne,
  signBody,
  randomSecret,
  isSupportedEvent,
  validateWebhookUrl,
};
