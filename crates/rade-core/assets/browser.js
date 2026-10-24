// RADE Browser Pane automation (PRD R18, ADR-0011; also the Chromium
// engine's, R72 / ADR-0026). Evaluated in the
// isolated script world "rade": it shares the page's DOM but not its
// JavaScript globals, so page scripts can neither see nor patch the ref
// table or these functions (and DOM setters here are the native ones).
// Refs: every element an Agent can act on gets `e<n>` the first time a
// snapshot sees it and keeps it until the page navigates.
(function () {
  if (window.__rade) return;
  var ids = new WeakMap(); // Element -> ref
  var els = new Map(); // ref -> WeakRef(Element)
  var next = 0;
  var MAX = 1500; // nodes per snapshot

  function refOf(el) {
    var r = ids.get(el);
    if (!r) {
      r = 'e' + ++next;
      ids.set(el, r);
      els.set(r, new WeakRef(el));
    }
    return r;
  }
  function byRef(r) {
    var w = els.get(r);
    var el = w && w.deref();
    if (!el || !el.isConnected) throw new Error('unknown ref ' + r + ' (take a new snapshot)');
    return el;
  }
  function flat(s) {
    return (s || '').replace(/\s+/g, ' ').trim();
  }
  function textOf(el) {
    return flat(el.innerText !== undefined ? el.innerText : el.textContent);
  }

  var SKIP = { SCRIPT: 1, STYLE: 1, NOSCRIPT: 1, TEMPLATE: 1, HEAD: 1, META: 1, LINK: 1 };
  var INPUT = {
    button: 'button', submit: 'button', reset: 'button', image: 'button', file: 'button',
    color: 'button', checkbox: 'checkbox', radio: 'radio', range: 'slider',
    number: 'spinbutton', search: 'searchbox'
  };
  var TAGS = {
    BUTTON: 'button', TEXTAREA: 'textbox', OPTION: 'option', NAV: 'navigation', MAIN: 'main',
    FORM: 'form', DIALOG: 'dialog', UL: 'list', OL: 'list', LI: 'listitem', TABLE: 'table',
    TR: 'row', TH: 'columnheader', TD: 'cell', SUMMARY: 'button', HEADER: 'banner',
    FOOTER: 'contentinfo', ASIDE: 'complementary', PROGRESS: 'progressbar',
    FIELDSET: 'group', ARTICLE: 'article', IFRAME: 'iframe'
  };
  var INTERACTIVE = {
    link: 1, button: 1, textbox: 1, searchbox: 1, checkbox: 1, radio: 1, combobox: 1,
    listbox: 1, option: 1, slider: 1, spinbutton: 1, 'switch': 1, tab: 1, menuitem: 1,
    menuitemcheckbox: 1, menuitemradio: 1, treeitem: 1, clickable: 1
  };
  // Named by their content; their children are not listed.
  var LEAF = {
    link: 1, button: 1, heading: 1, option: 1, tab: 1, menuitem: 1, menuitemcheckbox: 1,
    menuitemradio: 1, treeitem: 1, 'switch': 1, checkbox: 1, radio: 1, clickable: 1,
    img: 1, textbox: 1, searchbox: 1, combobox: 1, slider: 1, spinbutton: 1,
    progressbar: 1, iframe: 1
  };

  function implicitRole(el) {
    var r = el.getAttribute('role');
    if (r) {
      r = r.split(' ')[0];
      return r === 'presentation' || r === 'none' || r === 'generic' ? null : r;
    }
    var t = el.tagName;
    if ((t === 'A' || t === 'AREA') && el.hasAttribute('href')) return 'link';
    if (t === 'INPUT') return el.type === 'hidden' ? null : INPUT[el.type] || 'textbox';
    if (t === 'SELECT') return el.multiple || el.size > 1 ? 'listbox' : 'combobox';
    if (/^H[1-6]$/.test(t)) return 'heading';
    if (t === 'IMG') return el.alt ? 'img' : null;
    if (t === 'SECTION') return el.hasAttribute('aria-label') ? 'region' : null;
    if (TAGS[t]) return TAGS[t];
    if (el.isContentEditable && !(el.parentElement && el.parentElement.isContentEditable)) return 'textbox';
    return null;
  }
  function role(el, cs, parentPointer) {
    var r = implicitRole(el);
    if (r || el.tagName === 'LABEL') return r; // a label's control carries it
    // Something clickable without a role: an onclick, a tab stop, or the
    // top of a pointer-cursor region (SPAs bind clicks in JS).
    if (el.hasAttribute('onclick') || el.getAttribute('tabindex') === '0') return 'clickable';
    if (cs.cursor === 'pointer' && !parentPointer) return 'clickable';
    return null;
  }
  var CONTROLS = 'a[href],button,input,select,textarea,summary,[role],[onclick],[tabindex],[contenteditable]';

  function name(el, r) {
    var a = el.getAttribute('aria-label');
    if (a) return a;
    var lb = el.getAttribute('aria-labelledby');
    if (lb) {
      var t = lb.split(/\s+/).map(function (id) {
        var x = document.getElementById(id);
        return x ? textOf(x) : '';
      }).join(' ').trim();
      if (t) return t;
    }
    var tag = el.tagName;
    if (tag === 'INPUT' && /^(button|submit|reset)$/.test(el.type))
      return el.value || (el.type === 'submit' ? 'Submit' : el.type === 'reset' ? 'Reset' : '');
    if (tag === 'IMG' || (tag === 'INPUT' && el.type === 'image')) return el.alt || '';
    if (el.labels && el.labels.length)
      return Array.prototype.map.call(el.labels, textOf).join(' ');
    if (r === 'textbox' || r === 'searchbox' || r === 'combobox' || r === 'spinbutton' || r === 'slider' || r === 'listbox')
      return el.getAttribute('placeholder') || el.title || '';
    if (r === 'iframe') return el.title || el.getAttribute('src') || '';
    if (LEAF[r]) return textOf(el) || el.title || '';
    return el.title || '';
  }

  function hidden(el, cs) {
    return el.hidden || el.getAttribute('aria-hidden') === 'true' ||
      cs.display === 'none' || cs.visibility === 'hidden' || cs.visibility === 'collapse';
  }

  function node(el, r, d) {
    var n = { d: d, role: r, name: name(el, r) };
    if (INTERACTIVE[r]) n.ref = refOf(el);
    var tag = el.tagName;
    if (r === 'heading') n.level = +(el.getAttribute('aria-level') || tag.slice(1)) || undefined;
    if (r === 'textbox' || r === 'searchbox' || r === 'spinbutton' || r === 'slider') {
      if ('value' in el && tag !== 'BUTTON') n.value = el.type === 'password' && el.value ? '••••' : el.value;
      else if (el.isContentEditable) n.value = textOf(el);
    }
    if (r === 'combobox' && tag === 'SELECT') {
      var o = el.options[el.selectedIndex];
      n.value = o ? flat(o.text) : '';
    }
    if (r === 'checkbox' || r === 'radio' || r === 'switch' || r === 'menuitemcheckbox') {
      n.checked = 'checked' in el && tag === 'INPUT' ? el.checked : el.getAttribute('aria-checked') === 'true';
    }
    if (el.disabled || el.getAttribute('aria-disabled') === 'true') n.disabled = true;
    if (r === 'link' && el.getAttribute('href')) n.href = el.getAttribute('href');
    return n;
  }

  function snapshot() {
    var out = [];
    var omitted = 0;
    function push(n) {
      if (out.length >= MAX) omitted++;
      else out.push(n);
    }
    function text(t, d) {
      var last = out[out.length - 1];
      if (last && last.role === 'text' && last.d === d && last._open) last.name += ' ' + t;
      else push({ d: d, role: 'text', name: t, _open: true });
    }
    function walk(parent, d, pointer, skipText) {
      for (var c = parent.firstChild; c; c = c.nextSibling) {
        if (c.nodeType === 3) {
          var t = flat(c.textContent);
          if (t && !skipText) text(t, d);
          continue;
        }
        if (c.nodeType !== 1 || SKIP[c.tagName]) continue;
        var cs = getComputedStyle(c);
        if (hidden(c, cs)) continue;
        var isPointer = cs.cursor === 'pointer';
        var r = role(c, cs, pointer);
        // A label of a control: the control carries its text.
        var lbl = c.tagName === 'LABEL' && c.control;
        if (!r) {
          var last = out[out.length - 1];
          if (last) last._open = cs.display.indexOf('inline') === 0 && last._open;
          walk(c, d, pointer || isPointer, skipText || !!lbl);
          if (c.shadowRoot) walk(c.shadowRoot, d, pointer || isPointer, skipText);
          if (out.length) out[out.length - 1]._open = cs.display.indexOf('inline') === 0 && out[out.length - 1]._open;
          continue;
        }
        var n = node(c, r, d);
        // A clickable card or row holding controls: list them, not its text.
        var leaf = LEAF[r] && !(r === 'clickable' && c.querySelector(CONTROLS));
        if (!leaf && r === 'clickable') n.name = c.getAttribute('aria-label') || '';
        push(n);
        if (!leaf && r !== 'combobox') {
          walk(c, d + 1, pointer || isPointer || !!INTERACTIVE[r], skipText);
          if (c.shadowRoot) walk(c.shadowRoot, d + 1, pointer || isPointer, skipText);
        }
      }
    }
    if (document.body) walk(document.body, 0, false, false);
    out.forEach(function (n) { delete n._open; });
    return { url: location.href, title: document.title, nodes: out, omitted: omitted };
  }

  function describe(el) {
    var cs = getComputedStyle(el);
    var r = role(el, cs, false) || el.tagName.toLowerCase();
    if (!LEAF[r]) return { role: r, name: el.getAttribute('aria-label') || '' };
    return { role: r, name: name(el, r) };
  }

  function click(r) {
    var el = byRef(r);
    el.scrollIntoView({ block: 'center', inline: 'center' });
    var b = el.getBoundingClientRect();
    var o = {
      bubbles: true, cancelable: true, composed: true, view: window, button: 0,
      clientX: b.left + b.width / 2, clientY: b.top + b.height / 2
    };
    var p = Object.assign({ pointerId: 1, isPrimary: true, pointerType: 'mouse' }, o);
    el.dispatchEvent(new PointerEvent('pointerdown', Object.assign({ buttons: 1 }, p)));
    el.dispatchEvent(new MouseEvent('mousedown', Object.assign({ buttons: 1 }, o)));
    if (el.focus) el.focus({ preventScroll: true });
    el.dispatchEvent(new PointerEvent('pointerup', p));
    el.dispatchEvent(new MouseEvent('mouseup', o));
    // The click event plus the element's default (follow, submit, toggle);
    // SVG elements have no click().
    if (typeof el.click === 'function') el.click();
    else el.dispatchEvent(new MouseEvent('click', o));
    return describe(el);
  }

  var NOT_TEXT = /^(button|submit|reset|image|checkbox|radio|file|color|range|hidden)$/;

  function fill(r, value) {
    var el = byRef(r);
    el.scrollIntoView({ block: 'center' });
    if (el.focus) el.focus({ preventScroll: true });
    if (el.tagName === 'SELECT') {
      var o = Array.prototype.find.call(el.options, function (o) {
        return o.value === value || flat(o.text) === value;
      });
      if (!o) throw new Error('no option "' + value + '" in ' + r);
      el.value = o.value;
    } else if (el.tagName === 'INPUT' || el.tagName === 'TEXTAREA') {
      if (el.tagName === 'INPUT' && NOT_TEXT.test(el.type))
        throw new Error(r + ' is a ' + el.type + ' input: use click');
      el.value = value; // the native setter: frameworks see the change
    } else if (el.isContentEditable) {
      el.textContent = value;
    } else {
      throw new Error(r + ' is not a text field');
    }
    el.dispatchEvent(new Event('input', { bubbles: true, composed: true }));
    el.dispatchEvent(new Event('change', { bubbles: true }));
    return describe(el);
  }

  function isText(el) {
    return (el.tagName === 'INPUT' && !NOT_TEXT.test(el.type)) || el.tagName === 'TEXTAREA' || el.isContentEditable;
  }

  function insert(el, s) {
    if (document.execCommand('insertText', false, s)) return;
    if (el.setRangeText) {
      el.setRangeText(s, el.selectionStart, el.selectionEnd, 'end');
      el.dispatchEvent(new Event('input', { bubbles: true, composed: true }));
    }
  }

  function moveFocus(back) {
    var all = Array.prototype.filter.call(
      document.querySelectorAll('a[href],button,input,select,textarea,summary,[tabindex],[contenteditable=""],[contenteditable=true]'),
      function (e) { return e.tabIndex >= 0 && !e.disabled && e.getClientRects().length; });
    if (!all.length) return;
    var i = all.indexOf(document.activeElement);
    var n = back ? (i <= 0 ? all.length - 1 : i - 1) : (i + 1) % all.length;
    all[n].focus();
  }

  // Synthetic key events, then what the browser would do by default for
  // the keys Agents need: type into fields, Enter submits / activates,
  // Space activates, Tab moves focus, Backspace / Delete edit.
  function press(k) {
    var el = document.activeElement || document.body;
    var o = {
      key: k.key, code: k.code, ctrlKey: k.ctrl, shiftKey: k.shift, altKey: k.alt,
      metaKey: k.meta, bubbles: true, cancelable: true, composed: true
    };
    var plain = !k.ctrl && !k.meta && !k.alt;
    var go = el.dispatchEvent(new KeyboardEvent('keydown', o));
    if (go && plain && k.key.length === 1) go = el.dispatchEvent(new KeyboardEvent('keypress', o));
    if (go && plain) {
      var t = el.tagName;
      if (k.key === 'Enter') {
        if (t === 'TEXTAREA' || el.isContentEditable) insert(el, '\n');
        else if (t === 'INPUT' && el.form && !NOT_TEXT.test(el.type)) el.form.requestSubmit();
        else if (t === 'A' || t === 'BUTTON' || t === 'SUMMARY' || (t === 'INPUT' && /^(button|submit|reset|checkbox|radio)$/.test(el.type))) el.click();
      } else if (k.key === ' ' && !isText(el) && (t === 'BUTTON' || t === 'SUMMARY' || t === 'INPUT')) {
        el.click();
      } else if (k.key === 'Tab') {
        moveFocus(k.shift);
      } else if (k.key === 'Backspace' && isText(el)) {
        document.execCommand('delete');
      } else if (k.key === 'Delete' && isText(el)) {
        document.execCommand('forwardDelete');
      } else if (k.key.length === 1 && isText(el)) {
        insert(el, k.key);
      }
    }
    el.dispatchEvent(new KeyboardEvent('keyup', o));
    var a = document.activeElement;
    return a && a !== document.body ? describe(a) : { role: 'document', name: document.title };
  }

  // `rade browser wait`: is the condition met now?
  function check(w) {
    if (w.text !== undefined) return !!document.body && document.body.innerText.indexOf(w.text) >= 0;
    if (w.ref !== undefined) {
      try { return byRef(w.ref).getClientRects().length > 0; } catch (e) { return false; }
    }
    return false;
  }

  // Where a ref's element is on screen (the Chromium engine clicks there
  // with trusted mouse events).
  function point(r) {
    var el = byRef(r);
    el.scrollIntoView({ block: 'center', inline: 'center' });
    var b = el.getBoundingClientRect();
    return { x: b.left + b.width / 2, y: b.top + b.height / 2, w: b.width, h: b.height, el: describe(el) };
  }

  window.__rade = {
    snapshot: snapshot, click: click, fill: fill, press: press, check: check, point: point,
    focused: function () {
      var a = document.activeElement;
      return a && a !== document.body ? describe(a) : { role: 'document', name: document.title };
    },
    // For annotate.js: an element's ref and its role / name.
    ref: refOf, describe: describe, text: textOf
  };
})();
