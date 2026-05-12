// Welcome modal — shown on every visit to the homepage.
//
// The markup is rendered into the homepage with `hidden=true` (so the
// content is invisible until the script runs, and crawlers/screen
// readers see it announced as a dialog rather than as page content).
// On every page load the script reveals it; the user dismisses with the
// X button, the "Got it" button, a backdrop click, or the Escape key.
// We deliberately do NOT persist the dismissal — the explainer reappears
// on the next visit, matching the operator's intent that every visitor
// (returning or new) sees PreXiv's positioning before they scroll.
(function () {
  'use strict';

  var modal = document.getElementById('welcome-modal');
  if (!modal) return;

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
