// Welcome modal — shown on every visit to the homepage unless the
// visitor has explicitly opted out via the "Don't show this again"
// checkbox.
//
// The markup is rendered into the homepage with `hidden=true`, so the
// content stays invisible until the script runs. Default behaviour
// (checkbox unchecked at dismiss time): no persistence — the modal
// reappears on every visit. Opt-out behaviour (checkbox checked at
// dismiss time): we write a localStorage flag and skip showing the
// modal on subsequent loads.
//
// Storage key is versioned so we can re-engage the explainer for
// returning visitors if we update the wording meaningfully — bump the
// suffix and the suppression from the old key becomes inert.
(function () {
  'use strict';

  var SUPPRESS_KEY = 'prexiv:welcome-suppress-v1';
  var modal = document.getElementById('welcome-modal');
  if (!modal) return;

  // Has the user previously opted out for this version of the welcome?
  // Errors (e.g. localStorage blocked in private mode) → fall through
  // and show the modal; the explainer is more important than perfectly
  // honouring an opt-out we can't read.
  try {
    if (window.localStorage && localStorage.getItem(SUPPRESS_KEY) === '1') return;
  } catch (e) { /* show */ }

  var suppressCheckbox = document.getElementById('welcome-suppress');
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

    // Persist the opt-out only if the user explicitly checked the box.
    // Every dismiss path (X / Got it / backdrop / Escape) routes through
    // here, so the checkbox state is honoured uniformly.
    if (suppressCheckbox && suppressCheckbox.checked) {
      try {
        if (window.localStorage) localStorage.setItem(SUPPRESS_KEY, '1');
      } catch (e) { /* best-effort */ }
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
      'a[href], button:not([disabled]), input:not([disabled]), [tabindex]:not([tabindex="-1"])'
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
