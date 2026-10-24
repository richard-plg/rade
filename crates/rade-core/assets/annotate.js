// RADE Browser Annotations (PRD R71, ADR-0026). Runs in the isolated
// script world "rade" after browser.js. Annotate mode lays a transparent
// layer over the page (in a closed shadow root): hover highlights an
// element, a click picks it, a drag picks a region; a small box takes the
// comment. The result goes to the host — RADE's Browser Pane (a WebKit
// message handler of this world) or the Chromium engine (a CDP binding
// exposed to this world only) — which crops the screenshot and keeps it.
// Off, nothing of it is in the page (the headed Chromium keeps only its
// small toolbar button).
(function () {
  var R = window.__rade;
  if (!R || R.annotate) return;
  var host = null, root = null, st = { on: false, toolbar: false, items: [], pending: null };

  function post(o) {
    var s = JSON.stringify(o);
    var h = window.webkit && window.webkit.messageHandlers && window.webkit.messageHandlers.radeAnnotate;
    if (h) h.postMessage(s);
    else if (typeof window.radeAnnotate === 'function') window.radeAnnotate(s);
  }

  var CSS_TEXT = [
    ':host { all: initial; }',
    '* { box-sizing: border-box; font: 13px/1.35 system-ui, sans-serif; }',
    '.layer { position: fixed; inset: 0; pointer-events: auto; cursor: crosshair; background: transparent; }',
    '.hl { position: fixed; display: none; pointer-events: none; border: 2px solid #1c71d8; background: rgba(28,113,216,.14); border-radius: 3px; }',
    '.hl.region { border-style: dashed; }',
    '.tag { position: fixed; display: none; pointer-events: none; background: #1c71d8; color: #fff; padding: 1px 6px; border-radius: 3px; font-size: 12px; max-width: 360px; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }',
    '.pin { position: fixed; pointer-events: none; min-width: 20px; height: 20px; padding: 0 5px; border-radius: 10px; background: #c64600; color: #fff; font-weight: 700; font-size: 12px; text-align: center; line-height: 20px; box-shadow: 0 1px 3px rgba(0,0,0,.35); }',
    '.pinbox { position: fixed; pointer-events: none; border: 2px solid #c64600; border-radius: 3px; }',
    '.pop { position: fixed; display: none; pointer-events: auto; width: min(320px, calc(100vw - 16px)); padding: 10px; border-radius: 8px; background: #fff; color: #1e1e1e; box-shadow: 0 4px 18px rgba(0,0,0,.28); border: 1px solid rgba(0,0,0,.12); }',
    '.pop .what { font-weight: 600; margin-bottom: 6px; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }',
    '.pop textarea { width: 100%; min-height: 64px; resize: vertical; padding: 6px; border-radius: 6px; border: 1px solid rgba(0,0,0,.25); background: inherit; color: inherit; }',
    '.pop textarea:focus { outline: 2px solid #1c71d8; outline-offset: -1px; }',
    '.row { display: flex; gap: 6px; justify-content: flex-end; align-items: center; margin-top: 8px; }',
    '.hint { flex: 1; opacity: .65; font-size: 12px; }',
    'button { cursor: pointer; padding: 4px 12px; border-radius: 6px; border: 1px solid rgba(0,0,0,.18); background: rgba(0,0,0,.05); color: inherit; }',
    'button:focus-visible { outline: 2px solid #1c71d8; outline-offset: 2px; }',
    'button.go { background: #c64600; border-color: #c64600; color: #fff; }',
    'button:disabled { opacity: .5; cursor: default; }',
    '.bar { position: fixed; right: 12px; bottom: 12px; display: flex; gap: 6px; align-items: center; pointer-events: auto; padding: 6px 8px; border-radius: 8px; background: #fff; color: #1e1e1e; box-shadow: 0 2px 10px rgba(0,0,0,.25); }',
    '.bar { opacity: .6; transition: opacity .15s; } .bar:hover, .bar.live { opacity: 1; }',
    '.bar button.on { background: #1c71d8; color: #fff; border-color: #1c71d8; }',
    '@media (prefers-color-scheme: dark) {',
    '  .pop, .bar { background: #2e2e32; color: #f0f0f0; border-color: rgba(255,255,255,.12); }',
    '  .pop textarea { border-color: rgba(255,255,255,.25); }',
    '  button { border-color: rgba(255,255,255,.18); background: rgba(255,255,255,.08); }',
    '}'
  ].join('\n');

  function el(tag, cls, text) {
    var e = document.createElement(tag);
    if (cls) e.className = cls;
    if (text) e.textContent = text;
    return e;
  }

  var keyboardTarget = null;
  var layer, hl, tag, pins, pop, what, area, add, bar, toggleBtn, countLbl, sendBtn, clearBtn;

  function build() {
    host = document.createElement('rade-annotate');
    host.setAttribute('style', 'all: initial; position: fixed; inset: 0; z-index: 2147483647; pointer-events: none;');
    root = host.attachShadow({ mode: 'closed' });
    var style = el('style');
    style.textContent = CSS_TEXT;
    layer = el('div', 'layer');
    layer.tabIndex = -1;
    layer.setAttribute('aria-label', 'Annotation picker. Tab selects an element, Enter comments, Escape ends.');
    hl = el('div', 'hl');
    tag = el('div', 'tag');
    pins = el('div');
    pop = el('div', 'pop');
    what = el('div', 'what');
    area = el('textarea');
    area.placeholder = 'What should change here?';
    area.setAttribute('aria-label', 'Annotation comment');
    pop.setAttribute('role', 'dialog');
    pop.setAttribute('aria-label', 'Add annotation');
    tag.setAttribute('role', 'status');
    tag.setAttribute('aria-live', 'polite');
    var row = el('div', 'row');
    var hint = el('span', 'hint', 'Enter adds · Shift+Enter new line · Esc cancels');
    var cancel = el('button', '', 'Cancel');
    add = el('button', '', 'Add');
    row.append(hint, cancel, add);
    pop.append(what, area, row);
    bar = el('div', 'bar');
    toggleBtn = el('button', '', '✎ Annotate');
    countLbl = el('span', 'hint', '');
    sendBtn = el('button', '', 'Send to Agent');
    clearBtn = el('button', '', 'Clear');
    bar.append(toggleBtn, countLbl, sendBtn, clearBtn);
    root.append(style, layer, pins, hl, tag, pop, bar);

    layer.addEventListener('mousemove', onMove);
    layer.addEventListener('mousedown', onDown);
    layer.addEventListener('mouseup', onUp);
    layer.addEventListener('click', stop, true);
    layer.addEventListener('contextmenu', stop, true);
    cancel.addEventListener('click', function (e) { if (!e.isTrusted) return; stop(e); closePop(); });
    add.addEventListener('click', function (e) { if (!e.isTrusted) return; stop(e); submit(); });
    area.addEventListener('input', function () { setGo(add, !!area.value.trim()); });
    // Typing in the box is ours, not the page's shortcuts'.
    ['keydown', 'keyup', 'keypress'].forEach(function (t) {
      pop.addEventListener(t, function (e) {
        if (!e.isTrusted) return;
        e.stopPropagation();
        if (t !== 'keydown') return;
        if (e.key === 'Escape') { e.preventDefault(); closePop(); }
        else if (e.key === 'Enter' && !e.shiftKey) { e.preventDefault(); submit(); }
      });
    });
    toggleBtn.addEventListener('click', function (e) { if (!e.isTrusted) return; stop(e); post({ op: 'toggle', on: !st.on }); });
    sendBtn.addEventListener('click', function (e) { if (!e.isTrusted) return; stop(e); post({ op: 'send' }); });
    clearBtn.addEventListener('click', function (e) { if (!e.isTrusted) return; stop(e); post({ op: 'clear' }); });
    addEventListener('scroll', place, true);
    addEventListener('resize', place);
    // Marks follow the page's own layout changes too.
    if (window.ResizeObserver) new ResizeObserver(place).observe(document.documentElement);

  }

  document.addEventListener('keydown', onKey, true);

  function stop(e) { e.preventDefault(); e.stopPropagation(); }
  // A disabled primary button reads as an error (R20.1): accent only when it can be pressed.
  function setGo(b, on) { b.disabled = !on; b.className = on ? 'go' : ''; }

  function onKey(e) {
    if (!e.isTrusted || !root) return;
    var path = e.composedPath();
    if (path.indexOf(bar) >= 0 || path.indexOf(pop) >= 0) return;
    if (st.on && !st.pending && (e.key === 'Tab' || e.key === 'Enter')) {
      stop(e);
      R.annotateKey(e.key === 'Enter' ? 'pick' : (e.shiftKey ? 'previous' : 'next'));
      return;
    }
    if (st.on && e.key === 'Escape' && !st.pending) { stop(e); post({ op: 'toggle', on: false }); }
  }

  // Native WebKit shortcuts call this in the isolated world, before the page sees a key.
  R.annotateKey = function (key) {
    if (!st.on) return { on: false };
    if (key === 'cancel') {
      if (st.pending) closePop();
      else post({ op: 'toggle', on: false });
    } else if (!st.pending && key === 'pick') {
      if (keyboardTarget && keyboardTarget.isConnected) pickElement(keyboardTarget);
    } else if (!st.pending && (key === 'next' || key === 'previous')) {
      var targets = Array.from(document.querySelectorAll('body *')).filter(function (e) {
        if (e === host || /^(SCRIPT|STYLE|LINK|META)$/.test(e.tagName)) return false;
        var b = e.getBoundingClientRect();
        return b.width > 0 && b.height > 0 && getComputedStyle(e).visibility !== 'hidden';
      });
      var i = targets.indexOf(keyboardTarget), step = key === 'previous' ? -1 : 1;
      if (i < 0 && step < 0) i = 0;
      keyboardTarget = targets[(i + step + targets.length) % targets.length] || null;
      if (keyboardTarget) {
        keyboardTarget.scrollIntoView({ block: 'nearest', inline: 'nearest' });
        var b = rectOf(keyboardTarget);
        box(b, hl, 'hl');
        showTag(b, label(keyboardTarget) + ' · Enter picks · Tab next · Esc ends');
      }
    }
    return { on: st.on, pending: !!st.pending, target: keyboardTarget ? label(keyboardTarget) : null };
  };

  // The page element under a point (our layer let through).
  function under(x, y) {
    layer.style.pointerEvents = 'none';
    host.style.pointerEvents = 'none';
    var e = document.elementFromPoint(x, y);
    // Into open shadow roots (while our layer still lets the point through).
    while (e && e.shadowRoot && e !== host) {
      var inner = e.shadowRoot.elementFromPoint(x, y);
      if (!inner || inner === e) break;
      e = inner;
    }
    layer.style.pointerEvents = 'auto';
    return e === host ? null : e;
  }

  function box(b, node, cls) {
    node.className = cls;
    node.style.display = 'block';
    node.style.left = b.x + 'px';
    node.style.top = b.y + 'px';
    node.style.width = Math.max(b.w, 2) + 'px';
    node.style.height = Math.max(b.h, 2) + 'px';
  }
  function rectOf(e) {
    var b = e.getBoundingClientRect();
    return { x: b.left, y: b.top, w: b.width, h: b.height };
  }
  function label(e) {
    var d = R.describe(e);
    return d.name ? d.role + ' "' + d.name.slice(0, 60) + '"' : d.role;
  }
  function showTag(b, text) {
    tag.textContent = text;
    tag.style.display = 'block';
    tag.style.left = Math.max(b.x, 0) + 'px';
    tag.style.top = (b.y > 24 ? b.y - 22 : b.y + b.h + 4) + 'px';
  }

  var down = null;
  function onMove(e) {
    if (!e.isTrusted) return;
    stop(e);
    if (st.pending) return;
    if (down && (e.buttons & 1)) {
      var r = { x: Math.min(down.x, e.clientX), y: Math.min(down.y, e.clientY), w: Math.abs(e.clientX - down.x), h: Math.abs(e.clientY - down.y) };
      if (down.drag || r.w > 6 || r.h > 6) {
        down.drag = r;
        box(r, hl, 'hl region');
        showTag(r, Math.round(r.w) + ' × ' + Math.round(r.h));
      }
      return;
    }
    var t = under(e.clientX, e.clientY);
    if (!t || t === document.documentElement || t === document.body) { hl.style.display = 'none'; tag.style.display = 'none'; return; }
    var b = rectOf(t);
    box(b, hl, 'hl');
    showTag(b, label(t));
  }
  function onDown(e) {
    if (!e.isTrusted) return;
    stop(e);
    if (e.button !== 0) return;
    if (st.pending) closePop();
    down = { x: e.clientX, y: e.clientY, drag: null };
  }
  function onUp(e) {
    if (!e.isTrusted) return;
    stop(e);
    if (!down || e.button !== 0) return;
    var d = down;
    down = null;
    if (d.drag) pickRegion(d.drag);
    else {
      var t = under(e.clientX, e.clientY);
      if (t) pickElement(t);
    }
  }

  // The smallest element holding all of a region.
  function anchorOf(r) {
    var e = under(r.x + r.w / 2, r.y + r.h / 2) || document.body;
    while (e && e.parentElement && e !== document.body) {
      var b = e.getBoundingClientRect();
      if (b.left <= r.x + 1 && b.top <= r.y + 1 && b.right >= r.x + r.w - 1 && b.bottom >= r.y + r.h - 1) break;
      e = e.parentElement;
    }
    return e || document.body;
  }
  function pickElement(t) { open({ kind: 'element', el: t, rect: rectOf(t) }); }
  function pickRegion(r) { open({ kind: 'region', el: anchorOf(r), rect: r }); }

  function open(p) {
    st.pending = p;
    box(p.rect, hl, p.kind === 'region' ? 'hl region' : 'hl');
    tag.style.display = 'none';
    what.textContent = p.kind === 'region'
      ? 'Region ' + Math.round(p.rect.w) + ' × ' + Math.round(p.rect.h) + ' in ' + label(p.el)
      : label(p.el);
    area.value = '';
    setGo(add, false);
    pop.style.display = 'block';
    var vw = innerWidth, vh = innerHeight, pw = 320, ph = pop.offsetHeight || 150;
    var x = Math.min(Math.max(p.rect.x, 8), vw - pw - 8);
    var y = p.rect.y + p.rect.h + 8;
    if (y + ph > vh - 8) y = Math.max(p.rect.y - ph - 8, 8);
    pop.style.left = Math.max(x, 8) + 'px';
    pop.style.top = y + 'px';
    area.focus();
  }
  function closePop() {
    st.pending = null;
    pop.style.display = 'none';
    hl.style.display = 'none';
    tag.style.display = 'none';
    if (st.on) layer.focus();
  }

  function attr(e, a) { var v = e.getAttribute(a); return v === null ? null : v; }
  var TESTIDS = ['data-testid', 'data-test', 'data-cy', 'data-qa'];
  function count(sel) { try { return document.querySelectorAll(sel).length; } catch (x) { return 0; } }
  function info(e) {
    var tagName = e.tagName.toLowerCase(), nth = 1, of = 0;
    if (e.parentElement) {
      for (var s = e.parentElement.firstElementChild; s; s = s.nextElementSibling) {
        if (s.tagName === e.tagName) { of++; if (s === e) nth = of; }
      }
    } else of = 1;
    var i = { tag: tagName, nth: nth, of_type: of, id: e.id || null, name: null, testid: null,
      id_unique: false, testid_unique: false, name_unique: false };
    if (e.id) i.id_unique = count('#' + CSS.escape(e.id)) === 1;
    for (var k = 0; k < TESTIDS.length; k++) {
      var v = attr(e, TESTIDS[k]);
      if (v) { i.testid = [TESTIDS[k], v]; i.testid_unique = count('[' + TESTIDS[k] + '="' + CSS.escape(v) + '"]') === 1; break; }
    }
    var n = attr(e, 'name');
    if (n && /^(input|select|textarea|button|form|iframe)$/.test(tagName)) {
      i.name = n;
      i.name_unique = count(tagName + '[name="' + CSS.escape(n) + '"]') === 1;
    }
    return i;
  }
  function chain(e) {
    var out = [];
    for (; e && e.nodeType === 1; e = e.parentElement) {
      out.push(info(e));
      if (e.tagName === 'BODY' || e.tagName === 'HTML') break;
    }
    return out;
  }

  function pickOf(p, comment) {
    var d = R.describe(p.el);
    return {
      url: location.href, kind: p.kind, chain: chain(p.el), ref: R.ref(p.el),
      role: d.role, name: d.name || '', text: R.text(p.el).slice(0, 600),
      rect: p.kind === 'region' ? p.rect : rectOf(p.el),
      scroll: { x: scrollX, y: scrollY, w: 0, h: 0 },
      viewport: { x: 0, y: 0, w: innerWidth, h: innerHeight },
      comment: comment
    };
  }

  // Add: the box and highlight go first, so the host's screenshot shows
  // the page itself.
  function submit() {
    var p = st.pending, text = area.value.trim();
    if (!p || !text) return;
    closePop();
    var pick = pickOf(p, text);
    pins.style.display = 'none';
    requestAnimationFrame(function () {
      requestAnimationFrame(function () {
        post({ op: 'add', pick: pick });
        setTimeout(function () { pins.style.display = ''; }, 400);
      });
    });
  }

  // Numbered marks on this page's annotations (by selector, else where
  // they were).
  function place() {
    if (!root) return;
    pins.textContent = '';
    if (!st.on && !st.toolbar) return;
    st.items.forEach(function (it) {
      if (it.url !== location.href) return;
      var b = null;
      try { var t = it.selector && document.querySelector(it.selector); if (t && it.kind !== 'region') b = rectOf(t); } catch (x) {}
      if (!b) b = { x: it.bbox.x - scrollX, y: it.bbox.y - scrollY, w: it.bbox.w, h: it.bbox.h };
      var frame = el('div', 'pinbox');
      box(b, frame, 'pinbox');
      var pin = el('div', 'pin', String(it.n));
      pin.style.left = Math.max(b.x - 10, 0) + 'px';
      pin.style.top = Math.max(b.y - 10, 0) + 'px';
      pins.append(frame, pin);
    });
  }

  function render() {
    if (!st.on && !st.toolbar) {
      if (host) host.remove();
      host = null; root = null;
      return;
    }
    if (!host) build();
    if (!host.isConnected) document.documentElement.appendChild(host);
    layer.style.display = st.on ? 'block' : 'none';
    if (!st.on) closePop();
    bar.style.display = st.toolbar ? 'flex' : 'none';
    toggleBtn.className = st.on ? 'on' : '';
    toggleBtn.textContent = st.on ? '✎ Annotating · Tab selects · Enter picks · Esc ends' : '✎ Annotate';
    countLbl.textContent = st.items.length ? st.items.length + ' pending' : '';
    setGo(sendBtn, !!st.items.length);
    bar.className = st.on || st.items.length ? 'bar live' : 'bar';
    clearBtn.disabled = !st.items.length;
    place();
  }

  // Host → page: mode, the toolbar (headed Chromium), the annotations.
  R.annotate = function (o) {
    var wasOn = st.on;
    st.on = !!o.on;
    st.toolbar = !!o.toolbar;
    st.items = o.items || [];
    render();
    if (st.on && !wasOn) layer.focus();
    return { on: st.on, pending: !!st.pending };
  };
  // Tests (debug) and the CLI: pick what is at a point / a region / a ref
  // and type the comment, as the mouse and keyboard would.
  R.annotateAt = function (o) {
    if (!st.on) throw new Error('annotate mode is off');
    var p;
    if (o.ref) {
      var t = R.point(o.ref);
      p = { x: t.x, y: t.y };
    }
    if (o.w && o.h) pickRegion({ x: o.x, y: o.y, w: o.w, h: o.h });
    else {
      var e = under(p ? p.x : o.x, p ? p.y : o.y);
      if (!e) throw new Error('nothing to annotate there');
      pickElement(e);
    }
    var what_ = what.textContent;
    if (o.comment !== undefined) {
      area.value = o.comment;
      setGo(add, !!area.value.trim());
      if (o.submit !== false) submit();
    }
    return { what: what_ };
  };
})();
