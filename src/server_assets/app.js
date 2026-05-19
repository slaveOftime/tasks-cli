window.addEventListener('keydown', function (event) {
  if ((event.ctrlKey || event.metaKey) && event.key === 'r') {
    event.preventDefault();
    var board = document.querySelector('#board');
    if (board && window.htmx) {
      board.removeAttribute('data-hx-loaded');
      window.htmx.process(document);
    }
  }
});

function formHasFilledValue(form) {
  return Array.from(form.querySelectorAll('input, textarea, select')).some(function (field) {
    if (field.disabled || field.type === 'hidden' || field.type === 'submit' || field.type === 'button') return false;
    if (field.type === 'checkbox' || field.type === 'radio') return field.checked;
    return field.value.trim() !== '';
  });
}

function formRequiredFieldsReady(form) {
  return Array.from(form.querySelectorAll('[required]')).every(function (field) {
    return field.disabled || field.value.trim() !== '';
  });
}

function updateReadyForm(form) {
  var submit = form.querySelector('[data-ready-submit]');
  if (!submit) return;
  var ready = form.hasAttribute('data-ready-any') ? formHasFilledValue(form) : formRequiredFieldsReady(form);
  submit.hidden = !ready;
}

function updateScheduleForm(form) {
  var modeInput = form.querySelector('input[name="schedule_mode"]:checked');
  var mode = modeInput ? modeInput.value : 'interval';

  form.querySelectorAll('[data-schedule-panel]').forEach(function (panel) {
    var active = panel.getAttribute('data-schedule-panel') === mode;
    panel.hidden = !active;
    panel.querySelectorAll('input, textarea, select').forEach(function (field) {
      field.disabled = !active;
    });
  });

  var ready = false;
  if (mode === 'interval') {
    var interval = form.querySelector('[data-schedule-panel="interval"] input[name="every_minutes"]');
    ready = Boolean(interval && interval.value.trim() !== '');
  } else if (mode === 'cron') {
    var cron = form.querySelector('[data-schedule-panel="cron"] input[name="cron"]');
    ready = Boolean(cron && cron.value.trim() !== '');
  }

  var submit = form.querySelector('[data-ready-submit]');
  if (submit) submit.hidden = !ready;
}

function initializeForms(root) {
  root.querySelectorAll('[data-ready-form]').forEach(updateReadyForm);
  root.querySelectorAll('[data-schedule-form]').forEach(updateScheduleForm);
}

function syncScrollTopButton() {
  var button = document.querySelector('[data-scroll-top]');
  if (!button) return;
  button.setAttribute('data-visible', window.scrollY > 280 ? 'true' : 'false');
}

document.addEventListener('click', function (event) {
  var scrollTop = event.target.closest('[data-scroll-top]');
  if (scrollTop) {
    window.scrollTo({ top: 0, behavior: 'smooth' });
    return;
  }

  var opener = event.target.closest('[data-dialog-open]');
  if (opener) {
    var dialog = document.getElementById(opener.getAttribute('data-dialog-open'));
    if (dialog && typeof dialog.showModal === 'function') {
      dialog.showModal();
    }
    return;
  }

  var closer = event.target.closest('[data-dialog-close]');
  if (closer) {
    var openDialog = closer.closest('dialog');
    if (openDialog) openDialog.close();
    return;
  }

  if (event.target instanceof HTMLDialogElement) {
    event.target.close();
  }
});

document.addEventListener('input', function (event) {
  var readyForm = event.target.closest('[data-ready-form]');
  if (readyForm) updateReadyForm(readyForm);

  var scheduleForm = event.target.closest('[data-schedule-form]');
  if (scheduleForm) updateScheduleForm(scheduleForm);
});

document.addEventListener('change', function (event) {
  var readyForm = event.target.closest('[data-ready-form]');
  if (readyForm) updateReadyForm(readyForm);

  var scheduleForm = event.target.closest('[data-schedule-form]');
  if (scheduleForm) updateScheduleForm(scheduleForm);
});

document.addEventListener('DOMContentLoaded', function () {
  initializeForms(document);
  syncScrollTopButton();
});

document.addEventListener('tli:content-updated', function () {
  initializeForms(document);
  syncScrollTopButton();
});

window.addEventListener('scroll', syncScrollTopButton, { passive: true });
