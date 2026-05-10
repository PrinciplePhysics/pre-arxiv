// Tiny email abstraction.
// In production set SMTP_HOST/SMTP_PORT/SMTP_USER/SMTP_PASS/MAIL_FROM and
// nodemailer will dispatch real mail. With nothing set, we run in
// "dev/no-smtp" mode: the message is logged to stdout AND the verify/reset
// link is captured per-session so the relevant page can display it inline.
// That makes the flows usable for self-hosted demos that don't have SMTP.

const nodemailer = require('nodemailer');

const HAS_SMTP = !!process.env.SMTP_HOST;
const FROM     = process.env.MAIL_FROM || 'pre-arxiv <no-reply@pre-arxiv.local>';
const APP_URL  = (process.env.APP_URL || '').replace(/\/+$/, '');

let transporter = null;
if (HAS_SMTP) {
  transporter = nodemailer.createTransport({
    host: process.env.SMTP_HOST,
    port: parseInt(process.env.SMTP_PORT || '587', 10),
    secure: process.env.SMTP_SECURE === '1' || parseInt(process.env.SMTP_PORT, 10) === 465,
    auth: process.env.SMTP_USER ? { user: process.env.SMTP_USER, pass: process.env.SMTP_PASS } : undefined,
  });
}

function absoluteUrl(req, p) {
  if (APP_URL) return APP_URL + p;
  const host = req.get('host');
  const proto = req.get('x-forwarded-proto') || (req.secure ? 'https' : 'http');
  return proto + '://' + host + p;
}

async function sendMail({ to, subject, text }) {
  if (transporter) {
    try {
      await transporter.sendMail({ from: FROM, to, subject, text });
      return { sent: true };
    } catch (e) {
      console.error('[email] send failed:', e.message);
      return { sent: false, error: e.message };
    }
  }
  // dev mode: print to stdout
  console.log('───── pre-arxiv [dev mail] ─────');
  console.log('To:      ' + to);
  console.log('Subject: ' + subject);
  console.log(text);
  console.log('────────────────────────────────');
  return { sent: false, devMode: true };
}

const enabled = HAS_SMTP;

module.exports = { sendMail, absoluteUrl, enabled };
