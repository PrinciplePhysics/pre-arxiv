// Best-effort audit log.
//
// Records actor, action, target, optional detail, and source IP. We
// deliberately swallow insert failures: an audit-log outage must never block a
// user-facing action (admin would not want delete to fail because the audit
// table got locked, etc.), but we DO log to stderr so an operator notices.

const { db } = require('../db');

function getRequestIp(req) {
  if (!req) return null;
  // express's `req.ip` honors `trust proxy` if it's been set; fall back to
  // the raw socket address.
  return (req.ip
    || (req.headers && req.headers['x-forwarded-for'] ? String(req.headers['x-forwarded-for']).split(',')[0].trim() : null)
    || (req.socket && req.socket.remoteAddress)
    || null);
}

function actorIdFor(req) {
  if (!req || !req.user) return null;
  return req.user.id || null;
}

function auditLog(req, action, target_type, target_id, detail) {
  try {
    const ip = getRequestIp(req);
    const actor = actorIdFor(req);
    db.prepare(
      'INSERT INTO audit_log (actor_user_id, action, target_type, target_id, detail, ip) VALUES (?, ?, ?, ?, ?, ?)'
    ).run(
      actor,
      String(action || '').slice(0, 100),
      target_type ? String(target_type).slice(0, 50) : null,
      Number.isInteger(target_id) ? target_id : (target_id == null ? null : parseInt(target_id, 10) || null),
      detail == null ? null : String(detail).slice(0, 1000),
      ip ? String(ip).slice(0, 64) : null,
    );
  } catch (e) {
    // Audit failure must never break the user action. Surface it on stderr
    // so an operator can investigate; do nothing else.
    console.warn('[audit] insert failed:', e.message || e);
  }
}

module.exports = { auditLog };
