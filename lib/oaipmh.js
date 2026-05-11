// OAI-PMH 2.0 endpoint for PreXiv.
// Spec: https://www.openarchives.org/OAI/openarchivesprotocol.html
//
// We support: Identify, ListMetadataFormats, ListIdentifiers, ListRecords,
// GetRecord. Only the oai_dc metadata format is offered.
// Resumption tokens are intentionally NOT implemented; responses are capped
// at 100 records per request. Withdrawn manuscripts surface as
// <header status="deleted">.

const { db } = require('../db');

const RESPONSE_CAP = 100;

function xmlEscape(s) {
  if (s == null) return '';
  return String(s)
    .replace(/&/g, '&amp;')
    .replace(/</g, '&lt;')
    .replace(/>/g, '&gt;')
    .replace(/"/g, '&quot;')
    .replace(/'/g, '&apos;');
}

function originFor(req) {
  const proto  = req.get('x-forwarded-proto') || (req.secure ? 'https' : 'http');
  const host   = req.get('host');
  return (process.env.APP_URL || '').replace(/\/+$/, '') || (proto + '://' + host);
}

function nowUTCStamp() {
  return new Date().toISOString().replace(/\.\d+Z$/, 'Z');
}

// Convert a SQLite DATETIME (e.g. "2026-05-11 12:34:56") to ISO-8601 UTC.
function isoStampFromSqlite(s) {
  if (!s) return null;
  const txt = String(s).trim();
  // Already ISO?
  if (/T/.test(txt)) {
    return txt.endsWith('Z') ? txt.replace(/\.\d+Z$/, 'Z') : (txt + 'Z');
  }
  // SQLite's CURRENT_TIMESTAMP gives 'YYYY-MM-DD HH:MM:SS' (UTC).
  return txt.replace(' ', 'T') + 'Z';
}

function oaiIdentifier(req, m) {
  const host = req.get('host');
  // OAI uses the identifier scheme oai:<repository>:<id>
  return 'oai:' + host + ':' + m.arxiv_like_id;
}

// Wrap a body in the standard OAI-PMH envelope.
function envelope(req, verbAttr, requestParams, body) {
  const baseUrl = originFor(req) + '/oai-pmh';
  const ts = nowUTCStamp();
  // The spec says <request> echoes the caller's args; we always emit a
  // verb attribute (even on errors), so dedupe in case the caller's
  // requestParams already includes one.
  const params = { ...(requestParams || {}) };
  if (verbAttr) params.verb = verbAttr;
  const reqAttrs = [];
  for (const [k, v] of Object.entries(params)) {
    if (v != null) reqAttrs.push(xmlEscape(k) + '="' + xmlEscape(v) + '"');
  }
  return [
    '<?xml version="1.0" encoding="UTF-8"?>',
    '<OAI-PMH xmlns="http://www.openarchives.org/OAI/2.0/"',
    '         xmlns:xsi="http://www.w3.org/2001/XMLSchema-instance"',
    '         xsi:schemaLocation="http://www.openarchives.org/OAI/2.0/ http://www.openarchives.org/OAI/2.0/OAI-PMH.xsd">',
    '  <responseDate>' + ts + '</responseDate>',
    '  <request ' + reqAttrs.join(' ') + '>' + xmlEscape(baseUrl) + '</request>',
    body,
    '</OAI-PMH>',
  ].join('\n');
}

// Error response.
function oaiError(req, code, message, requestParams = {}) {
  return envelope(req, requestParams.verb || '', requestParams,
    '  <error code="' + xmlEscape(code) + '">' + xmlEscape(message) + '</error>');
}

function fetchManuscriptByOaiId(req, oaiId) {
  const host = req.get('host');
  const prefix = 'oai:' + host + ':';
  if (!oaiId || !oaiId.startsWith(prefix)) return null;
  const local = oaiId.slice(prefix.length);
  return db.prepare(`SELECT * FROM manuscripts WHERE arxiv_like_id = ?`).get(local);
}

// ─── verb implementations ───────────────────────────────────────────────────
function verbIdentify(req) {
  const baseUrl = originFor(req) + '/oai-pmh';
  // earliestDatestamp is the oldest manuscript created_at, or "now" if none.
  const earliestRow = db.prepare(`SELECT MIN(created_at) AS t FROM manuscripts`).get();
  const earliest = earliestRow && earliestRow.t ? isoStampFromSqlite(earliestRow.t) : nowUTCStamp();
  const body = [
    '  <Identify>',
    '    <repositoryName>PreXiv</repositoryName>',
    '    <baseURL>' + xmlEscape(baseUrl) + '</baseURL>',
    '    <protocolVersion>2.0</protocolVersion>',
    '    <adminEmail>noreply@prexiv.local</adminEmail>',
    '    <earliestDatestamp>' + earliest + '</earliestDatestamp>',
    '    <deletedRecord>persistent</deletedRecord>',
    '    <granularity>YYYY-MM-DDThh:mm:ssZ</granularity>',
    '    <description>',
    '      <oai-identifier xmlns="http://www.openarchives.org/OAI/2.0/oai-identifier"',
    '                      xmlns:xsi="http://www.w3.org/2001/XMLSchema-instance"',
    '                      xsi:schemaLocation="http://www.openarchives.org/OAI/2.0/oai-identifier http://www.openarchives.org/OAI/2.0/oai-identifier.xsd">',
    '        <scheme>oai</scheme>',
    '        <repositoryIdentifier>' + xmlEscape(req.get('host')) + '</repositoryIdentifier>',
    '        <delimiter>:</delimiter>',
    '        <sampleIdentifier>oai:' + xmlEscape(req.get('host')) + ':prexiv:2605.43390</sampleIdentifier>',
    '      </oai-identifier>',
    '    </description>',
    '  </Identify>',
  ].join('\n');
  return envelope(req, 'Identify', { verb: 'Identify' }, body);
}

function verbListMetadataFormats(req) {
  const body = [
    '  <ListMetadataFormats>',
    '    <metadataFormat>',
    '      <metadataPrefix>oai_dc</metadataPrefix>',
    '      <schema>http://www.openarchives.org/OAI/2.0/oai_dc.xsd</schema>',
    '      <metadataNamespace>http://www.openarchives.org/OAI/2.0/oai_dc/</metadataNamespace>',
    '    </metadataFormat>',
    '  </ListMetadataFormats>',
  ].join('\n');
  return envelope(req, 'ListMetadataFormats', { verb: 'ListMetadataFormats' }, body);
}

function rowsForListing(metadataPrefix) {
  // Cap at 100 (no resumption tokens).
  return db.prepare(`
    SELECT m.*, u.username AS submitter_username, u.display_name AS submitter_display
    FROM manuscripts m JOIN users u ON u.id = m.submitter_id
    ORDER BY m.created_at DESC
    LIMIT ${RESPONSE_CAP}
  `).all();
}

function recordHeader(req, m) {
  const status = m.withdrawn ? ' status="deleted"' : '';
  const stamp = isoStampFromSqlite(m.withdrawn_at || m.updated_at || m.created_at) || nowUTCStamp();
  return [
    '      <header' + status + '>',
    '        <identifier>' + xmlEscape(oaiIdentifier(req, m)) + '</identifier>',
    '        <datestamp>' + stamp + '</datestamp>',
    '      </header>',
  ].join('\n');
}

function dublinCoreXml(req, m) {
  const url = originFor(req) + '/m/' + m.arxiv_like_id;
  const creators = String(m.authors || '')
    .split(/\s*(?:;| and |&)\s*/i)
    .map(s => s.trim())
    .filter(Boolean);
  const lines = [];
  lines.push('        <oai_dc:dc xmlns:oai_dc="http://www.openarchives.org/OAI/2.0/oai_dc/"');
  lines.push('                   xmlns:dc="http://purl.org/dc/elements/1.1/"');
  lines.push('                   xmlns:xsi="http://www.w3.org/2001/XMLSchema-instance"');
  lines.push('                   xsi:schemaLocation="http://www.openarchives.org/OAI/2.0/oai_dc/ http://www.openarchives.org/OAI/2.0/oai_dc.xsd">');
  lines.push('          <dc:title>' + xmlEscape(m.title) + '</dc:title>');
  for (const c of creators) lines.push('          <dc:creator>' + xmlEscape(c) + '</dc:creator>');
  if (m.abstract) lines.push('          <dc:description>' + xmlEscape(m.abstract) + '</dc:description>');
  lines.push('          <dc:date>' + (isoStampFromSqlite(m.created_at) || '') + '</dc:date>');
  lines.push('          <dc:identifier>' + xmlEscape(url) + '</dc:identifier>');
  lines.push('          <dc:type>Preprint</dc:type>');
  if (m.doi) lines.push('          <dc:relation>doi:' + xmlEscape(m.doi) + '</dc:relation>');
  if (m.category) lines.push('          <dc:subject>' + xmlEscape(m.category) + '</dc:subject>');
  lines.push('          <dc:publisher>PreXiv</dc:publisher>');
  lines.push('        </oai_dc:dc>');
  return lines.join('\n');
}

function verbListIdentifiers(req, params) {
  if (params.metadataPrefix !== 'oai_dc') {
    return oaiError(req, 'cannotDisseminateFormat',
      'Only oai_dc is supported.', { verb: 'ListIdentifiers', ...params });
  }
  const rows = rowsForListing(params.metadataPrefix);
  if (!rows.length) {
    return oaiError(req, 'noRecordsMatch', 'No records.', { verb: 'ListIdentifiers', ...params });
  }
  const body = ['  <ListIdentifiers>',
    ...rows.map(m => recordHeader(req, m)),
    '  </ListIdentifiers>'].join('\n');
  return envelope(req, 'ListIdentifiers', { verb: 'ListIdentifiers', ...params }, body);
}

function verbListRecords(req, params) {
  if (params.metadataPrefix !== 'oai_dc') {
    return oaiError(req, 'cannotDisseminateFormat',
      'Only oai_dc is supported.', { verb: 'ListRecords', ...params });
  }
  const rows = rowsForListing(params.metadataPrefix);
  if (!rows.length) {
    return oaiError(req, 'noRecordsMatch', 'No records.', { verb: 'ListRecords', ...params });
  }
  const records = rows.map(m => {
    const head = recordHeader(req, m);
    if (m.withdrawn) {
      // Deleted records carry only the header.
      return ['    <record>', head, '    </record>'].join('\n');
    }
    return [
      '    <record>',
      head,
      '      <metadata>',
      dublinCoreXml(req, m),
      '      </metadata>',
      '    </record>',
    ].join('\n');
  });
  const body = ['  <ListRecords>', ...records, '  </ListRecords>'].join('\n');
  return envelope(req, 'ListRecords', { verb: 'ListRecords', ...params }, body);
}

function verbGetRecord(req, params) {
  if (!params.identifier) {
    return oaiError(req, 'badArgument', 'identifier is required.',
      { verb: 'GetRecord', ...params });
  }
  if (params.metadataPrefix !== 'oai_dc') {
    return oaiError(req, 'cannotDisseminateFormat',
      'Only oai_dc is supported.', { verb: 'GetRecord', ...params });
  }
  const m = fetchManuscriptByOaiId(req, params.identifier);
  if (!m) {
    return oaiError(req, 'idDoesNotExist',
      'No record with identifier ' + params.identifier,
      { verb: 'GetRecord', ...params });
  }
  let record;
  if (m.withdrawn) {
    record = ['    <record>', recordHeader(req, m), '    </record>'].join('\n');
  } else {
    record = [
      '    <record>',
      recordHeader(req, m),
      '      <metadata>',
      dublinCoreXml(req, m),
      '      </metadata>',
      '    </record>',
    ].join('\n');
  }
  const body = ['  <GetRecord>', record, '  </GetRecord>'].join('\n');
  return envelope(req, 'GetRecord', { verb: 'GetRecord', ...params }, body);
}

function handleOaiRequest(req, res) {
  res.set('Content-Type', 'text/xml; charset=utf-8');
  // OAI-PMH allows GET and POST; we accept both.
  const params = req.method === 'POST' ? (req.body || {}) : (req.query || {});
  const flat = {};
  for (const [k, v] of Object.entries(params)) flat[k] = Array.isArray(v) ? v[0] : v;
  const verb = flat.verb;

  switch (verb) {
    case 'Identify':
      return res.send(verbIdentify(req));
    case 'ListMetadataFormats':
      return res.send(verbListMetadataFormats(req));
    case 'ListIdentifiers':
      return res.send(verbListIdentifiers(req, flat));
    case 'ListRecords':
      return res.send(verbListRecords(req, flat));
    case 'GetRecord':
      return res.send(verbGetRecord(req, flat));
    default:
      return res.send(oaiError(req,
        verb ? 'badVerb' : 'badVerb',
        verb ? 'Unsupported verb: ' + verb : 'verb argument is required.',
        flat));
  }
}

module.exports = { handleOaiRequest };
