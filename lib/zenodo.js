// Zenodo deposition + DOI minting.
// Activated when ZENODO_TOKEN is set. Set ZENODO_USE_PRODUCTION=1 to mint
// permanent DOIs from production Zenodo; otherwise we hit sandbox.zenodo.org
// where DOIs have the form 10.5281/zenodo.NNN but only resolve on the
// sandbox host. Sandbox is the safe default for testing this integration.
//
// On error or if no token is configured, callers should fall back to the
// synthetic 10.99999/<arxiv-id> identifier — never block submission on a
// Zenodo failure.
//
// Reference: https://developers.zenodo.org/

const TOKEN  = process.env.ZENODO_TOKEN || '';
const PROD   = process.env.ZENODO_USE_PRODUCTION === '1';
const HOST   = PROD ? 'zenodo.org' : 'sandbox.zenodo.org';
const enabled = !!TOKEN;

/**
 * Create a Zenodo deposition for a manuscript and publish it. No file is
 * uploaded — Zenodo accepts metadata-only deposits when the account is
 * configured to allow them.
 *
 * Returns one of:
 *   - `{ ok: true,  doi, url, sandbox }`
 *   - `{ ok: false, reason: 'no-token' }`
 *   - `{ ok: false, reason: 'create-failed', status }`
 *   - `{ ok: false, reason: 'publish-failed', status, prereservedDoi? }`
 *   - `{ ok: false, reason: 'exception', message }`
 *
 * Callers should fall back to the synthetic `10.99999/<id>` identifier on
 * any non-ok result — never block submission on a Zenodo failure.
 *
 * @param {object} m manuscript row (arxiv_like_id, title, abstract, authors, …)
 * @param {string} baseUrl absolute origin used for related-identifier links
 * @returns {Promise<{ok:true, doi:string, url?:string, sandbox:boolean}|{ok:false, reason:string, status?:number, prereservedDoi?:string, message?:string}>}
 */
async function depositAndPublish(m, baseUrl) {
  if (!enabled) return { ok: false, reason: 'no-token' };

  const metadata = {
    upload_type: 'preprint',
    publication_type: 'preprint',
    title: m.title,
    creators: splitAuthors(m.authors).map(name => ({ name })),
    description: (m.abstract || '').slice(0, 4000),
    keywords: [m.category, 'PreXiv', 'AI-assisted research'].filter(Boolean),
    notes:
      (m.conductor_type === 'ai-agent'
        ? `Produced autonomously by ${m.conductor_ai_model}` +
          (m.agent_framework ? ` (framework: ${m.agent_framework}). ` : '. ') +
          `No human conductor. `
        : `Conducted by ${m.conductor_human} with ${m.conductor_ai_model}. `) +
      (m.has_auditor
        ? `Audited by ${m.auditor_name}. `
        : `Unaudited — no human auditor has signed a correctness statement. `) +
      `PreXiv id: ${m.arxiv_like_id}.`,
    related_identifiers: [
      { relation: 'isAlternateIdentifier', identifier: m.arxiv_like_id, scheme: 'other' },
      ...(baseUrl ? [{ relation: 'isIdenticalTo', identifier: baseUrl + '/m/' + m.arxiv_like_id, scheme: 'url' }] : []),
    ],
  };

  try {
    const create = await fetch(`https://${HOST}/api/deposit/depositions?access_token=${encodeURIComponent(TOKEN)}`, {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({ metadata }),
    });
    if (!create.ok) {
      const body = await create.text().catch(() => '');
      console.warn('[zenodo] create failed', create.status, body.slice(0, 300));
      return { ok: false, reason: 'create-failed', status: create.status };
    }
    const dep = await create.json();
    const depId = dep.id;
    const reservedDoi = dep.metadata && dep.metadata.prereserve_doi && dep.metadata.prereserve_doi.doi;

    // Publish (without uploading a file — Zenodo allows metadata-only deposits
    // when there's no file requirement; if your account is set to require a
    // file you'd add one here via /files PUT).
    const pub = await fetch(`https://${HOST}/api/deposit/depositions/${depId}/actions/publish?access_token=${encodeURIComponent(TOKEN)}`, {
      method: 'POST',
    });
    if (!pub.ok) {
      const body = await pub.text().catch(() => '');
      console.warn('[zenodo] publish failed', pub.status, body.slice(0, 300));
      // Return the prereserved DOI even on publish failure — it's at least a
      // recognizable identifier.
      return { ok: false, reason: 'publish-failed', status: pub.status, prereservedDoi: reservedDoi };
    }
    const published = await pub.json();
    return {
      ok:  true,
      doi: published.doi || reservedDoi,
      url: published.links && (published.links.html || published.links.record_html),
      sandbox: !PROD,
    };
  } catch (e) {
    console.warn('[zenodo] error:', e.message);
    return { ok: false, reason: 'exception', message: e.message };
  }
}

function splitAuthors(s) {
  if (!s) return [];
  return String(s).split(/\s*(?:;| and |&)\s*/i).map(x => x.trim()).filter(Boolean);
}

module.exports = { depositAndPublish, enabled, HOST, PROD };
