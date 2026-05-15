(function () {
  var pendingTriggers = new WeakMap();
  var pendingRequests = new Map();

  function request(method, url, body, targetSelector, swap) {
    method = (method || 'GET').toUpperCase();
    if (method === 'GET' && body) {
      url = appendQuery(url, body);
      body = null;
    }
    var selector = targetSelector || 'body';
    var target = document.querySelector(selector);
    if (!target) return;
    var controller = typeof AbortController === 'function' ? new AbortController() : null;
    var token = {};
    var prior = pendingRequests.get(selector);
    if (prior && prior.controller) prior.controller.abort();
    pendingRequests.set(selector, { token: token, controller: controller });
    target.classList.add('is-loading');
    fetch(url, {
      method: method,
      body: body,
      headers: { 'Accept': 'text/html' },
      signal: controller ? controller.signal : undefined
    }).then(function (response) {
      if (!response.ok) {
        return response.text().then(function (text) { throw new Error(text || response.statusText); });
      }
      return response.text();
    }).then(function (html) {
      var current = pendingRequests.get(selector);
      if (!current || current.token !== token) return;
      var nextTarget = document.querySelector(selector);
      if (!nextTarget) return;
      if ((swap || '').toLowerCase() === 'outerhtml') {
        nextTarget.outerHTML = html;
      } else {
        nextTarget.innerHTML = html;
      }
      process(document);
      document.dispatchEvent(new CustomEvent('tli:content-updated'));
    }).catch(function (error) {
      if (error && error.name === 'AbortError') return;
      window.alert(error.message);
    }).finally(function () {
      var current = pendingRequests.get(selector);
      if (!current || current.token !== token) return;
      pendingRequests.delete(selector);
      var next = document.querySelector(selector);
      if (next) next.classList.remove('is-loading');
    });
  }

  function appendQuery(url, params) {
    if (!params || Array.from(params.entries()).length === 0) return url;
    var next = new URL(url, window.location.href);
    params.forEach(function (value, key) {
      next.searchParams.append(key, value);
    });
    return next.pathname + next.search;
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

  function paramsForElement(el) {
    if (el.matches('form')) return paramsForForm(el);
    if (el.getAttribute('hx-include') === 'closest form') {
      var form = el.closest('form');
      if (form) return paramsForForm(form);
    }
    return null;
  }

  function paramsForForm(form) {
    var body = new URLSearchParams();
    Array.from(new FormData(form).entries()).forEach(function (entry) {
      if (entry[1] !== '') body.append(entry[0], entry[1]);
    });
    return body;
  }

  function isFormControl(el) {
    return el.matches('input, textarea, select, option');
  }

  function hasTrigger(el, trigger) {
    var spec = (el.getAttribute('hx-trigger') || '').toLowerCase();
    if (!spec) return false;
    return spec.split(',').some(function (part) {
      return part.trim().split(/\s+/).indexOf(trigger) !== -1;
    });
  }

  function triggerDelay(el) {
    var spec = el.getAttribute('hx-trigger') || '';
    var match = spec.match(/delay:(\d+)ms/i);
    return match ? Number(match[1]) : 0;
  }

  function requestForElement(el) {
    var method = el.hasAttribute('hx-post') ? 'POST' : 'GET';
    request(method, el.getAttribute('hx-post') || el.getAttribute('hx-get'), paramsForElement(el), selectorFor(el), el.getAttribute('hx-swap'));
  }

  function scheduleElementRequest(el) {
    var delay = triggerDelay(el);
    var pending = pendingTriggers.get(el);
    if (pending) window.clearTimeout(pending);
    pendingTriggers.set(el, window.setTimeout(function () {
      pendingTriggers.delete(el);
      requestForElement(el);
    }, delay));
  }

  document.addEventListener('submit', function (event) {
    var form = event.target.closest('form[hx-post], form[hx-get]');
    if (!form) return;
    event.preventDefault();
    requestForElement(form);
  });

  document.addEventListener('click', function (event) {
    var el = event.target.closest('[hx-get]:not([hx-trigger~="load"]), [hx-post]:not(form)');
    if (!el) return;
    if (isFormControl(el) && el.tagName !== 'BUTTON') return;
    event.preventDefault();
    requestForElement(el);
  });

  document.addEventListener('input', function (event) {
    var el = event.target.closest('[hx-get], [hx-post]');
    if (!el || !hasTrigger(el, 'input')) return;
    scheduleElementRequest(el);
  });

  document.addEventListener('search', function (event) {
    var el = event.target.closest('[hx-get], [hx-post]');
    if (!el || !hasTrigger(el, 'search')) return;
    scheduleElementRequest(el);
  });

  document.addEventListener('DOMContentLoaded', function () { process(document); });
  window.htmx = { process: process };
})();
