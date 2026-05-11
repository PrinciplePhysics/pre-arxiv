// TOTP (RFC 6238) + Base32 (RFC 4648) implementation using only Node's
// built-in `crypto`. This deliberately avoids adding an npm dep.
//
// Exports:
//   generateSecret(bytes=20)          -> base32-encoded shared secret
//   getOtpauthUrl(label, secret, opts) -> 'otpauth://totp/...' URI
//   verifyTotp(secret, code, window=1) -> bool, with constant-time compare
//   currentTotp(secret, t=now)         -> 6-digit current code (debug/test)
//
// Implementation notes:
//   - HOTP per RFC 4226: HMAC-SHA1 over the 8-byte counter, then dynamic
//     truncation to a 31-bit integer, modulo 10**digits.
//   - TOTP per RFC 6238: counter = floor(unixtime / period). period is 30s.
//   - We accept ±`window` 30-second steps from the current step (default 1)
//     to tolerate clock skew between server and authenticator.

const crypto = require('crypto');

const BASE32_ALPHABET = 'ABCDEFGHIJKLMNOPQRSTUVWXYZ234567';

function base32Encode(buf) {
  let bits = 0;
  let value = 0;
  let out = '';
  for (let i = 0; i < buf.length; i++) {
    value = (value << 8) | buf[i];
    bits += 8;
    while (bits >= 5) {
      out += BASE32_ALPHABET[(value >>> (bits - 5)) & 0x1f];
      bits -= 5;
    }
  }
  if (bits > 0) {
    out += BASE32_ALPHABET[(value << (5 - bits)) & 0x1f];
  }
  // No padding (most authenticator apps accept unpadded).
  return out;
}

function base32Decode(str) {
  if (typeof str !== 'string') return Buffer.alloc(0);
  // Strip whitespace + padding, normalise case.
  const clean = str.replace(/=+$/g, '').replace(/\s+/g, '').toUpperCase();
  let bits = 0;
  let value = 0;
  const out = [];
  for (let i = 0; i < clean.length; i++) {
    const idx = BASE32_ALPHABET.indexOf(clean[i]);
    if (idx === -1) continue; // skip illegal chars
    value = (value << 5) | idx;
    bits += 5;
    if (bits >= 8) {
      bits -= 8;
      out.push((value >>> bits) & 0xff);
    }
  }
  return Buffer.from(out);
}

function generateSecret(bytes = 20) {
  // 20 random bytes -> 32 base32 chars; matches Google Authenticator default.
  return base32Encode(crypto.randomBytes(bytes));
}

// `label` is conventionally "<issuer>:<account>"; the issuer query parameter
// is also set so apps that prefer it can render an issuer label. We URL-
// encode the components so labels with spaces / colons round-trip cleanly.
function getOtpauthUrl(label, secret, opts = {}) {
  const issuer = opts.issuer || 'PreXiv';
  const period = opts.period || 30;
  const digits = opts.digits || 6;
  const algorithm = (opts.algorithm || 'SHA1').toUpperCase();
  const params = new URLSearchParams();
  params.set('secret', secret);
  params.set('issuer', issuer);
  params.set('algorithm', algorithm);
  params.set('digits', String(digits));
  params.set('period', String(period));
  // Path-encode the label, but keep the conventional ":" separator readable.
  const safeLabel = String(label || '').split(':').map(encodeURIComponent).join(':');
  return 'otpauth://totp/' + safeLabel + '?' + params.toString();
}

function hotp(secretBuf, counter, digits = 6) {
  // 8-byte big-endian counter.
  const buf = Buffer.alloc(8);
  // Use a 64-bit write to handle counters > 2**32 cleanly. Node's
  // writeBigUInt64BE is available since v12.
  buf.writeBigUInt64BE(BigInt(counter), 0);
  const hmac = crypto.createHmac('sha1', secretBuf).update(buf).digest();
  const offset = hmac[hmac.length - 1] & 0x0f;
  const code =
    ((hmac[offset]     & 0x7f) << 24) |
    ((hmac[offset + 1] & 0xff) << 16) |
    ((hmac[offset + 2] & 0xff) <<  8) |
     (hmac[offset + 3] & 0xff);
  const mod = 10 ** digits;
  return String(code % mod).padStart(digits, '0');
}

function currentTotp(secret, opts = {}) {
  const buf = base32Decode(secret);
  const period = opts.period || 30;
  const t = Math.floor((opts.t != null ? opts.t : Date.now() / 1000) / period);
  return hotp(buf, t, opts.digits || 6);
}

// Constant-time string compare for the 6-digit code.
function timingSafeEqualStr(a, b) {
  if (typeof a !== 'string' || typeof b !== 'string') return false;
  if (a.length !== b.length) return false;
  const ab = Buffer.from(a);
  const bb = Buffer.from(b);
  return crypto.timingSafeEqual(ab, bb);
}

function verifyTotp(secret, code, window = 1) {
  if (!secret) return false;
  if (typeof code !== 'string') return false;
  const cleaned = code.replace(/\s+/g, '');
  if (!/^\d{6}$/.test(cleaned)) return false;
  const buf = base32Decode(secret);
  if (!buf.length) return false;
  const period = 30;
  const t = Math.floor(Date.now() / 1000 / period);
  for (let i = -window; i <= window; i++) {
    const candidate = hotp(buf, t + i, 6);
    if (timingSafeEqualStr(candidate, cleaned)) return true;
  }
  return false;
}

module.exports = {
  generateSecret,
  getOtpauthUrl,
  verifyTotp,
  currentTotp,
  base32Encode,
  base32Decode,
};
