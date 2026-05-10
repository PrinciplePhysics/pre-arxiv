// pre-arxiv — small bits of progressive enhancement
(function () {
  'use strict';

  // Reply toggle for nested comments
  document.addEventListener('click', function (e) {
    const t = e.target;
    if (t.classList && t.classList.contains('reply-toggle')) {
      e.preventDefault();
      const id = t.getAttribute('data-target');
      const el = document.getElementById(id);
      if (el) {
        const isHidden = el.style.display === 'none' || !el.style.display;
        el.style.display = isHidden ? 'block' : 'none';
        if (isHidden) {
          const ta = el.querySelector('textarea');
          if (ta) ta.focus();
        }
      }
    }
  });

  // Optimistic vote handling — submit via fetch and update score in place
  document.addEventListener('submit', function (e) {
    const form = e.target;
    if (!form.classList || !form.classList.contains('vote-form')) return;
    if (form.dataset.noajax) return;
    e.preventDefault();
    const url = form.getAttribute('action');
    const fd = new FormData(form);
    const value = parseInt(fd.get('value'), 10);
    const csrf = fd.get('_csrf') || '';

    fetch(url, {
      method: 'POST',
      headers: { 'Accept': 'application/json', 'X-CSRF-Token': csrf },
      body: new URLSearchParams([['value', String(value)], ['_csrf', csrf]]),
      credentials: 'same-origin',
    }).then(r => {
      if (r.status === 401 || r.status === 302 || r.redirected) {
        // not logged in — go to login
        window.location.href = '/login?next=' + encodeURIComponent(window.location.pathname);
        return null;
      }
      return r.json().catch(() => null);
    }).then(data => {
      if (!data) return;
      // update score in place
      const row = form.closest('.ms-row, .comment, .ms-actions-bar, .ms-actions-right, .manuscript');
      if (row) {
        const sEl = row.querySelector('.vote-score, .cvote-score, .score-pill');
        if (sEl) {
          if (sEl.classList.contains('score-pill')) sEl.textContent = data.score + ' pts';
          else sEl.textContent = data.score;
        }
        // mark voted state
        const ups = row.querySelectorAll('.vote-up');
        const dns = row.querySelectorAll('.vote-dn');
        ups.forEach(b => b.classList.toggle('voted', data.myVote === 1));
        dns.forEach(b => b.classList.toggle('voted', data.myVote === -1));
      }
    }).catch(() => {
      form.dataset.noajax = '1';
      form.submit();
    });
  });
})();
