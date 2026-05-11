// Verification / reset link plumbing.
// PreXiv does not ship with an SMTP integration. Both the email-verify
// and password-reset flows surface the link directly on the page after the
// user submits the relevant form, and also log it to stdout so a server
// operator can grab it from the journal if a session expires before the
// user copies it. To wire up real email later, swap this file's sendMail
// implementation for a nodemailer (or other) transport — nothing else in
// the app needs to change.

const APP_URL = (process.env.APP_URL || '').replace(/\/+$/, '');

/**
 * Build an absolute URL for a verify / reset link. Honors APP_URL env if
 * set, else reflects the incoming request's protocol + host.
 * @param {{get:(h:string) => string|undefined, secure?:boolean}} req
 * @param {string} p path starting with '/'
 * @returns {string}
 */
function absoluteUrl(req, p) {
  if (APP_URL) return APP_URL + p;
  const host = req.get('host');
  const proto = req.get('x-forwarded-proto') || (req.secure ? 'https' : 'http');
  return proto + '://' + host + p;
}

/**
 * Best-effort "send" — actually just logs the message to stdout. Returns
 * `{sent:false, devMode:true}` so callers can surface the link directly to
 * the user. To wire up real email, replace this body with a nodemailer (or
 * other) transport call.
 * @param {{to:string, subject:string, text:string}} args
 * @returns {Promise<{sent:boolean, devMode:boolean}>}
 */
async function sendMail({ to, subject, text }) {
  console.log('───── PreXiv [verify/reset link, no SMTP] ─────');
  console.log('To:      ' + to);
  console.log('Subject: ' + subject);
  console.log(text);
  console.log('──────────────────────────────────────────────────');
  return { sent: false, devMode: true };
}

module.exports = { sendMail, absoluteUrl };
