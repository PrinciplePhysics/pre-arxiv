// Static prose pages (about, guidelines, legal). These have no logic —
// just render the matching EJS template. Kept as their own module so
// adding new prose pages doesn't bloat server.js or another routes file.

/**
 * Register the static-prose routes:
 *   GET /about        — the About page
 *   GET /guidelines   — the community-guidelines page
 *   GET /tos          — Terms of Service
 *   GET /privacy      — Privacy Policy
 *   GET /dmca         — DMCA / takedown process
 *   GET /policies     — Content moderation policies
 *
 * The four legal pages cross-link to each other and are reachable from the
 * site footer (see views/partials/footer.ejs).
 *
 * @param {import('express').Application} app
 * @param {object} [_deps] unused — kept for the uniform `register(app, deps)` shape
 */
function register(app, _deps) {
  app.get('/about',      (_req, res) => res.render('about'));
  app.get('/guidelines', (_req, res) => res.render('guidelines'));
  app.get('/tos',        (_req, res) => res.render('tos'));
  app.get('/privacy',    (_req, res) => res.render('privacy'));
  app.get('/dmca',       (_req, res) => res.render('dmca'));
  app.get('/policies',   (_req, res) => res.render('policies'));
}

module.exports = { register };
