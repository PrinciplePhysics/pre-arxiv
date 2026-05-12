// First-visit welcome modal.
//
// The markup is rendered into the homepage with `hidden=true`, so return
// visitors never see a flash. On first visit we reveal it; once the user
// dismisses (X / "Got it" / backdrop click / Escape) we set a localStorage
// flag so it doesn't reappear on subsequent visits.
//
// localStorage key is versioned so we can re-show the modal if the wording
// is updated meaningfully later — bump the suffix.
(function () {
  'use strict';

  var SEEN_KEY = 'prexiv:welcome-seen-v1';
  var modal = document.getElementById('welcome-modal');
  if (!modal) return;

  // Bail if the user has already dismissed this version of the welcome.
  try {
    if (window.localStorage && localStorage.getItem(SEEN_KEY) === '1') return;
  } catch (e) {
    // localStorage may be blocked (private mode, strict storage settings).
    // In that case we still show the modal once per page load — better
    // than silently breaking the first-visit explainer.
  }

  var previouslyFocused = null;
  var bodyOverflowBefore = '';

  function reveal() {
    previouslyFocused = document.activeElement;
    bodyOverflowBefore = document.body.style.overflow;

    modal.hidden = false;
    modal.setAttribute('aria-hidden', 'false');
    document.body.style.overflow = 'hidden';

    // Focus the dialog box itself so screen readers announce the contents
    // and keyboard users start inside the modal.
    var box = modal.querySelector('.welcome-box');
    if (box) {
      try { box.focus({ preventScroll: true }); } catch (e) { box.focus(); }
    }
  }

  function dismiss() {
    if (modal.hidden) return;
    modal.hidden = true;
    modal.setAttribute('aria-hidden', 'true');
    document.body.style.overflow = bodyOverflowBefore;

    try {
      if (window.localStorage) localStorage.setItem(SEEN_KEY, '1');
    } catch (e) {
      // ignored — re-displaying on next visit is acceptable failure mode.
    }

    if (previouslyFocused && typeof previouslyFocused.focus === 'function') {
      try { previouslyFocused.focus({ preventScroll: true }); } catch (e) {}
    }
  }

  // Wire all dismiss controls (close X, "Got it", backdrop).
  var dismissEls = modal.querySelectorAll('[data-welcome-dismiss]');
  for (var i = 0; i < dismissEls.length; i++) {
    dismissEls[i].addEventListener('click', function (ev) {
      ev.preventDefault();
      dismiss();
    });
  }

  // Escape key — only intercepts when the modal is open.
  document.addEventListener('keydown', function (ev) {
    if (modal.hidden) return;
    if (ev.key === 'Escape' || ev.key === 'Esc') {
      ev.preventDefault();
      dismiss();
    }
  });

  // Trap focus inside the modal while open: Tab from the last focusable
  // element wraps back to the first, and Shift+Tab from the first wraps
  // to the last. Keeps keyboard users from accidentally landing in
  // background content they can't see while the overlay is active.
  modal.addEventListener('keydown', function (ev) {
    if (ev.key !== 'Tab' || modal.hidden) return;
    var focusables = modal.querySelectorAll(
      'a[href], button:not([disabled]), [tabindex]:not([tabindex="-1"])'
    );
    if (focusables.length === 0) return;
    var first = focusables[0];
    var last = focusables[focusables.length - 1];
    if (ev.shiftKey && document.activeElement === first) {
      ev.preventDefault();
      last.focus();
    } else if (!ev.shiftKey && document.activeElement === last) {
      ev.preventDefault();
      first.focus();
    }
  });

  reveal();
})();
