const sanitizeHtml = require('sanitize-html');
const { marked } = require('marked');

function timeAgo(dateStr) {
  const then = new Date(dateStr.endsWith('Z') ? dateStr : dateStr + 'Z').getTime();
  const now = Date.now();
  const diff = Math.max(1, Math.floor((now - then) / 1000));
  if (diff < 60)        return diff + ' second' + (diff === 1 ? '' : 's') + ' ago';
  if (diff < 3600)      { const m = Math.floor(diff / 60);    return m + ' minute' + (m === 1 ? '' : 's') + ' ago'; }
  if (diff < 86400)     { const h = Math.floor(diff / 3600);  return h + ' hour'   + (h === 1 ? '' : 's') + ' ago'; }
  if (diff < 86400 * 30){ const d = Math.floor(diff / 86400); return d + ' day'    + (d === 1 ? '' : 's') + ' ago'; }
  if (diff < 86400 * 365){const mo = Math.floor(diff / (86400 * 30)); return mo + ' month' + (mo === 1 ? '' : 's') + ' ago'; }
  const y = Math.floor(diff / (86400 * 365));
  return y + ' year' + (y === 1 ? '' : 's') + ' ago';
}

function escapeHtml(s) {
  if (s == null) return '';
  return String(s)
    .replace(/&/g, '&amp;')
    .replace(/</g, '&lt;')
    .replace(/>/g, '&gt;')
    .replace(/"/g, '&quot;')
    .replace(/'/g, '&#39;');
}

// Hacker News style ranking. Newer + higher score floats up.
function rankScore(score, ageHours, gravity = 1.8) {
  return (score) / Math.pow(ageHours + 2, gravity);
}

function makeArxivLikeId(date = new Date()) {
  const yy = String(date.getFullYear()).slice(-2);
  const mm = String(date.getMonth() + 1).padStart(2, '0');
  // 5-digit serial within the month — random for now to avoid races
  const serial = String(Math.floor(Math.random() * 99999)).padStart(5, '0');
  return `pa.${yy}${mm}.${serial}`;
}

function renderMarkdown(text) {
  if (!text) return '';
  const html = marked.parse(text, { breaks: true, gfm: true });
  return sanitizeHtml(html, {
    allowedTags: [
      'p', 'br', 'b', 'i', 'em', 'strong', 'a', 'code', 'pre', 'blockquote',
      'ul', 'ol', 'li', 'h1', 'h2', 'h3', 'h4', 'hr', 'span', 'sub', 'sup',
      'table', 'thead', 'tbody', 'tr', 'th', 'td',
    ],
    allowedAttributes: {
      a: ['href', 'title', 'target', 'rel'],
      code: ['class'],
      span: ['class'],
    },
    transformTags: {
      a: sanitizeHtml.simpleTransform('a', { rel: 'nofollow noopener', target: '_blank' }),
    },
  });
}

function ageHours(dateStr) {
  const then = new Date(dateStr.endsWith('Z') ? dateStr : dateStr + 'Z').getTime();
  return (Date.now() - then) / 3600000;
}

function paginate(req, defaultPer = 30) {
  const page = Math.max(1, parseInt(req.query.page, 10) || 1);
  const per  = Math.min(100, Math.max(1, parseInt(req.query.per, 10) || defaultPer));
  return { page, per, offset: (page - 1) * per };
}

module.exports = { timeAgo, escapeHtml, rankScore, makeArxivLikeId, renderMarkdown, ageHours, paginate };
