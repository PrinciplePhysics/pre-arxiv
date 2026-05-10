// Citation-format generators for a manuscript.
// Inputs are the row returned by the manuscripts query, possibly with the
// submitter username already attached. We don't try to be clever about
// author order — the manuscript's authors string is treated as a list
// separated by ';' or '&' or 'and'.

function splitAuthors(s) {
  if (!s) return [];
  return String(s)
    .split(/\s*(?:;| and |&)\s*/i)
    .map(a => a.trim())
    .filter(Boolean);
}

// Strip TeX-unsafe characters from a value so the output BibTeX parses cleanly.
function texEscape(s) {
  if (s == null) return '';
  return String(s)
    .replace(/\\/g, '\\textbackslash{}')
    .replace(/([&%$#_{}])/g, '\\$1')
    .replace(/~/g, '\\textasciitilde{}')
    .replace(/\^/g, '\\textasciicircum{}');
}

function bibtexKey(m) {
  const auths = splitAuthors(m.authors);
  const surname = (auths[0] || 'unknown').split(/\s+/).pop().toLowerCase().replace(/[^a-z0-9]/g, '');
  const year = new Date(m.created_at + 'Z').getFullYear();
  const tail = (m.arxiv_like_id || '').replace(/[^a-zA-Z0-9]/g, '');
  return `${surname}${year}_${tail}`;
}

function isoYear(m) { return new Date(m.created_at + 'Z').getFullYear(); }

function publicUrl(m, baseUrl) {
  const base = (baseUrl || '').replace(/\/+$/, '');
  return base + '/m/' + m.arxiv_like_id;
}

function renderBibtex(m, baseUrl) {
  const auths = splitAuthors(m.authors).map(texEscape).join(' and ');
  const conductorTag = m.conductor_type === 'ai-agent' ? 'AI-agent (autonomous)' : 'human-conducted';
  return [
    '@misc{' + bibtexKey(m) + ',',
    '  title         = {' + texEscape(m.title) + '},',
    '  author        = {' + auths + '},',
    '  year          = {' + isoYear(m) + '},',
    '  eprint        = {' + (m.arxiv_like_id || '') + '},',
    '  archivePrefix = {PreXiv},',
    m.doi ? '  doi           = {' + m.doi + '},' : null,
    '  url           = {' + publicUrl(m, baseUrl) + '},',
    '  note          = {PreXiv: preprint of preprints; ' + conductorTag + privacySuffix(m) + (m.has_auditor ? '; audited by ' + m.auditor_name : '; unaudited') + '}',
    '}',
  ].filter(Boolean).join('\n');
}

function privacySuffix(m) {
  const bits = [];
  if (m.conductor_ai_model_public === 0) bits.push('AI model undisclosed');
  if (m.conductor_human_public    === 0) bits.push('human conductor undisclosed');
  return bits.length ? '; ' + bits.join(', ') : '';
}

function renderRis(m, baseUrl) {
  const lines = ['TY  - GEN'];
  for (const a of splitAuthors(m.authors)) lines.push('AU  - ' + a);
  lines.push('TI  - ' + (m.title || ''));
  lines.push('PY  - ' + isoYear(m));
  lines.push('AB  - ' + (m.abstract || '').replace(/\s+/g, ' '));
  if (m.arxiv_like_id) lines.push('ID  - ' + m.arxiv_like_id);
  if (m.doi)           lines.push('DO  - ' + m.doi);
  lines.push('UR  - ' + publicUrl(m, baseUrl));
  lines.push('PB  - PreXiv');
  if (m.conductor_type === 'ai-agent') lines.push('N1  - Produced autonomously by ' + (m.conductor_ai_model || 'an AI agent') + '; no human conductor');
  if (m.has_auditor) lines.push('N1  - Audited by ' + m.auditor_name);
  else                lines.push('N1  - Unaudited');
  lines.push('ER  - ');
  return lines.join('\n');
}

function renderPlain(m, baseUrl) {
  const auths = splitAuthors(m.authors).join(', ');
  const url = publicUrl(m, baseUrl);
  return `${auths} (${isoYear(m)}). ${m.title}. PreXiv ${m.arxiv_like_id}. ${url}` +
         (m.doi ? ` doi:${m.doi}` : '');
}

module.exports = { renderBibtex, renderRis, renderPlain };
