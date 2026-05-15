(function () {
  function request(method, url, body, targetSelector, swap) {
    var target = document.querySelector(targetSelector || 'body');
    if (!target) return;
    target.classList.add('is-loading');
    fetch(url, {
      method: method,
      body: body,
      headers: { 'Accept': 'text/html' }
    }).then(function (response) {
      if (!response.ok) {
        return response.text().then(function (text) { throw new Error(text || response.statusText); });
      }
      return response.text();
    }).then(function (html) {
      if ((swap || '').toLowerCase() === 'outerhtml') {
        target.outerHTML = html;
      } else {
        target.innerHTML = html;
      }
      process(document);
      document.dispatchEvent(new CustomEvent('tli:content-updated'));
    }).catch(function (error) {
      window.alert(error.message);
    }).finally(function () {
      var next = document.querySelector(targetSelector || 'body');
      if (next) next.classList.remove('is-loading');
    });
  }

  function process(root) {
    root.querySelectorAll('[hx-trigger~="load"][hx-get]:not([data-hx-loaded])').forEach(function (el) {
      el.setAttribute('data-hx-loaded', 'true');
      request('GET', el.getAttribute('hx-get'), null, selectorFor(el), el.getAttribute('hx-swap'));
    });
  }

  function selectorFor(el) {
    return el.getAttribute('hx-target') || (el.id ? '#' + el.id : 'body');
  }

  document.addEventListener('submit', function (event) {
    var form = event.target.closest('form[hx-post]');
    if (!form) return;
    event.preventDefault();
    var body = new URLSearchParams();
    Array.from(new FormData(form).entries()).forEach(function (entry) {
      if (entry[1] !== '') body.append(entry[0], entry[1]);
    });
    request('POST', form.getAttribute('hx-post'), body, selectorFor(form), form.getAttribute('hx-swap'));
  });

  document.addEventListener('click', function (event) {
    var el = event.target.closest('[hx-get]:not([hx-trigger~="load"]), [hx-post]:not(form)');
    if (!el) return;
    event.preventDefault();
    var method = el.hasAttribute('hx-post') ? 'POST' : 'GET';
    request(method, el.getAttribute('hx-post') || el.getAttribute('hx-get'), null, selectorFor(el), el.getAttribute('hx-swap'));
  });

  document.addEventListener('DOMContentLoaded', function () { process(document); });
  window.htmx = { process: process };
})();
