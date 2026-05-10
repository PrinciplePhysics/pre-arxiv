// Verification / reset link plumbing.
// PreXiv does not ship with an SMTP integration. Both the email-verify
// and password-reset flows surface the link directly on the page after the
// user submits the relevant form, and also log it to stdout so a server
// operator can grab it from the journal if a session expires before the
// user copies it. To wire up real email later, swap this file's sendMail
// implementation for a nodemailer (or other) transport — nothing else in
// the app needs to change.

const APP_URL = (process.env.APP_URL || '').replace(/\/+$/, '');

function absoluteUrl(req, p) {
  if (APP_URL) return APP_URL + p;
  const host = req.get('host');
  const proto = req.get('x-forwarded-proto') || (req.secure ? 'https' : 'http');
  return proto + '://' + host + p;
}

async function sendMail({ to, subject, text }) {
  console.log('───── PreXiv [verify/reset link, no SMTP] ─────');
  console.log('To:      ' + to);
  console.log('Subject: ' + subject);
  console.log(text);
  console.log('──────────────────────────────────────────────────');
  return { sent: false, devMode: true };
}

module.exports = { sendMail, absoluteUrl };
