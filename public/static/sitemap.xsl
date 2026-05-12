<?xml version="1.0" encoding="UTF-8"?>
<xsl:stylesheet version="1.0"
                xmlns:xsl="http://www.w3.org/1999/XSL/Transform"
                xmlns:s="http://www.sitemaps.org/schemas/sitemap/0.9"
                exclude-result-prefixes="s">
<xsl:output method="html" indent="yes" encoding="UTF-8"
            doctype-system="about:legacy-compat"/>

<xsl:template match="/">
<html lang="en">
<head>
<meta charset="utf-8"/>
<meta name="viewport" content="width=device-width,initial-scale=1"/>
<title>Sitemap · PreXiv</title>
<meta name="robots" content="noindex"/>
<link rel="stylesheet" href="/static/css/style.css?v=20260512j"/>
<link rel="stylesheet" href="/static/css/prexiv-rust.css?v=20260512j"/>
</head>
<body>
<header class="topbar"><div class="topbar-inner">
<a class="brand" href="/" aria-label="PreXiv home">
<span class="brand-mark"><svg viewBox="0 0 64 64" width="32" height="32" aria-hidden="true"><rect width="64" height="64" rx="12" fill="#fff"/><path d="M 14 14 L 50 50" stroke="#b8430a" stroke-width="8" stroke-linecap="round"/><path d="M 50 14 L 14 50" stroke="#b8430a" stroke-width="3.5" stroke-linecap="round"/><circle cx="32" cy="32" r="2.6" fill="#fff"/></svg></span>
<span class="brand-name"><span class="bp">Pre</span><span class="bx">X</span><span class="bi">iv</span></span>
</a>
</div></header>
<main class="container">
<div class="page-header">
<h1>Sitemap</h1>
<p class="muted">
Machine-readable site index for search engines and harvesters. This page is the same XML
file at <a href="/sitemap.xml"><code>/sitemap.xml</code></a> — your browser is rendering it
with our XSL stylesheet because raw XML is hard to read. Indexers ignore the stylesheet
and parse the XML directly.
</p>
<p class="muted">
<strong><xsl:value-of select="count(s:urlset/s:url)"/></strong> URLs.
</p>
</div>

<table class="sitemap-table">
<thead><tr><th>URL</th><th>Priority</th><th>Last modified</th></tr></thead>
<tbody>
<xsl:for-each select="s:urlset/s:url">
<tr>
  <td><a href="{s:loc}"><xsl:value-of select="s:loc"/></a></td>
  <td class="num"><xsl:value-of select="s:priority"/></td>
  <td class="muted small"><xsl:value-of select="s:lastmod"/></td>
</tr>
</xsl:for-each>
</tbody>
</table>
</main>
</body>
</html>
</xsl:template>

</xsl:stylesheet>
