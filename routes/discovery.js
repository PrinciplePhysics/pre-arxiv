// Discovery and probe routes. These are public, side-effect-free, and meant
// to be findable by humans, search engines, and AI agents alike.
//
//   GET  /healthz                       — liveness probe (always 200)
//   GET  /readyz                        — readiness probe (200 iff DB reachable)
//   GET  /robots.txt                    — search-engine + agent crawl policy
//   GET  /sitemap.xml                   — XML sitemap of every public manuscript
//   GET  /manifest.json                 — PWA web app manifest
//   GET  /openapi.json                  — root-level alias of /api/v1/openapi.json
//   GET  /openapi.yaml                  — same spec, YAML serialization
//   GET  /feed.atom                     — Atom 1.0 of the latest 50 manuscripts
//   GET  /feed.rss                      — RSS 2.0 of the latest 50 manuscripts
//   GET  /feed.json                     — JSON Feed 1.1 of the latest 50 manuscripts
//   GET  /oai-pmh                       — OAI-PMH 2.0 endpoint (XML)
//   POST /oai-pmh                       — OAI-PMH 2.0 via POST (per spec)
//   GET  /.well-known/oai-pmh           — convenience pointer → /oai-pmh
//   GET  /.well-known/prexiv-manifest   — agent-discovery manifest (JSON)
//   GET  /.well-known/security.txt      — security.txt per RFC 9116

const { db } = require('../db');
const { escapeHtml } = require('../lib/util');

/**
 * Format a Date / SQLite datetime as RFC 3339 / ISO 8601.
 * @param {string|Date} v
 * @returns {string}
 */
function rfc3339(v) {
  const s = typeof v === 'string' ? (v.endsWith('Z') ? v : v + 'Z') : v.toISOString();
  return new Date(s).toISOString();
}

/**
 * Format a Date / SQLite datetime as RFC 822 (RSS-friendly).
 * @param {string|Date} v
 * @returns {string}
 */
function rfc822(v) {
  const s = typeof v === 'string' ? (v.endsWith('Z') ? v : v + 'Z') : v.toISOString();
  return new Date(s).toUTCString();
}

/**
 * Compute the canonical origin (honors APP_URL env, falls back to req).
 * @param {import('express').Request} req
 */
function originOf(req) {
  const proto = req.get('x-forwarded-proto') || (req.secure ? 'https' : 'http');
  const host  = req.get('host');
  return (process.env.APP_URL || '').replace(/\/+$/, '') || (proto + '://' + host);
}

/**
 * Tiny JSON → YAML serializer. We avoid pulling in `js-yaml` because the
 * surface we render is OpenAPI / structured config: nested plain objects and
 * arrays of primitives. Output uses 2-space indent and always quotes strings
 * that look like they could be misparsed (numbers, bools, anchors, indicators).
 * @param {*} v
 * @param {number} [indent]
 * @returns {string}
 */
function toYaml(v, indent = 0) {
  const pad = '  '.repeat(indent);
  if (v === null || v === undefined) return 'null';
  if (typeof v === 'boolean' || typeof v === 'number') return String(v);
  if (typeof v === 'string') {
    // Always single-quote strings; double the literal single-quote inside.
    return "'" + v.replace(/'/g, "''") + "'";
  }
  if (Array.isArray(v)) {
    if (v.length === 0) return '[]';
    return v.map(item => {
      const child = toYaml(item, indent + 1);
      if (child.includes('\n')) {
        // For multi-line (object/array) children, attach the dash to the
        // first non-blank line and indent the rest.
        const lines = child.split('\n');
        return pad + '- ' + lines[0].replace(/^\s+/, '') + '\n' +
          lines.slice(1).map(l => l).join('\n');
      }
      return pad + '- ' + child;
    }).join('\n');
  }
  if (typeof v === 'object') {
    const keys = Object.keys(v);
    if (keys.length === 0) return '{}';
    return keys.map(k => {
      const child = v[k];
      const safeKey = /^[A-Za-z_][\w.-]*$/.test(k) ? k : "'" + k.replace(/'/g, "''") + "'";
      if (child === null || child === undefined) return pad + safeKey + ': null';
      if (typeof child === 'string' || typeof child === 'boolean' || typeof child === 'number')
        return pad + safeKey + ': ' + toYaml(child, 0);
      if (Array.isArray(child) && child.length === 0) return pad + safeKey + ': []';
      if (Array.isArray(child)) return pad + safeKey + ':\n' + toYaml(child, indent + 1);
      // object
      if (Object.keys(child).length === 0) return pad + safeKey + ': {}';
      return pad + safeKey + ':\n' + toYaml(child, indent + 1);
    }).join('\n');
  }
  return String(v);
}

/**
 * Register the discovery / probe routes.
 *
 * Probes are deliberately registered before any route module that might
 * depend on session machinery so that monitoring systems get a fast,
 * cookie-free 200/503 answer.
 *
 * @param {import('express').Application} app
 * @param {object} [_deps]
 */
function register(app, _deps) {
  let pkgVersion = null;
  try { pkgVersion = require('../package.json').version; } catch { /* fall through */ }
  const probePayload = () => ({
    ok: true,
    uptime_s: Math.round(process.uptime()),
    version: pkgVersion,
    node: process.version,
  });

  app.get('/healthz', (_req, res) => {
    res.status(200).json(probePayload());
  });
  app.get('/readyz', (_req, res) => {
    try {
      db.prepare('SELECT 1').get();
      res.status(200).json(probePayload());
    } catch (e) {
      res.status(503).json({ ok: false, error: 'db unreachable: ' + ((/** @type {Error} */ (e)).message || 'unknown') });
    }
  });

  // ─── robots.txt ───────────────────────────────────────────────────────────
  // Allow all crawlers across the public surface; explicitly disallow auth /
  // user-private surfaces. Advertise the sitemap so well-behaved bots find it.
  app.get('/robots.txt', (req, res) => {
    const origin = originOf(req);
    const lines = [
      'User-agent: *',
      'Allow: /',
      'Disallow: /me/',
      'Disallow: /admin',
      'Disallow: /admin/',
      'Disallow: /login',
      'Disallow: /register',
      'Disallow: /forgot',
      'Disallow: /reset/',
      'Disallow: /verify/',
      '',
      'Sitemap: ' + origin + '/sitemap.xml',
    ];
    res.type('text/plain').send(lines.join('\n') + '\n');
  });

  // ─── sitemap.xml ──────────────────────────────────────────────────────────
  // Static prose + every non-withdrawn manuscript. Caps at 5 000 URLs which is
  // well below the 50k limit and fits comfortably in a single uncompressed
  // sitemap. Withdrawn manuscripts are deliberately excluded.
  app.get('/sitemap.xml', (req, res) => {
    const origin = originOf(req);
    const staticPaths = [
      { loc: '/',           changefreq: 'hourly',  priority: '1.0' },
      { loc: '/new',        changefreq: 'hourly',  priority: '0.9' },
      { loc: '/top',        changefreq: 'daily',   priority: '0.8' },
      { loc: '/audited',    changefreq: 'daily',   priority: '0.8' },
      { loc: '/browse',     changefreq: 'weekly',  priority: '0.6' },
      { loc: '/about',      changefreq: 'monthly', priority: '0.4' },
      { loc: '/guidelines', changefreq: 'monthly', priority: '0.4' },
      { loc: '/tos',        changefreq: 'yearly',  priority: '0.2' },
      { loc: '/privacy',    changefreq: 'yearly',  priority: '0.2' },
      { loc: '/dmca',       changefreq: 'yearly',  priority: '0.2' },
      { loc: '/policies',   changefreq: 'yearly',  priority: '0.2' },
    ];
    const rows = /** @type {{arxiv_like_id:string, updated_at:string}[]} */ (
      db.prepare(`
        SELECT arxiv_like_id, COALESCE(updated_at, created_at) AS updated_at
        FROM manuscripts
        WHERE withdrawn = 0
        ORDER BY COALESCE(updated_at, created_at) DESC
        LIMIT 5000
      `).all()
    );
    const xmlEscape = (/** @type {string} */ s) => String(s)
      .replace(/&/g, '&amp;').replace(/</g, '&lt;')
      .replace(/>/g, '&gt;').replace(/"/g, '&quot;').replace(/'/g, '&apos;');
    const parts = ['<?xml version="1.0" encoding="UTF-8"?>',
      '<urlset xmlns="http://www.sitemaps.org/schemas/sitemap/0.9">'];
    for (const s of staticPaths) {
      parts.push('  <url>');
      parts.push('    <loc>' + xmlEscape(origin + s.loc) + '</loc>');
      parts.push('    <changefreq>' + s.changefreq + '</changefreq>');
      parts.push('    <priority>' + s.priority + '</priority>');
      parts.push('  </url>');
    }
    for (const row of rows) {
      parts.push('  <url>');
      parts.push('    <loc>' + xmlEscape(origin + '/m/' + row.arxiv_like_id) + '</loc>');
      parts.push('    <lastmod>' + rfc3339(row.updated_at) + '</lastmod>');
      parts.push('    <changefreq>weekly</changefreq>');
      parts.push('    <priority>0.7</priority>');
      parts.push('  </url>');
    }
    parts.push('</urlset>');
    res.type('application/xml').send(parts.join('\n') + '\n');
  });

  // ─── manifest.json (PWA) ──────────────────────────────────────────────────
  app.get('/manifest.json', (_req, res) => {
    res.type('application/manifest+json').json({
      name:             'PreXiv',
      short_name:       'PreXiv',
      description:      'Community archive for manuscripts with explicit AI-use disclosure that have not yet passed rigorous human audit.',
      start_url:        '/',
      scope:            '/',
      display:          'standalone',
      orientation:      'any',
      background_color: '#fbf6f0',
      theme_color:      '#b8430a',
      lang:             'en',
      icons: [
        { src: '/favicon.svg', sizes: 'any', type: 'image/svg+xml', purpose: 'any maskable' },
      ],
      categories: ['science', 'education', 'productivity'],
    });
  });

  // ─── OpenAPI mirror (root) ────────────────────────────────────────────────
  // The canonical spec lives at /api/v1/openapi.json. Many crawlers and
  // editors look at the site root first, so we mirror it as both .json and
  // .yaml serializations there too.
  app.get('/openapi.json', (req, res) => {
    try {
      const { buildOpenApi } = require('../lib/openapi');
      res.type('application/json').send(JSON.stringify(buildOpenApi(originOf(req)), null, 2));
    } catch (e) {
      res.status(500).json({ ok: false, error: 'openapi unavailable: ' + ((/** @type {Error} */ (e)).message || 'unknown') });
    }
  });
  app.get('/openapi.yaml', (req, res) => {
    try {
      const { buildOpenApi } = require('../lib/openapi');
      res.type('application/yaml').send(toYaml(buildOpenApi(originOf(req))) + '\n');
    } catch (e) {
      res.status(500).type('text/plain').send('openapi unavailable: ' + ((/** @type {Error} */ (e)).message || 'unknown'));
    }
  });

  // ─── public site-wide feeds ───────────────────────────────────────────────
  // /feed (the path) is reserved for the authenticated social inbox in
  // routes/social.js. The .atom / .rss / .json siblings are the public
  // latest-50-manuscripts feed for newsreaders and bots.
  function fetchLatestForFeed() {
    return /** @type {{arxiv_like_id:string, doi:string|null, title:string, abstract:string, authors:string, category:string, created_at:string, updated_at:string|null, submitter_username:string, conductor_type:string, has_auditor:number}[]} */ (
      db.prepare(`
        SELECT m.arxiv_like_id, m.doi, m.title, m.abstract, m.authors, m.category,
               m.created_at, m.updated_at, u.username AS submitter_username,
               m.conductor_type, m.has_auditor
        FROM manuscripts m
        JOIN users u ON u.id = m.submitter_id
        WHERE m.withdrawn = 0
        ORDER BY m.created_at DESC
        LIMIT 50
      `).all()
    );
  }

  app.get('/feed.atom', (req, res) => {
    const origin = originOf(req);
    const items = fetchLatestForFeed();
    const updated = items.length ? rfc3339(items[0].updated_at || items[0].created_at) : new Date().toISOString();
    const parts = ['<?xml version="1.0" encoding="UTF-8"?>',
      '<feed xmlns="http://www.w3.org/2005/Atom">',
      '  <title>PreXiv — latest manuscripts</title>',
      '  <link href="' + escapeHtml(origin) + '/" rel="alternate" type="text/html"/>',
      '  <link href="' + escapeHtml(origin) + '/feed.atom" rel="self" type="application/atom+xml"/>',
      '  <id>' + escapeHtml(origin) + '/</id>',
      '  <updated>' + updated + '</updated>',
      '  <subtitle>AI-assisted manuscripts pending peer audit.</subtitle>',
      '  <generator>PreXiv</generator>',
    ];
    for (const m of items) {
      const url = origin + '/m/' + m.arxiv_like_id;
      const summary = (m.abstract || '').slice(0, 1000);
      parts.push('  <entry>');
      parts.push('    <title>' + escapeHtml(m.title) + '</title>');
      parts.push('    <link href="' + escapeHtml(url) + '" rel="alternate" type="text/html"/>');
      parts.push('    <id>' + escapeHtml(url) + '</id>');
      parts.push('    <published>' + rfc3339(m.created_at) + '</published>');
      parts.push('    <updated>'   + rfc3339(m.updated_at || m.created_at) + '</updated>');
      parts.push('    <author><name>' + escapeHtml(m.authors) + '</name></author>');
      parts.push('    <category term="' + escapeHtml(m.category) + '"/>');
      parts.push('    <summary type="text">' + escapeHtml(summary) + '</summary>');
      parts.push('  </entry>');
    }
    parts.push('</feed>');
    res.type('application/atom+xml').send(parts.join('\n') + '\n');
  });

  app.get('/feed.rss', (req, res) => {
    const origin = originOf(req);
    const items = fetchLatestForFeed();
    const lastBuild = items.length ? rfc822(items[0].updated_at || items[0].created_at) : new Date().toUTCString();
    const parts = ['<?xml version="1.0" encoding="UTF-8"?>',
      '<rss version="2.0" xmlns:atom="http://www.w3.org/2005/Atom">',
      '  <channel>',
      '    <title>PreXiv — latest manuscripts</title>',
      '    <link>' + escapeHtml(origin) + '/</link>',
      '    <atom:link href="' + escapeHtml(origin) + '/feed.rss" rel="self" type="application/rss+xml"/>',
      '    <description>AI-assisted manuscripts pending peer audit.</description>',
      '    <lastBuildDate>' + lastBuild + '</lastBuildDate>',
      '    <generator>PreXiv</generator>',
    ];
    for (const m of items) {
      const url = origin + '/m/' + m.arxiv_like_id;
      const summary = (m.abstract || '').slice(0, 1000);
      parts.push('    <item>');
      parts.push('      <title>' + escapeHtml(m.title) + '</title>');
      parts.push('      <link>' + escapeHtml(url) + '</link>');
      parts.push('      <guid isPermaLink="true">' + escapeHtml(url) + '</guid>');
      parts.push('      <pubDate>' + rfc822(m.created_at) + '</pubDate>');
      parts.push('      <category>' + escapeHtml(m.category) + '</category>');
      parts.push('      <description>' + escapeHtml(summary) + '</description>');
      parts.push('    </item>');
    }
    parts.push('  </channel>', '</rss>');
    res.type('application/rss+xml').send(parts.join('\n') + '\n');
  });

  app.get('/feed.json', (req, res) => {
    const origin = originOf(req);
    const items = fetchLatestForFeed();
    res.type('application/feed+json').json({
      version:       'https://jsonfeed.org/version/1.1',
      title:         'PreXiv — latest manuscripts',
      home_page_url: origin + '/',
      feed_url:      origin + '/feed.json',
      description:   'AI-assisted manuscripts pending peer audit.',
      items: items.map(m => ({
        id:             origin + '/m/' + m.arxiv_like_id,
        url:            origin + '/m/' + m.arxiv_like_id,
        title:          m.title,
        content_text:   m.abstract,
        date_published: rfc3339(m.created_at),
        date_modified:  rfc3339(m.updated_at || m.created_at),
        author:        { name: m.authors, url: origin + '/u/' + m.submitter_username },
        tags:          [m.category, m.conductor_type, m.has_auditor ? 'audited' : 'unaudited'],
      })),
    });
  });

  // ─── /.well-known/security.txt ────────────────────────────────────────────
  app.get('/.well-known/security.txt', (req, res) => {
    const origin = originOf(req);
    const expires = new Date(Date.now() + 365 * 24 * 3600 * 1000).toISOString();
    const lines = [
      '# Security contact for PreXiv',
      'Contact: ' + (process.env.SECURITY_CONTACT || origin + '/dmca'),
      'Expires: ' + expires,
      'Preferred-Languages: en',
      'Canonical: ' + origin + '/.well-known/security.txt',
      'Policy: ' + origin + '/dmca',
    ];
    res.type('text/plain').send(lines.join('\n') + '\n');
  });

  // OAI-PMH (best-effort — handler may be missing on a cut-down tree).
  /** @type {((req:any, res:any) => void)|null} */
  let handleOai = null;
  try {
    handleOai = require('../lib/oaipmh').handleOaiRequest;
  } catch (_e) { /* lib/oaipmh.js may be missing */ }
  if (handleOai) {
    app.get('/oai-pmh', (req, res) => /** @type {(r:any,s:any)=>void} */ (handleOai)(req, res));
    app.post('/oai-pmh', (req, res) => /** @type {(r:any,s:any)=>void} */ (handleOai)(req, res));
    // /.well-known convenience — some crawlers look here first.
    app.get('/.well-known/oai-pmh', (_req, res) => res.redirect(301, '/oai-pmh?verb=Identify'));
  }

  // Agent-discovery manifest (best-effort).
  /** @type {((req:any) => any)|null} */
  let buildManifest = null;
  try {
    buildManifest = require('../lib/manifest').buildManifest;
  } catch (_e) { /* lib/manifest.js may be missing */ }
  if (buildManifest) {
    app.get('/.well-known/prexiv-manifest', (req, res) => {
      res.type('application/json').send(JSON.stringify(/** @type {(r:any)=>any} */ (buildManifest)(req), null, 2));
    });
  }
}

module.exports = { register };
