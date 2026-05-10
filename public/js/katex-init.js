// Render LaTeX math anywhere in the page once both katex.js and auto-render.js
// have loaded. Defer-loaded so this file runs after both KaTeX scripts have
// executed; if for some reason renderMathInElement isn't yet available we
// retry briefly.
(function () {
  'use strict';

  const opts = {
    delimiters: [
      { left: '$$', right: '$$', display: true  },
      { left: '\\[', right: '\\]', display: true  },
      { left: '$',  right: '$',  display: false },
      { left: '\\(', right: '\\)', display: false },
    ],
    throwOnError: false,
    ignoredTags:    ['script', 'noscript', 'style', 'textarea', 'pre', 'code', 'option'],
    ignoredClasses: ['no-katex'],
  };

  function tryRender(root) {
    if (typeof window.renderMathInElement === 'function') {
      try { window.renderMathInElement(root || document.body, opts); }
      catch (e) { console.warn('[katex] render error:', e); }
      return true;
    }
    return false;
  }

  // Initial render: retry briefly if auto-render hasn't installed itself yet.
  let attempts = 0;
  function init() {
    if (tryRender(document.body)) return;
    if (++attempts < 40) setTimeout(init, 50);
  }

  if (document.readyState === 'loading') {
    document.addEventListener('DOMContentLoaded', init);
  } else {
    init();
  }

  // Re-render any subtree the page wants to typeset later (e.g. after AJAX).
  // Usage from app.js: window.preXivRenderMath(el)
  window.preXivRenderMath = tryRender;
})();
