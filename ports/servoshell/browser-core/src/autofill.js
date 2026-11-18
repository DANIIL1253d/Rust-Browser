(function(a) {
  'use strict';
  var empty = a.mode === 'discover' ? '' : false;
  if (window !== window.top || location.origin !== a.origin) return empty;
  var bindingKey = '__rust_browser_autofill_target_v1';
  var initialDocument = document;
  function autocomplete(e) { return (e.getAttribute('autocomplete') || '').toLowerCase().split(/\s+/); }
  function visible(e) {
    if (!(e instanceof HTMLInputElement) || e.ownerDocument !== document || !e.isConnected ||
        e.disabled || e.readOnly || e.matches(':disabled') || e.type === 'hidden' ||
        e.closest('[hidden],[inert],[aria-hidden="true"]')) return false;
    var s = getComputedStyle(e), r = e.getBoundingClientRect();
    return s.display !== 'none' && s.visibility !== 'hidden' && s.visibility !== 'collapse' &&
      s.opacity !== '0' && e.getClientRects().length > 0 && r.width > 0 && r.height > 0 &&
      r.bottom > 0 && r.right > 0 && r.top < innerHeight && r.left < innerWidth;
  }
  function password(e) {
    var ac = autocomplete(e);
    return visible(e) && e.type === 'password' && ac.indexOf('new-password') < 0 && ac.indexOf('one-time-code') < 0 &&
      !/(?:^|[\s_-])(otp|one.?time|verification|code|token)(?:$|[\s_-])/i.test(e.name + ' ' + e.id);
  }
  function inputs(scope) { return Array.from(scope.querySelectorAll('input')); }
  function role(e) {
    if (!visible(e)) return '';
    var ac = autocomplete(e);
    if (ac.indexOf('new-password') >= 0 || ac.indexOf('one-time-code') >= 0 ||
        /(?:^|[\s_-])(otp|one.?time|verification|code|token)(?:$|[\s_-])/i.test(e.name + ' ' + e.id)) return '';
    if (password(e)) return 'password';
    if (e.type !== 'text' && e.type !== 'email') return '';
    var scope = e.form || document, peers = inputs(scope);
    // Do not offer an account in a registration-only form.
    if (peers.some(function(p) { return visible(p) && autocomplete(p).indexOf('new-password') >= 0; }) &&
        !peers.some(password)) return '';
    if (ac.indexOf('username') >= 0 || e.type === 'email' ||
        /(?:user(?:name)?|login|email|identifier)/i.test(e.name + ' ' + e.id)) return 'username';
    // A generic text field is a username only in a form with a current password.
    return e.form && peers.some(password) ? 'username' : '';
  }
  function choose(focused) {
    var active = document.activeElement;
    if (active && /^(IFRAME|FRAME)$/.test(active.tagName)) return null;
    if (role(active)) return active;
    if (focused) return null;
    var all = inputs(document);
    return all.find(password) || all.find(function(e) { return role(e) === 'username'; }) || null;
  }
  if (a.mode === 'discover') {
    var target = choose(a.focused);
    if (!target || typeof a.token !== 'string' || !a.token) return '';
    Object.defineProperty(window, bindingKey, {configurable:true, writable:true,
      value:{document:document, element:target, token:a.token, requireFocus:a.focused === true}});
    var rect = target.getBoundingClientRect();
    return JSON.stringify({origin:location.origin, token:a.token, role:role(target),
      rect:[rect.left,rect.top,rect.right,rect.bottom]});
  }
  if (a.mode !== 'fill') return false;
  var target;
  if (a.token) {
    var binding = window[bindingKey];
    if (!binding || binding.document !== document || binding.token !== a.token) return false;
    target = binding.element;
    if (!role(target)) return false;
    var active = document.activeElement;
    // Native focus suggestions bind one focused field. Explicit manager discovery
    // may choose a form without login focus and keeps that intentional fallback.
    if (binding.requireFocus && active !== target) return false;
    if (active && /^(IFRAME|FRAME)$/.test(active.tagName)) return false;
    if (active instanceof HTMLInputElement && active !== target && role(active)) return false;
  } else target = choose(false);
  if (!target) return false;
  var scope = target.form || document;
  var peers = inputs(scope);
  var p = role(target) === 'password' ? target : peers.find(password);
  var u = role(target) === 'username' ? target : peers.find(function(e) { return role(e) === 'username'; });
  var setter = Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, 'value').set;
  var count = 0;
  function put(e, value, expectedRole) {
    if (!e || initialDocument !== document || location.origin !== a.origin || role(e) !== expectedRole) return;
    setter.call(e, value);
    e.dispatchEvent(new Event('input', {bubbles:true}));
    e.dispatchEvent(new Event('change', {bubbles:true}));
    count++;
  }
  if (a.username) put(u, a.username, 'username');
  put(p, a.password, 'password');
  return count > 0;
})
