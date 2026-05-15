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

  function initRadioCardState() {
    var groups = {};
    document.querySelectorAll('.ctype-card input[type="radio"]').forEach(function (input) {
      if (!input.name) return;
      groups[input.name] = true;
      if (input.dataset.radioCardInit === '1') return;
      input.dataset.radioCardInit = '1';
      input.addEventListener('change', function () {
        syncRadioCardGroup(input.name);
      });
    });
    Object.keys(groups).forEach(syncRadioCardGroup);
  }

  function syncRadioCardGroup(group) {
    document.querySelectorAll('.ctype-card input[type="radio"]').forEach(function (peer) {
      if (peer.name !== group) return;
      var card = peer.closest('.ctype-card');
      if (card) {
        card.classList.toggle('is-checked', peer.checked);
      }
    });
  }

  function initConductorChoiceFallback() {
    var forms = document.querySelectorAll('.submit-form');
    forms.forEach(function (form) {
      if (form.dataset.conductorChoiceInit === '1') return;
      var inputs = form.querySelectorAll('input[name="conductor_type"]');
      if (!inputs.length) return;
      form.dataset.conductorChoiceInit = '1';

      function setSectionDisabled(selector, disabled) {
        var section = form.querySelector(selector);
        if (!section) return;
        section.querySelectorAll('input, select, textarea, button').forEach(function (el) {
          el.disabled = disabled;
        });
      }

      function sync() {
        var checked = form.querySelector('input[name="conductor_type"]:checked');
        var value = checked ? checked.value : 'human-ai';
        var isAgent = value === 'ai-agent';
        form.classList.toggle('conductor-human-ai', !isAgent);
        form.classList.toggle('conductor-ai-agent', isAgent);
        setSectionDisabled('.ctype-human-ai', isAgent);
        setSectionDisabled('.ctype-ai-agent', !isAgent);

        if (isAgent) {
          var selfAudit = form.querySelector('input[name="audit_status"][value="self"]');
          if (selfAudit && selfAudit.checked) {
            var noneAudit = form.querySelector('input[name="audit_status"][value="none"]');
            if (noneAudit) noneAudit.checked = true;
          }
        }
        syncAuditChoice(form);
        initRadioCardState();
      }

      inputs.forEach(function (input) { input.addEventListener('change', sync); });
      sync();
    });
  }

  function syncAuditChoice(scope) {
    var form = scope && scope.querySelector ? scope : document;
    form.querySelectorAll('.audit-section').forEach(function (section) {
      var checked = section.querySelector('input[name="audit_status"]:checked');
      var value = checked ? checked.value : 'none';
      section.classList.toggle('audit-choice-none', value === 'none');
      section.classList.toggle('audit-choice-self', value === 'self');
      section.classList.toggle('audit-choice-other', value === 'other');
    });
  }

  function initAuditChoiceFallback() {
    document.querySelectorAll('.audit-section').forEach(function (section) {
      if (section.dataset.auditChoiceInit === '1') return;
      section.dataset.auditChoiceInit = '1';
      section.querySelectorAll('input[name="audit_status"]').forEach(function (input) {
        input.addEventListener('change', function () {
          syncAuditChoice(document);
          initRadioCardState();
        });
      });
      syncAuditChoice(document);
    });
  }

  function initModelTagsAndReview() {
    var root = document.getElementById('ai-model-tag-input');
    if (!root || root.dataset.tagInputInit === '1') return;
    root.dataset.tagInputInit = '1';
    var hidden = document.getElementById('conductor_ai_model');
    var chips = document.getElementById('ai-model-chips');
    var typer = document.getElementById('ai-model-typer');
    if (!hidden || !chips || !typer) return;

    function uniqAppend(list, item) {
      var lower = item.toLowerCase();
      for (var i = 0; i < list.length; i++) {
        if (list[i].toLowerCase() === lower) return list;
      }
      list.push(item);
      return list;
    }
    function parseModels() {
      return (hidden.value || '').split(',').map(function (s) {
        return s.trim();
      }).filter(Boolean);
    }
    function clearChildren(node) {
      while (node.firstChild) node.removeChild(node.firstChild);
    }
    function syncFromList(list) {
      hidden.value = list.join(', ');
      clearChildren(chips);
      list.forEach(function (name, idx) {
        var chip = document.createElement('span');
        chip.className = 'tag-chip';
        var label = document.createElement('span');
        label.className = 'tag-chip-label';
        label.textContent = name;
        var remove = document.createElement('button');
        remove.type = 'button';
        remove.className = 'tag-chip-x';
        remove.setAttribute('aria-label', 'Remove ' + name);
        remove.textContent = 'x';
        remove.addEventListener('click', function () {
          var cur = parseModels();
          cur.splice(idx, 1);
          syncFromList(cur);
          updateReview();
          typer.focus();
        });
        chip.appendChild(label);
        chip.appendChild(remove);
        chips.appendChild(chip);
      });
    }
    function addFromTyper() {
      var raw = (typer.value || '').trim();
      if (!raw) return;
      var pieces = raw.split(',').map(function (s) { return s.trim(); }).filter(Boolean);
      var cur = parseModels();
      pieces.forEach(function (piece) { uniqAppend(cur, piece); });
      syncFromList(cur);
      typer.value = '';
      updateReview();
    }

    typer.addEventListener('keydown', function (event) {
      if (event.key === 'Enter' || event.key === ',') {
        event.preventDefault();
        addFromTyper();
      } else if (event.key === 'Backspace' && !typer.value) {
        var cur = parseModels();
        if (cur.length) {
          typer.value = cur.pop();
          syncFromList(cur);
          updateReview();
        }
      }
    });
    typer.addEventListener('blur', addFromTyper);

    var form = typer.closest('form');
    if (form) form.addEventListener('submit', addFromTyper);
    syncFromList(parseModels());

    var reviewRoot = document.getElementById('step-review');
    var submitForm = reviewRoot && reviewRoot.closest('form');
    if (!reviewRoot || !submitForm) return;

    function checkedValue(name) {
      var el = submitForm.querySelector('input[name="' + name + '"]:checked');
      return el ? el.value : '';
    }
    function field(name) {
      return submitForm.querySelector('[name="' + name + '"]');
    }
    function setText(id, text) {
      var el = document.getElementById(id);
      if (el) el.textContent = text;
    }
    function labelTextForChecked(name) {
      var el = submitForm.querySelector('input[name="' + name + '"]:checked');
      var card = el && el.closest('label');
      var strong = card && card.querySelector('strong');
      return strong ? strong.textContent.replace(/\s+/g, ' ').trim() : '';
    }
    function selectedOptionText(name) {
      var el = field(name);
      if (!el || !el.options || el.selectedIndex < 0) return '';
      return el.options[el.selectedIndex].textContent.replace(/\s+/g, ' ').trim();
    }
    function updateReview() {
      var sourceType = checkedValue('source_type') || 'tex';
      var conductorType = checkedValue('conductor_type') || 'human-ai';
      var auditStatus = checkedValue('audit_status') || 'none';
      var modelField = field('conductor_ai_model_public');
      var humanField = field('conductor_human_public');
      var externalField = field('external_url');
      var modelsPrivate = !!(modelField && modelField.checked);
      var humanPrivate = !!(humanField && humanField.checked);
      var hasExternal = !!(externalField && externalField.value.trim());
      var modelText = hidden.value.trim() ? hidden.value.trim() : 'not entered yet';
      var conductorField = field('conductor_human');
      var humanText = conductorField && conductorField.value.trim()
        ? conductorField.value.trim()
        : 'not entered yet';

      setText('review-production-mode', conductorType === 'ai-agent'
        ? 'Autonomous AI-agent workflow; the submitter remains responsible for lawful posting and accurate disclosure.'
        : 'Human + AI co-conductor workflow.');
      setText('review-ai-models', modelsPrivate
        ? 'hidden from public viewers as (undisclosed); saved for you and admins.'
        : 'public: ' + modelText + '.');
      setText('review-human-conductor', conductorType === 'ai-agent'
        ? 'not used for autonomous AI-agent submissions.'
        : (humanPrivate ? 'hidden from public viewers as (undisclosed).' : 'public: ' + humanText + '.'));
      setText('review-audit-status', auditStatus === 'self'
        ? 'Self-audit statement will be published with the manuscript.'
        : (auditStatus === 'other'
          ? 'Third-party auditor details and statement will be published with the manuscript.'
          : 'No auditor; the page will show an unaudited warning.'));
      setText('review-hidden-models', modelsPrivate
        ? 'AI model details will be hidden from public viewers and displayed as (undisclosed).'
        : 'AI model details are public by default.');
      setText('review-hidden-conductor', conductorType === 'ai-agent'
        ? 'No human conductor field is published for the autonomous agent option.'
        : (humanPrivate
          ? 'Human conductor name will be hidden from public viewers and displayed as (undisclosed).'
          : 'Human conductor name is public by default for Human + AI submissions.'));
      setText('review-hosted-primary', sourceType === 'pdf'
        ? 'PDF direct upload: PreXiv will host the uploaded PDF.'
        : 'LaTeX source upload: PreXiv will compile and host a PDF.');
      setText('review-hosted-source', sourceType === 'pdf'
        ? 'No PreXiv source download is created for direct PDF uploads.'
        : ((modelsPrivate || humanPrivate)
          ? 'A public source download will exist after private provenance fields are redacted.'
          : 'A source download will be hosted alongside the compiled PDF.'));
      setText('review-hosted-external', hasExternal
        ? 'External URL will be shown as a supplemental link, not as the hosted copy.'
        : 'No external URL entered; hosted PreXiv files will be the canonical downloads.');
      setText('review-license', (selectedOptionText('license') || 'CC BY 4.0') + '.');
      setText('review-training', (labelTextForChecked('ai_training') || 'Allow AI training') + '.');
    }

    submitForm.querySelectorAll('input, select, textarea').forEach(function (el) {
      el.addEventListener('input', updateReview);
      el.addEventListener('change', updateReview);
    });
    submitForm.addEventListener('submit', updateReview);
    updateReview();
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
  initRadioCardState();
  initConductorChoiceFallback();
  initAuditChoiceFallback();
  initModelTagsAndReview();
  initConfirmForms();
})();
