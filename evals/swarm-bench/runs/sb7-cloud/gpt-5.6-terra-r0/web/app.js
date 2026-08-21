(function () {
  'use strict';

  var EXP = { EUR: 2, USD: 2, JPY: 0, KWD: 3 };
  var SYM = { EUR: '\u20AC', USD: '$', JPY: '\u00A5' };
  var STATUS_LABEL = { settled: 'Settled', pending: 'Pending', refunded: 'Refunded', failed: 'Failed' };
  var POW = [1, 10, 100, 1000];

  function expOf(cur) {
    return Object.prototype.hasOwnProperty.call(EXP, cur) ? EXP[cur] : 2;
  }

  function groupDigits(s) {
    var out = '';
    var n = s.length;
    for (var i = 0; i < n; i++) {
      if (i > 0 && (n - i) % 3 === 0) out += ',';
      out += s.charAt(i);
    }
    return out;
  }

  function fmtMoney(amount_minor, currency) {
    var cur = typeof currency === 'string' ? currency.toUpperCase() : 'EUR';
    var m = Number(amount_minor);
    if (!isFinite(m)) m = 0;
    m = Math.trunc(m);
    var neg = m < 0;
    var abs = Math.abs(m);
    var e = expOf(cur);
    var p = POW[e];
    var whole = Math.trunc(abs / p);
    var frac = abs % p;
    var digits = groupDigits(String(whole));
    if (e > 0) {
      var f = String(frac);
      while (f.length < e) f = '0' + f;
      digits += '.' + f;
    }
    var sym = SYM[cur];
    var body = sym ? sym + digits : cur + ' ' + digits;
    return neg ? '-' + body : body;
  }

  var DATE_OPTS = { day: 'numeric', month: 'short', year: 'numeric', hour: '2-digit', minute: '2-digit' };

  function fmtDate(iso) {
    if (!iso) return '\u2014';
    var d = new Date(iso);
    if (isNaN(d.getTime())) return '\u2014';
    try {
      return d.toLocaleString(undefined, DATE_OPTS);
    } catch (e) {
      return d.toDateString();
    }
  }

  function el(id) { return document.getElementById(id); }

  function txt(node, s) { if (node) node.textContent = s; }

  function show(node, visible) {
    if (!node) return;
    if (visible) node.removeAttribute('hidden');
    else node.setAttribute('hidden', '');
  }

  var noticeTimer = null;

  function notice(msg, kind) {
    var n = el('notice');
    if (!n) return;
    if (!msg) {
      n.textContent = '';
      n.removeAttribute('data-kind');
      return;
    }
    n.textContent = String(msg);
    n.setAttribute('data-kind', kind === 'ok' || kind === 'warn' || kind === 'error' ? kind : 'warn');
    if (noticeTimer) clearTimeout(noticeTimer);
    if (kind === 'ok') {
      noticeTimer = setTimeout(function () {
        if (!stickyNotice) {
          n.textContent = '';
          n.removeAttribute('data-kind');
        } else {
          notice(stickyNotice, 'warn');
        }
      }, 4000);
    }
  }

  var stickyNotice = '';

  function setSticky(msg) {
    stickyNotice = msg || '';
    if (stickyNotice) {
      var n = el('notice');
      if (n && !n.getAttribute('data-kind')) notice(stickyNotice, 'warn');
    }
  }

  function envelopeMessage(body, fallback) {
    if (body && body.error) {
      var e = body.error;
      var m = e.message ? String(e.message) : '';
      if (e.field_errors && e.field_errors.length) {
        var parts = [];
        for (var i = 0; i < e.field_errors.length; i++) {
          var fe = e.field_errors[i];
          parts.push(String(fe.path) + ' (' + String(fe.code) + ')');
        }
        m = (m ? m + ' \u2014 ' : '') + parts.join(', ');
      }
      if (m) return m;
      if (e.code) return String(e.code);
    }
    return fallback;
  }

  function envelopeCode(body) {
    return body && body.error && body.error.code ? String(body.error.code) : '';
  }

  function api(path, opts) {
    var o = opts || {};
    var init = { method: o.method || 'GET', headers: {} };
    if (o.body !== undefined) {
      init.headers['Content-Type'] = 'application/json';
      init.body = JSON.stringify(o.body);
    }
    if (o.auth) {
      var tk = currentToken();
      if (tk) init.headers['Authorization'] = 'Bearer ' + tk;
    }
    return fetch(path, init).then(function (res) {
      return res.text().then(function (raw) {
        var body = null;
        if (raw) {
          try { body = JSON.parse(raw); } catch (e) { body = null; }
        }
        return { ok: res.ok, status: res.status, body: body };
      }, function () {
        return { ok: res.ok, status: res.status, body: null };
      });
    }, function (err) {
      return { ok: false, status: 0, body: null, network: true, err: err };
    });
  }

  var state = {
    limit: 50,
    offset: 0,
    status: '',
    currency: '',
    sort: 'created_at',
    total: 0,
    rows: [],
    firstLoadDone: false,
    lastHealthPayments: -1
  };

  var noteVersions = {};

  function tableState(which) {
    show(el('table-loading'), which === 'loading');
    show(el('table-empty'), which === 'empty');
    show(el('table-error'), which === 'error');
    var wrap = document.querySelector('.table-scroll');
    show(wrap, which === 'table');
  }

  function renderSummary(s) {
    var host = el('summary');
    if (!host) return;
    var frag = document.createDocumentFragment();
    var byCur = (s && s.by_currency) || [];
    var revs = (s && s.reversals) || [];
    var i;
    for (i = 0; i < byCur.length; i++) {
      var c = byCur[i];
      frag.appendChild(summaryCard('cur-total', c.currency, c.count, c.total_minor, 'payments'));
    }
    for (i = 0; i < revs.length; i++) {
      var r = revs[i];
      frag.appendChild(summaryCard('rev-total', r.currency, r.count, r.total_minor, 'reversals'));
    }
    if (!byCur.length && !revs.length) {
      var none = document.createElement('p');
      none.className = 's-label';
      none.textContent = 'No ledger totals yet \u2014 run a sync to pull the vendor ledger.';
      frag.appendChild(none);
    }
    host.textContent = '';
    host.appendChild(frag);
  }

  function summaryCard(cls, currency, count, totalMinor, word) {
    var cur = String(currency || '').toUpperCase();
    var d = document.createElement('div');
    d.className = cls;
    d.setAttribute('data-currency', cur);
    var lab = document.createElement('div');
    lab.className = 's-label';
    lab.textContent = cls === 'rev-total' ? cur + ' reversals' : cur;
    var amt = document.createElement('div');
    amt.className = 's-amount';
    amt.textContent = fmtMoney(totalMinor, cur);
    var cnt = document.createElement('div');
    cnt.className = 's-count';
    var n = Number(count) || 0;
    cnt.textContent = n.toLocaleString() + ' ' + (n === 1 ? word.replace(/s$/, '') : word);
    d.appendChild(lab);
    d.appendChild(amt);
    d.appendChild(cnt);
    return d;
  }

  function loadSummary() {
    return api('/api/summary').then(function (r) {
      if (!r.ok || !r.body) return false;
      renderSummary(r.body);
      var ls = el('last-sync');
      if (ls) ls.textContent = r.body.last_sync ? 'Last sync ' + fmtDate(r.body.last_sync) : 'Never synced';
      return true;
    });
  }

  function paymentsQuery() {
    var q = ['limit=' + state.limit, 'offset=' + state.offset, 'sort=' + encodeURIComponent(state.sort)];
    if (state.status) q.push('status=' + encodeURIComponent(state.status));
    if (state.currency) q.push('currency=' + encodeURIComponent(state.currency));
    return '/api/payments?' + q.join('&');
  }

  function renderRows(rows) {
    var body = el('payments-body');
    if (!body) return;
    var frag = document.createDocumentFragment();
    for (var i = 0; i < rows.length; i++) {
      var p = rows[i];
      noteVersions[p.id] = p.version;
      var tr = document.createElement('tr');
      tr.setAttribute('data-id', p.id);
      tr.setAttribute('data-brushed', isBrushed(p.id) ? 'true' : 'false');

      var td1 = document.createElement('td');
      td1.className = 'cell-date';
      td1.textContent = fmtDate(p.created_at);
      tr.appendChild(td1);

      var td2 = document.createElement('td');
      td2.className = 'cell-amount';
      td2.textContent = fmtMoney(p.amount_minor, p.currency);
      tr.appendChild(td2);

      var td3 = document.createElement('td');
      var badge = document.createElement('span');
      var st = String(p.status || '');
      badge.className = 'badge badge-' + st;
      badge.textContent = STATUS_LABEL[st] || st;
      td3.appendChild(badge);
      tr.appendChild(td3);

      var td4 = document.createElement('td');
      td4.className = 'cell-cp';
      td4.appendChild(document.createTextNode(String(p.counterparty_name || '\u2014')));
      td4.appendChild(document.createTextNode(' \u00B7 '));
      var cc = document.createElement('span');
      cc.className = 'cp-country';
      cc.textContent = String(p.country || '');
      td4.appendChild(cc);
      tr.appendChild(td4);

      var td5 = document.createElement('td');
      td5.className = 'note-cell';
      paintNote(td5, p.note);
      tr.appendChild(td5);

      frag.appendChild(tr);
    }
    body.textContent = '';
    body.appendChild(frag);
  }

  function paintNote(cell, note) {
    cell.textContent = '';
    var span = document.createElement('span');
    if (note === null || note === undefined || note === '') {
      span.className = 'note-empty';
      span.textContent = 'Add a note';
    } else {
      span.className = 'note-text';
      span.textContent = String(note);
    }
    cell.appendChild(span);
    cell.setAttribute('data-note', note === null || note === undefined ? '' : String(note));
  }

  function renderShowing() {
    var s = el('showing');
    if (!s) return;
    var shownCount = state.rows.length;
    if (!state.total || shownCount === 0) {
      s.textContent = 'showing 0\u20130 of ' + (Number(state.total) || 0);
    } else {
      s.textContent = 'showing ' + (state.offset + 1) + '\u2013' + (state.offset + shownCount) +
        ' of ' + Number(state.total);
    }
    var prev = el('prev');
    var next = el('next');
    if (prev) prev.disabled = state.offset <= 0;
    if (next) next.disabled = state.offset + shownCount >= state.total || shownCount === 0;
  }

  function loadPayments(opts) {
    var o = opts || {};
    if (!state.firstLoadDone && !o.quiet) tableState('loading');
    return api(paymentsQuery()).then(function (r) {
      state.firstLoadDone = true;
      if (!r.ok || !r.body) {
        if (r.status === 0 || r.status >= 500) {
          tableState('error');
          state.rows = [];
          renderRows([]);
          state.total = 0;
          renderShowing();
          return false;
        }
        var msg = envelopeMessage(r.body, 'Payments could not be loaded.');
        notice(msg, 'error');
        tableState('error');
        return false;
      }
      var data = r.body.data || [];
      state.total = Number(r.body.total) || 0;
      if (data.length === 0 && state.offset > 0 && state.total > 0) {
        state.offset = Math.max(0, (Math.ceil(state.total / state.limit) - 1) * state.limit);
        return loadPayments({ quiet: true });
      }
      state.rows = data;
      renderRows(data);
      renderShowing();
      tableState(state.total === 0 ? 'empty' : 'table');
      return true;
    });
  }

  function isBrushed(id) {
    try {
      return !!(window.VizAPI && VizAPI.hasBrush && VizAPI.hasBrush(id));
    } catch (e) {
      return false;
    }
  }

  function repaintBrush(ids) {
    var set = {};
    if (ids && ids.length) {
      for (var i = 0; i < ids.length; i++) set[ids[i]] = true;
    }
    var rows = document.querySelectorAll('#payments-body tr[data-id]');
    for (var j = 0; j < rows.length; j++) {
      var tr = rows[j];
      tr.setAttribute('data-brushed', set[tr.getAttribute('data-id')] ? 'true' : 'false');
    }
  }

  function markRow(id) {
    var tr = document.querySelector('#payments-body tr[data-id="' + cssEscape(id) + '"]');
    if (!tr) return false;
    tr.setAttribute('data-brushed', 'true');
    try {
      tr.scrollIntoView({ block: 'center', behavior: 'smooth' });
    } catch (e) {
      tr.scrollIntoView();
    }
    return true;
  }

  function cssEscape(s) {
    return String(s).replace(/["\\]/g, '\\$&');
  }

  function sortKeyOf(rec) {
    if (state.sort.indexOf('amount_minor') >= 0) return Number(rec.amount_minor);
    var t = new Date(rec.created_at).getTime();
    return isNaN(t) ? 0 : t;
  }

  function sortDesc() { return state.sort.charAt(0) === '-'; }

  function pageHas(rows, id) {
    for (var i = 0; i < rows.length; i++) if (rows[i].id === id) return true;
    return false;
  }

  function fetchPage(pageIndex) {
    var offset = pageIndex * state.limit;
    var save = state.offset;
    state.offset = offset;
    var url = paymentsQuery();
    state.offset = save;
    return api(url).then(function (r) {
      if (!r.ok || !r.body) return null;
      return { rows: r.body.data || [], total: Number(r.body.total) || 0, offset: offset };
    });
  }

  function revealPayment(id) {
    if (!id) return;
    if (markRow(id)) {
      if (window.VizAPI && VizAPI.hasBrush && !VizAPI.hasBrush(id) && VizAPI.toggleBrush) {
        try { VizAPI.toggleBrush(id); } catch (e) {}
      }
      return;
    }
    api('/api/payments/' + encodeURIComponent(id)).then(function (r) {
      if (!r.ok || !r.body) {
        notice('That payment could not be located in the ledger.', 'warn');
        return;
      }
      var rec = r.body;
      if (state.status && rec.status !== state.status) {
        notice('Payment ' + id + ' is ' + (STATUS_LABEL[rec.status] || rec.status) + ' and is hidden by the current status filter.', 'warn');
        return;
      }
      if (state.currency && rec.currency !== state.currency) {
        notice('Payment ' + id + ' is in ' + rec.currency + ' and is hidden by the current currency filter.', 'warn');
        return;
      }
      return locatePage(rec).then(function (pageIndex) {
        if (pageIndex === null) {
          notice('Could not page to payment ' + id + '.', 'warn');
          return;
        }
        state.offset = pageIndex * state.limit;
        return loadPayments({ quiet: true }).then(function () {
          if (window.VizAPI && VizAPI.hasBrush && !VizAPI.hasBrush(id) && VizAPI.toggleBrush) {
            try { VizAPI.toggleBrush(id); } catch (e) {}
          }
          markRow(id);
        });
      });
    });
  }

  function locatePage(rec) {
    var target = sortKeyOf(rec);
    var desc = sortDesc();
    var budget = 16;

    function before(a, b) { return desc ? a > b : a < b; }

    return fetchPage(Math.floor(state.offset / state.limit)).then(function (first) {
      var total = first ? first.total : state.total;
      if (!total) return null;
      var lastPage = Math.max(0, Math.ceil(total / state.limit) - 1);
      var lo = 0, hi = lastPage;

      function step() {
        if (budget-- <= 0 || lo > hi) return Promise.resolve(null);
        var mid = Math.floor((lo + hi) / 2);
        return fetchPage(mid).then(function (page) {
          if (!page || !page.rows.length) return null;
          if (pageHas(page.rows, rec.id)) return mid;
          var kFirst = sortKeyOf(page.rows[0]);
          var kLast = sortKeyOf(page.rows[page.rows.length - 1]);
          if (before(target, kFirst)) {
            hi = mid - 1;
          } else if (before(kLast, target)) {
            lo = mid + 1;
          } else {
            return scanOut(mid);
          }
          return step();
        });
      }

      function scanOut(center) {
        var offsets = [];
        for (var d = 1; d <= 6; d++) {
          if (center - d >= lo) offsets.push(center - d);
          if (center + d <= hi) offsets.push(center + d);
        }
        var i = 0;
        function nextTry() {
          if (i >= offsets.length || budget-- <= 0) return null;
          var pg = offsets[i++];
          return fetchPage(pg).then(function (page) {
            if (page && pageHas(page.rows, rec.id)) return pg;
            return nextTry();
          });
        }
        return nextTry();
      }

      return step();
    });
  }

  function openNoteEditor(cell, tr) {
    if (cell.querySelector('.note-editor')) return;
    var id = tr.getAttribute('data-id');
    var prev = cell.getAttribute('data-note') || '';
    var wrap = document.createElement('div');
    wrap.className = 'note-editor';
    var input = document.createElement('input');
    input.type = 'text';
    input.value = prev;
    input.maxLength = 280;
    input.setAttribute('aria-label', 'Note for payment ' + id);
    var ok = document.createElement('button');
    ok.type = 'button';
    ok.className = 'note-ok';
    ok.textContent = 'Save';
    var cancel = document.createElement('button');
    cancel.type = 'button';
    cancel.className = 'note-cancel';
    cancel.textContent = 'Cancel';
    wrap.appendChild(input);
    wrap.appendChild(ok);
    wrap.appendChild(cancel);
    cell.textContent = '';
    cell.appendChild(wrap);
    input.focus();
    input.select();

    function close() { paintNote(cell, prev); }

    function confirm() {
      var val = input.value.trim();
      if (val.length < 1 || val.length > 280) {
        notice('A note must be between 1 and 280 characters.', 'warn');
        input.focus();
        return;
      }
      paintNote(cell, val);
      tr.setAttribute('data-state', 'saving');
      saveNote(id, val, prev, cell, tr);
    }

    ok.addEventListener('click', function (ev) { ev.stopPropagation(); confirm(); });
    cancel.addEventListener('click', function (ev) { ev.stopPropagation(); close(); });
    input.addEventListener('click', function (ev) { ev.stopPropagation(); });
    input.addEventListener('keydown', function (ev) {
      ev.stopPropagation();
      if (ev.key === 'Enter') { ev.preventDefault(); confirm(); }
      else if (ev.key === 'Escape') { ev.preventDefault(); close(); }
    });
  }

  function saveNote(id, val, prev, cell, tr) {
    api('/api/payments/' + encodeURIComponent(id) + '/note', { method: 'POST', body: { note: val } }).then(function (r) {
      if (r.ok && r.body) {
        noteVersions[id] = r.body.version;
        paintNote(cell, r.body.note === undefined ? val : r.body.note);
        tr.setAttribute('data-state', 'saved');
        notice('Note saved.', 'ok');
        return;
      }
      paintNote(cell, prev);
      tr.removeAttribute('data-state');
      var code = envelopeCode(r.body);
      if (code === 'conflict' || r.status === 409) {
        notice(envelopeMessage(r.body, 'This payment changed elsewhere \u2014 your note was reverted. Reload the row and try again.'), 'error');
        loadPayments({ quiet: true });
      } else if (r.status === 0) {
        notice('The note could not be saved \u2014 the ledger service is unreachable. Your change was reverted.', 'error');
      } else {
        notice(envelopeMessage(r.body, 'The note could not be saved \u2014 your change was reverted.'), 'error');
      }
    });
  }

  function wireTable() {
    var body = el('payments-body');
    if (body) {
      body.addEventListener('click', function (ev) {
        var tr = ev.target.closest ? ev.target.closest('tr[data-id]') : null;
        if (!tr) return;
        var noteCell = ev.target.closest('.note-cell');
        if (noteCell) {
          openNoteEditor(noteCell, tr);
          return;
        }
        var id = tr.getAttribute('data-id');
        if (window.VizAPI && VizAPI.toggleBrush) {
          try { VizAPI.toggleBrush(id); } catch (e) {}
          tr.setAttribute('data-brushed', isBrushed(id) ? 'true' : 'false');
        } else {
          tr.setAttribute('data-brushed', tr.getAttribute('data-brushed') === 'true' ? 'false' : 'true');
        }
      });
    }

    var prev = el('prev');
    if (prev) prev.addEventListener('click', function () {
      if (state.offset <= 0) return;
      state.offset = Math.max(0, state.offset - state.limit);
      loadPayments({ quiet: true });
    });
    var next = el('next');
    if (next) next.addEventListener('click', function () {
      if (state.offset + state.limit >= state.total) return;
      state.offset += state.limit;
      loadPayments({ quiet: true });
    });

    var heads = document.querySelectorAll('th[data-sort-key]');
    for (var i = 0; i < heads.length; i++) {
      (function (th) {
        function activate() {
          var key = th.getAttribute('data-sort-key');
          var cur = th.getAttribute('aria-sort');
          var asc = cur !== 'ascending';
          for (var j = 0; j < heads.length; j++) {
            heads[j].setAttribute('aria-sort', heads[j] === th ? (asc ? 'ascending' : 'descending') : 'none');
          }
          state.sort = (asc ? '' : '-') + key;
          state.offset = 0;
          loadPayments({ quiet: true });
        }
        th.addEventListener('click', activate);
        th.addEventListener('keydown', function (ev) {
          if (ev.key === 'Enter' || ev.key === ' ' || ev.key === 'Spacebar') {
            ev.preventDefault();
            activate();
          }
        });
      })(heads[i]);
    }
  }

  function closeAllMenus(except) {
    var dds = document.querySelectorAll('.dropdown');
    for (var i = 0; i < dds.length; i++) {
      if (dds[i] === except) continue;
      var t = dds[i].querySelector('.dd-trigger');
      var m = dds[i].querySelector('.dd-menu');
      if (t) t.setAttribute('aria-expanded', 'false');
      if (m) m.setAttribute('hidden', '');
    }
  }

  function wireDropdown(dd, onPick) {
    if (!dd) return;
    var trigger = dd.querySelector('.dd-trigger');
    var menu = dd.querySelector('.dd-menu');
    var label = dd.querySelector('.dd-label');
    if (!trigger || !menu) return;

    function setOpen(open) {
      if (open) closeAllMenus(dd);
      trigger.setAttribute('aria-expanded', open ? 'true' : 'false');
      if (open) menu.removeAttribute('hidden');
      else menu.setAttribute('hidden', '');
    }

    trigger.addEventListener('click', function (ev) {
      ev.stopPropagation();
      setOpen(trigger.getAttribute('aria-expanded') !== 'true');
    });

    trigger.addEventListener('keydown', function (ev) {
      if (ev.key === 'Escape') { setOpen(false); }
      else if (ev.key === 'ArrowDown') { ev.preventDefault(); setOpen(true); }
    });

    var opts = menu.querySelectorAll('.dd-opt');
    for (var i = 0; i < opts.length; i++) {
      (function (li) {
        if (!li.hasAttribute('tabindex')) li.setAttribute('tabindex', '0');
        function pick(ev) {
          if (ev) ev.stopPropagation();
          var val = li.getAttribute('data-opt') || '';
          dd.setAttribute('data-value', val);
          if (label) label.textContent = li.textContent;
          for (var k = 0; k < opts.length; k++) {
            opts[k].setAttribute('aria-selected', opts[k] === li ? 'true' : 'false');
          }
          setOpen(false);
          trigger.focus();
          if (onPick) onPick(val);
        }
        li.addEventListener('click', pick);
        li.addEventListener('keydown', function (ev) {
          if (ev.key === 'Enter' || ev.key === ' ' || ev.key === 'Spacebar') { ev.preventDefault(); pick(ev); }
          else if (ev.key === 'Escape') { ev.preventDefault(); setOpen(false); trigger.focus(); }
        });
      })(opts[i]);
    }
  }

  function wireFilters() {
    wireDropdown(el('status-filter'), function (val) {
      state.status = val;
      state.offset = 0;
      loadPayments({ quiet: true });
    });
    wireDropdown(el('currency-filter'), function (val) {
      state.currency = val;
      state.offset = 0;
      loadPayments({ quiet: true });
    });
    wireDropdown(el('draft-currency'), null);

    document.addEventListener('click', function () { closeAllMenus(null); });
    document.addEventListener('keydown', function (ev) {
      if (ev.key === 'Escape') closeAllMenus(null);
    });
  }

  var syncing = false;

  function refreshAll() {
    loadSummary();
    loadPayments({ quiet: true });
    loadNotifications();
    loadDrafts();
  }

  function doSync() {
    if (syncing) return Promise.resolve();
    syncing = true;
    var btn = el('sync-now');
    var eb = el('empty-sync');
    if (btn) { btn.setAttribute('data-state', 'syncing'); btn.disabled = true; }
    if (eb) eb.disabled = true;
    return api('/api/sync', { method: 'POST' }).then(function (r) {
      syncing = false;
      if (btn) { btn.setAttribute('data-state', 'idle'); btn.disabled = false; }
      if (eb) eb.disabled = false;
      if (r.ok && r.body) {
        var b = r.body;
        notice('Sync complete \u2014 ' + (Number(b.fetched) || 0).toLocaleString() + ' fetched, ' +
          (Number(b.inserted) || 0).toLocaleString() + ' new, ' +
          (Number(b.updated) || 0).toLocaleString() + ' updated.', 'ok');
        setSticky('');
      } else if (r.status === 0) {
        notice('Sync failed \u2014 the ledger service is unreachable. The data already on screen is still current as of the last sync.', 'error');
      } else {
        notice(envelopeMessage(r.body, 'Sync failed \u2014 the payment vendor did not answer. Local data is still shown; try again shortly.'), 'error');
      }
      refreshAll();
    });
  }

  var notifRows = [];

  function renderNotifications(rows) {
    var host = el('notifications');
    if (!host) return;
    var sorted = rows.slice().sort(function (a, b) {
      return (Number(b.event_seq) || 0) - (Number(a.event_seq) || 0);
    });
    var frag = document.createDocumentFragment();
    for (var i = 0; i < sorted.length; i++) {
      var n = sorted[i];
      var d = document.createElement('div');
      d.className = 'notif';
      d.setAttribute('data-event-seq', String(n.event_seq));
      d.setAttribute('data-kind', String(n.kind || ''));
      var seq = document.createElement('span');
      seq.className = 'n-seq';
      seq.textContent = '#' + String(n.event_seq);
      var kind = document.createElement('span');
      kind.className = 'n-kind';
      kind.textContent = String(n.kind || '');
      var msg = document.createElement('span');
      msg.className = 'n-msg';
      msg.textContent = String(n.message || '');
      var at = document.createElement('span');
      at.className = 'n-at';
      at.textContent = fmtDate(n.at);
      d.appendChild(seq);
      d.appendChild(kind);
      d.appendChild(msg);
      d.appendChild(at);
      frag.appendChild(d);
    }
    if (!sorted.length) {
      var p = document.createElement('p');
      p.className = 'n-msg';
      p.textContent = 'No notifications yet.';
      frag.appendChild(p);
    }
    host.textContent = '';
    host.appendChild(frag);
  }

  function loadNotifications() {
    return api('/api/notifications?limit=50&offset=0').then(function (r) {
      var host = el('notifications');
      if (r.ok && r.body) {
        notifRows = r.body.data || [];
        renderNotifications(notifRows);
        if (host) host.setAttribute('data-state', 'live');
        show(el('notif-degraded'), false);
        return true;
      }
      if (host) host.setAttribute('data-state', 'degraded');
      show(el('notif-degraded'), true);
      if (notifRows.length) renderNotifications(notifRows);
      return false;
    });
  }

  function currentToken() {
    var t = el('role-token');
    return t ? String(t.value || '').trim() : '';
  }

  var TOKEN_KEY = 'mpc.role-token';
  var selectedDraft = null;
  var draftRows = [];

  function initToken() {
    var t = el('role-token');
    if (!t) return;
    var saved = null;
    try { saved = localStorage.getItem(TOKEN_KEY); } catch (e) { saved = null; }
    if (saved) t.value = saved;
    t.addEventListener('input', function () {
      try { localStorage.setItem(TOKEN_KEY, t.value); } catch (e) {}
    });
    t.addEventListener('change', function () {
      try { localStorage.setItem(TOKEN_KEY, t.value); } catch (e) {}
      loadDrafts();
    });
  }

  var TERMINAL = { rejected: 1, approved: 1, sent: 1 };

  function updateDraftButtons() {
    var st = null;
    for (var i = 0; i < draftRows.length; i++) {
      if (draftRows[i].id === selectedDraft) { st = String(draftRows[i].state || ''); break; }
    }
    var sub = el('submit-btn');
    var app = el('approve-btn');
    var rej = el('reject-btn');
    var legalSubmit = st === 'draft';
    var legalCheck = st === 'submitted';
    if (st && TERMINAL[st]) { legalSubmit = false; legalCheck = false; }
    if (sub) sub.disabled = !legalSubmit;
    if (app) app.disabled = !legalCheck;
    if (rej) rej.disabled = !legalCheck;
  }

  function renderDrafts(rows) {
    var host = el('draft-list');
    if (!host) return;
    var frag = document.createDocumentFragment();
    for (var i = 0; i < rows.length; i++) {
      var d = rows[i];
      var cp = d.counterparty || {};
      var row = document.createElement('div');
      row.setAttribute('data-draft-id', d.id);
      row.setAttribute('data-state', String(d.state || ''));
      row.setAttribute('data-selected', d.id === selectedDraft ? 'true' : 'false');
      var amt = document.createElement('div');
      amt.className = 'd-amount';
      amt.textContent = fmtMoney(d.amount_minor, d.currency) + ' ' + String(d.currency || '');
      var cpEl = document.createElement('div');
      cpEl.className = 'd-cp';
      cpEl.textContent = String(cp.name || '\u2014') + ' \u00B7 ' + String(cp.country || '') +
        ' \u00B7 ' + fmtDate(d.created_at);
      var chip = document.createElement('span');
      chip.className = 'd-chip';
      chip.textContent = String(d.state || '');
      row.appendChild(amt);
      row.appendChild(cpEl);
      row.appendChild(chip);
      frag.appendChild(row);
    }
    if (!rows.length) {
      var p = document.createElement('p');
      p.className = 'd-cp';
      p.textContent = 'No drafts yet. Create one above with a maker token.';
      frag.appendChild(p);
    }
    host.textContent = '';
    host.appendChild(frag);
    updateDraftButtons();
  }

  function loadDrafts() {
    if (!currentToken()) {
      draftRows = [];
      var host = el('draft-list');
      if (host) {
        host.textContent = '';
        var p = document.createElement('p');
        p.className = 'd-cp';
        p.textContent = 'Enter a role token to load drafts.';
        host.appendChild(p);
      }
      updateDraftButtons();
      return Promise.resolve(false);
    }
    return api('/api/drafts', { auth: true }).then(function (r) {
      if (r.ok && r.body) {
        draftRows = r.body.data || [];
        var still = false;
        for (var i = 0; i < draftRows.length; i++) if (draftRows[i].id === selectedDraft) still = true;
        if (!still) selectedDraft = null;
        renderDrafts(draftRows);
        return true;
      }
      authNotice(r, 'Drafts could not be loaded.');
      return false;
    });
  }

  function authNotice(r, fallback) {
    var code = envelopeCode(r.body);
    if (code === 'approval_forbidden') {
      notice('The approver must be a different user than the submitter.', 'error');
    } else if (code === 'forbidden' || r.status === 403) {
      notice(envelopeMessage(r.body, 'That token is not allowed to perform this action \u2014 switch to a token with the right role.'), 'error');
    } else if (code === 'unauthorized' || r.status === 401) {
      notice('The role token was rejected. Enter a valid maker or checker token and try again.', 'error');
    } else if (r.status === 0) {
      notice('The ledger service is unreachable \u2014 ' + fallback, 'error');
    } else {
      notice(envelopeMessage(r.body, fallback), 'error');
    }
  }

  function wireDrafts() {
    initToken();

    var form = el('draft-form');
    if (form) form.addEventListener('submit', function (ev) {
      ev.preventDefault();
      if (!currentToken()) {
        notice('Enter a role token before creating a draft.', 'warn');
        return;
      }
      var amountEl = el('draft-amount');
      var raw = amountEl ? String(amountEl.value || '').trim() : '';
      if (!/^\d+$/.test(raw)) {
        notice('Amount must be a whole number of minor units \u2014 digits only, no separators.', 'warn');
        if (amountEl) amountEl.focus();
        return;
      }
      var amount = parseInt(raw, 10);
      if (!(amount > 0)) {
        notice('Amount must be greater than zero.', 'warn');
        return;
      }
      var dc = el('draft-currency');
      var currency = dc ? String(dc.getAttribute('data-value') || 'EUR').toUpperCase() : 'EUR';
      var nameEl = el('draft-cp-name');
      var countryEl = el('draft-cp-country');
      var noteEl = el('draft-note');
      var name = nameEl ? String(nameEl.value || '').trim() : '';
      var country = countryEl ? String(countryEl.value || '').trim().toUpperCase() : '';
      if (countryEl) countryEl.value = country;
      if (!name) {
        notice('Counterparty name is required.', 'warn');
        if (nameEl) nameEl.focus();
        return;
      }
      if (!country) {
        notice('Counterparty country is required (2-letter code).', 'warn');
        if (countryEl) countryEl.focus();
        return;
      }
      var payload = {
        amount_minor: amount,
        currency: currency,
        counterparty: { name: name, country: country },
        note: noteEl ? String(noteEl.value || '') : ''
      };
      var btn = el('create-draft');
      if (btn) btn.disabled = true;
      api('/api/drafts', { method: 'POST', body: payload, auth: true }).then(function (r) {
        if (btn) btn.disabled = false;
        if (r.ok && r.body) {
          notice('Draft created for ' + fmtMoney(amount, currency) + '.', 'ok');
          if (amountEl) amountEl.value = '';
          if (nameEl) nameEl.value = '';
          if (countryEl) countryEl.value = '';
          if (noteEl) noteEl.value = '';
          selectedDraft = r.body.id || selectedDraft;
          loadDrafts();
          loadNotifications();
          return;
        }
        authNotice(r, 'The draft could not be created.');
      });
    });

    var list = el('draft-list');
    if (list) list.addEventListener('click', function (ev) {
      var row = ev.target.closest ? ev.target.closest('[data-draft-id]') : null;
      if (!row) return;
      selectedDraft = row.getAttribute('data-draft-id');
      var all = list.querySelectorAll('[data-draft-id]');
      for (var i = 0; i < all.length; i++) {
        if (all[i] === row) all[i].setAttribute('data-selected', 'true');
        else all[i].removeAttribute('data-selected');
      }
      updateDraftButtons();
    });

    wireDraftAction('submit-btn', 'submit', 'submitted for approval');
    wireDraftAction('approve-btn', 'approve', 'approved');
    wireDraftAction('reject-btn', 'reject', 'rejected');
  }

  function wireDraftAction(btnId, action, word) {
    var btn = el(btnId);
    if (!btn) return;
    btn.addEventListener('click', function () {
      if (!selectedDraft) {
        notice('Select a draft first.', 'warn');
        return;
      }
      if (!currentToken()) {
        notice('Enter a role token before acting on a draft.', 'warn');
        return;
      }
      btn.disabled = true;
      api('/api/drafts/' + encodeURIComponent(selectedDraft) + '/' + action, { method: 'POST', auth: true })
        .then(function (r) {
          if (r.ok) {
            notice('Draft ' + word + '.', 'ok');
          } else {
            authNotice(r, 'The draft could not be ' + word + '.');
          }
          return loadDrafts().then(function () {
            loadNotifications();
            loadSummary();
            loadPayments({ quiet: true });
          });
        });
    });
  }

  function pollHealth() {
    api('/api/health').then(function (r) {
      if (!r.ok || !r.body) {
        setSticky('The ledger service is not answering health checks \u2014 the figures on screen may be stale.');
        return;
      }
      var h = r.body;
      var payments = Number(h.payments) || 0;
      if (!h.last_sync) {
        setSticky('No sync has run yet \u2014 press Sync now to pull the vendor ledger.');
      } else if (h.status && h.status !== 'ok' && h.status !== 'healthy') {
        setSticky('Vendor connection degraded (' + String(h.status) + ') \u2014 showing the last known ledger.');
      } else {
        if (stickyNotice) {
          stickyNotice = '';
          var n = el('notice');
          if (n && n.getAttribute('data-kind') === 'warn') {
            n.textContent = '';
            n.removeAttribute('data-kind');
          }
        }
      }
      if (state.lastHealthPayments >= 0 && payments !== state.lastHealthPayments) {
        loadSummary();
        loadPayments({ quiet: true });
      }
      state.lastHealthPayments = payments;
    });
  }

  window.AppAPI = {
    fmtMoney: fmtMoney,
    fmtDate: fmtDate,
    notice: notice,
    revealPayment: revealPayment
  };

  function init() {
    wireTable();
    wireFilters();
    wireDrafts();

    var sb = el('sync-now');
    if (sb) {
      sb.setAttribute('data-state', 'idle');
      sb.addEventListener('click', function () { doSync(); });
    }
    var esb = el('empty-sync');
    if (esb) esb.addEventListener('click', function () { doSync(); });

    tableState('empty');

    loadSummary();
    loadPayments();
    loadNotifications();
    loadDrafts();
    pollHealth();

    setInterval(loadNotifications, 4000);
    setInterval(pollHealth, 5000);

    if (window.VizAPI) {
      if (VizAPI.onBrushChange) {
        try { VizAPI.onBrushChange(function (ids) { repaintBrush(ids); }); } catch (e) {}
      }
      if (VizAPI.init) {
        try { VizAPI.init(); } catch (e) {}
      }
    }
  }

  if (document.readyState === 'loading') {
    document.addEventListener('DOMContentLoaded', init);
  } else {
    init();
  }

  window.addEventListener('unhandledrejection', function (ev) {
    ev.preventDefault();
  });
})();
