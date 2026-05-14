// Shared PreXiv progressive enhancement.
(function () {
  'use strict';

  function initUploadDropzones() {
    document.querySelectorAll('.upload-dropzone').forEach(function (zone) {
      if (zone.dataset.uploadInit === '1') return;
      zone.dataset.uploadInit = '1';
      var input = zone.querySelector('.upload-input');
      var output = document.getElementById(zone.dataset.boundName);
      if (!input || !output) return;
      var empty = output.dataset.empty || 'No file selected';
      input.addEventListener('change', function () {
        var file = input.files && input.files[0];
        if (file) {
          output.textContent = file.name + ' · ' + (file.size / 1024 / 1024).toFixed(2) + ' MB';
          output.classList.add('has-file');
        } else {
          output.textContent = empty;
          output.classList.remove('has-file');
        }
      });
    });
  }

  function initSourceChoiceFallback() {
    document.querySelectorAll('.source-choice-section').forEach(function (section) {
      if (section.dataset.sourceChoiceInit === '1') return;
      section.dataset.sourceChoiceInit = '1';
      var inputs = section.querySelectorAll('input[name="source_type"]');
      if (!inputs.length) return;
      function sync() {
        var checked = section.querySelector('input[name="source_type"]:checked');
        var value = checked ? checked.value : 'tex';
        section.classList.toggle('source-choice-tex', value === 'tex');
        section.classList.toggle('source-choice-pdf', value === 'pdf');
      }
      inputs.forEach(function (input) {
        input.addEventListener('change', sync);
      });
      sync();
    });
  }

  function initConfirmForms() {
    document.addEventListener('submit', function (event) {
      var form = event.target;
      if (!form || !form.getAttribute) return;
      var message = form.getAttribute('data-confirm');
      if (!message) return;
      if (!window.confirm(message)) {
        event.preventDefault();
      }
    });
  }

  initUploadDropzones();
  initSourceChoiceFallback();
  initConfirmForms();
})();
