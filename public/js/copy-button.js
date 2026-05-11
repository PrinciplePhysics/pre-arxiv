// Copy-to-clipboard for any <pre> wrapped in .copy-pre-wrap.
//
// HTML pattern that this script binds to (rendered by templates that
// want a copy affordance on a code block):
//
//   <div class="copy-pre-wrap">
//     <button type="button" class="copy-pre-btn">Copy prompt</button>
//     <pre>...the content to be copied...</pre>
//   </div>
//
// Behaviour:
//   - Click → navigator.clipboard.writeText(<pre>.innerText).
//   - On success: button text flips to "✓ Copied", .copied class added
//     for green styling, reverts to the original label after 2s.
//   - On failure (clipboard API rejected — e.g. insecure context, or
//     user denied permission): button flips to "Copy failed" with
//     .failed class for amber styling; reverts after 2.5s.
//   - Falls back to document.execCommand('copy') on very old browsers
//     that lack navigator.clipboard. Idempotent re-init across HMR /
//     multiple loads.
(function () {
  'use strict';

  document.querySelectorAll('.copy-pre-wrap').forEach(function (wrap) {
    if (wrap.dataset.copyInit === '1') return; // re-entrancy guard
    wrap.dataset.copyInit = '1';

    var btn = wrap.querySelector('.copy-pre-btn');
    var pre = wrap.querySelector('pre');
    if (!btn || !pre) return;

    var originalLabel = btn.textContent;
    var resetTimer = null;

    btn.addEventListener('click', function () {
      if (resetTimer) { clearTimeout(resetTimer); resetTimer = null; }
      var text = pre.innerText;
      attempt(text).then(showCopied, showFailed);
    });

    function attempt(text) {
      if (navigator.clipboard && window.isSecureContext !== false) {
        return navigator.clipboard.writeText(text);
      }
      // Fallback for plain-http or stricter old browsers.
      return new Promise(function (resolve, reject) {
        try {
          var range = document.createRange();
          range.selectNodeContents(pre);
          var sel = window.getSelection();
          sel.removeAllRanges();
          sel.addRange(range);
          var ok = document.execCommand('copy');
          sel.removeAllRanges();
          ok ? resolve() : reject(new Error('execCommand returned false'));
        } catch (e) { reject(e); }
      });
    }

    function showCopied() {
      btn.textContent = '✓ Copied';
      btn.classList.add('copied');
      btn.classList.remove('failed');
      resetTimer = setTimeout(reset, 2000);
    }
    function showFailed() {
      btn.textContent = 'Copy failed';
      btn.classList.add('failed');
      btn.classList.remove('copied');
      resetTimer = setTimeout(reset, 2500);
    }
    function reset() {
      btn.textContent = originalLabel;
      btn.classList.remove('copied');
      btn.classList.remove('failed');
      resetTimer = null;
    }
  });
})();
