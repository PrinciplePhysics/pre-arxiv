<?xml version="1.0" encoding="UTF-8"?>
<xsl:stylesheet version="1.0"
                xmlns:xsl="http://www.w3.org/1999/XSL/Transform"
                xmlns:dc="http://purl.org/dc/elements/1.1/"
                xmlns:atom="http://www.w3.org/2005/Atom">
<xsl:output method="html" indent="yes" encoding="UTF-8"
            doctype-system="about:legacy-compat"/>

<xsl:template match="/rss">
<html lang="en">
<head>
<meta charset="utf-8"/>
<meta name="viewport" content="width=device-width,initial-scale=1"/>
<title><xsl:value-of select="channel/title"/></title>
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
<h1><xsl:value-of select="channel/title"/></h1>
<p class="muted"><xsl:value-of select="channel/description"/></p>
<p class="muted small">
This is the RSS feed at <a href="{channel/atom:link/@href}"><code><xsl:value-of select="channel/atom:link/@href"/></code></a>.
Subscribe with any feed reader — your browser is rendering it as a page because raw RSS is hard to read.
</p>
</div>

<ol class="ms-list">
<xsl:for-each select="channel/item">
<li class="ms-row">
  <div class="ms-body">
    <div class="ms-title-line">
      <a class="ms-title" href="{link}"><xsl:value-of select="title"/></a>
    </div>
    <div class="ms-meta">
      <span class="ms-authors"><xsl:value-of select="dc:creator"/></span>
      <xsl:text> · </xsl:text>
      <span><xsl:value-of select="category"/></span>
      <xsl:text> · </xsl:text>
      <span class="muted"><xsl:value-of select="pubDate"/></span>
    </div>
  </div>
</li>
</xsl:for-each>
</ol>
</main>
</body>
</html>
</xsl:template>

</xsl:stylesheet>
