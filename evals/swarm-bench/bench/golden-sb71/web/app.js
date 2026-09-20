/* Meridian Payments Console — page behavior: summary, table, filters, notes, sync,
 * notifications feed, and the maker/checker drafts panel. viz.js owns the 3D engine. */
'use strict';

(function () {
  var fmtMoney = window.Viz.fmtMoney;
  var STATUSES = ['settled', 'pending', 'refunded', 'failed'];
  var CURRENCIES = ['EUR', 'USD', 'JPY', 'KWD'];

  var state = {
    limit: 50, offset: 0, sort: 'created_at', status: '', currency: '',
    total: 0, rows: [], loaded: false, token: '', drafts: [],
    selectedDraft: null, optimistic: {},
  };

  var $ = function (id) { return document.getElementById(id); };

  function fmtDate(iso) {
    if (!iso) return '';
    var d = new Date(iso);
    if (isNaN(d)) return '';
    return d.toLocaleString(undefined, {
      day: 'numeric', month: 'short', year: 'numeric',
      hour: '2-digit', minute: '2-digit',
    });
  }

  var noticeTimer = 0;
  function showNotice(text, kind, sticky) {
    var n = $('notice');
    n.textContent = text;
    n.className = kind === 'ok' ? 'ok' : '';
    n.hidden = false;
    if (noticeTimer) clearTimeout(noticeTimer);
    if (!sticky) noticeTimer = setTimeout(function () { n.hidden = true; }, 6000);
  }

  function api(path, opts) {
    opts = opts || {};
    var headers = opts.headers || {};
    if (opts.auth && state.token) headers.Authorization = 'Bearer ' + state.token;
    return fetch(path, {
      method: opts.method || 'GET',
      headers: Object.assign({ 'Content-Type': 'application/json' }, headers),
      body: opts.body ? JSON.stringify(opts.body) : undefined,
    });
  }

  // ── summary ────────────────────────────────────────────────────────────────────────────────
  function renderSummary(s) {
    var wrap = $('summary-cards');
    wrap.textContent = '';
    (s.by_currency || []).forEach(function (row) {
      var card = document.createElement('div');
      card.className = 'cur-total';
      card.setAttribute('data-currency', row.currency);
      card.innerHTML = '<div class="cur-code">' + row.currency +
        '</div><div class="cur-count">' + row.count.toLocaleString('en-US') +
        '</div><div class="cur-amount">' + fmtMoney(row.total_minor, row.currency) +
        '</div>';
      wrap.appendChild(card);
    });
    (s.reversals || []).forEach(function (row) {
      var card = document.createElement('div');
      card.className = 'rev-total';
      card.setAttribute('data-currency', row.currency);
      card.innerHTML = '<div class="cur-code">' + row.currency +
        ' reversals</div><div class="cur-count">' + row.count.toLocaleString('en-US') +
        '</div><div class="cur-amount">' + fmtMoney(row.total_minor, row.currency) +
        '</div>';
      wrap.appendChild(card);
    });
    $('last-sync').textContent = s.last_sync
      ? 'Last sync ' + fmtDate(s.last_sync) : 'Never synced';
  }

  function loadSummary() {
    return api('/api/summary').then(function (r) {
      if (!r.ok) throw new Error('summary ' + r.status);
      return r.json();
    }).then(renderSummary).catch(function () {});
  }

  // ── table ──────────────────────────────────────────────────────────────────────────────────
  function query() {
    var q = 'limit=' + state.limit + '&offset=' + state.offset + '&sort=' + state.sort;
    if (state.status) q += '&status=' + state.status;
    if (state.currency) q += '&currency=' + state.currency;
    return q;
  }

  function renderRows() {
    var body = $('table-body');
    body.textContent = '';
    state.rows.forEach(function (p) {
      var tr = document.createElement('tr');
      tr.setAttribute('data-id', p.id);
      if (window.Viz.brushHas(p.id)) tr.setAttribute('data-brushed', 'true');
      var cp = (p.counterparty_name || '') + (p.country ? ' · ' + p.country : '');
      tr.innerHTML =
        '<td class="date">' + fmtDate(p.created_at) + '</td>' +
        '<td class="amount">' + fmtMoney(p.amount_minor, p.currency) + '</td>' +
        '<td><span class="badge ' + p.status + '">' + p.status + '</span></td>' +
        '<td class="cp"></td>' +
        '<td class="note-cell"></td>';
      tr.querySelector('.cp').textContent = cp;
      tr.querySelector('.note-cell').textContent = p.note || '—';
      tr.addEventListener('click', function (e) {
        if (e.target.closest('.note-edit')) return;
        if (e.target.closest('td') && e.target.closest('td').classList
            .contains('note-cell')) {
          openNoteEditor(tr, p);
          return;
        }
        window.Viz.toggleBrush(p.id);
      });
      body.appendChild(tr);
    });
    var readout = $('range-readout');
    if (state.total === 0) {
      readout.textContent = '';
    } else {
      var from = state.offset + 1;
      var to = state.offset + state.rows.length;
      readout.textContent = 'Showing ' + from.toLocaleString('en-US') + '–' +
        to.toLocaleString('en-US') + ' of ' + state.total.toLocaleString('en-US');
    }
    $('prev').disabled = state.offset <= 0;
    $('next').disabled = state.offset + state.rows.length >= state.total;
  }

  function setTableState(text) {
    var el = $('table-state');
    if (!text) {
      el.hidden = true;
      el.textContent = '';
    } else {
      el.hidden = false;
      el.textContent = text;
    }
  }

  function loadTable() {
    return api('/api/payments?' + query()).then(function (r) {
      if (!r.ok) throw new Error('payments ' + r.status);
      return r.json();
    }).then(function (body) {
      state.rows = body.data || [];
      state.total = body.total || 0;
      state.loaded = true;
      renderRows();
      if (state.total === 0) {
        setTableState('No payments yet — the first sync is running; rows appear '
                      + 'as data lands.');
      } else {
        setTableState(null);
      }
    }).catch(function () {
      setTableState('Backend unreachable — check the service is running, '
                    + 'then retry.');
      showNotice('Backend unreachable — check the service is running, then retry.',
                 'err', true);
    });
  }

  // ── note editor (custom inline, optimistic) ────────────────────────────────────────────────
  function openNoteEditor(tr, p) {
    var cell = tr.querySelector('.note-cell');
    if (cell.querySelector('.note-edit')) return;
    var prev = p.note || '';
    cell.textContent = '';
    var wrap = document.createElement('div');
    wrap.className = 'note-edit';
    var input = document.createElement('input');
    input.value = prev;
    input.maxLength = 280;
    var save = document.createElement('button');
    save.type = 'button';
    save.className = 'note-save';
    save.textContent = 'Save';
    var cancel = document.createElement('button');
    cancel.type = 'button';
    cancel.textContent = 'Cancel';
    wrap.appendChild(input);
    wrap.appendChild(save);
    wrap.appendChild(cancel);
    cell.appendChild(wrap);
    input.focus();
    var close = function (text) {
      cell.textContent = text || '—';
    };
    cancel.addEventListener('click', function (e) {
      e.stopPropagation();
      close(prev);
    });
    save.addEventListener('click', function (e) {
      e.stopPropagation();
      var value = input.value.trim();
      if (!value) { close(prev); return; }
      close(value);                                  // optimistic — paints immediately
      tr.setAttribute('data-state', 'saving');
      api('/api/payments/' + p.id + '/note', { method: 'POST', body: { note: value } })
        .then(function (r) {
          if (r.status === 409) {
            close(prev);
            tr.removeAttribute('data-state');
            showNotice('Someone updated this payment first — the note was not '
                       + 'saved. Reload and retry.');
            return;
          }
          if (!r.ok) throw new Error('note ' + r.status);
          return r.json().then(function (body) {
            p.note = body.note;
            p.version = body.version;
            tr.setAttribute('data-state', 'saved');
          });
        })
        .catch(function () {
          close(prev);
          tr.removeAttribute('data-state');
          showNotice('The note could not be saved — the vendor is unreachable.');
        });
    });
  }

  // ── sorting, filters, paging ───────────────────────────────────────────────────────────────
  function setSort(key) {
    if (state.sort === key) state.sort = '-' + key;
    else if (state.sort === '-' + key) state.sort = key;
    else state.sort = key;
    var thDate = $('th-date'), thAmount = $('th-amount');
    thDate.removeAttribute('aria-sort');
    thAmount.removeAttribute('aria-sort');
    var active = state.sort.indexOf('created_at') >= 0 ? thDate : thAmount;
    active.setAttribute('aria-sort',
                        state.sort[0] === '-' ? 'descending' : 'ascending');
    state.offset = 0;
    loadTable();
  }

  function buildDropdown(rootId, label, values, apply) {
    var root = $(rootId);
    var btn = root.querySelector('.dd-btn');
    var menu = root.querySelector('.dd-menu');
    var items = [{ value: '', text: label }].concat(values.map(function (v) {
      return { value: v, text: v };
    }));
    items.forEach(function (it) {
      var el = document.createElement('div');
      el.className = 'dd-item' + (it.value === '' ? ' active' : '');
      el.textContent = it.text;
      el.addEventListener('click', function () {
        root.setAttribute('data-value', it.value);
        btn.textContent = it.value === '' ? label : it.text;
        menu.hidden = true;
        menu.querySelectorAll('.dd-item').forEach(function (x) {
          x.classList.toggle('active', x === el);
        });
        apply(it.value);
      });
      menu.appendChild(el);
    });
    btn.addEventListener('click', function (e) {
      e.stopPropagation();
      menu.hidden = !menu.hidden;
    });
    document.addEventListener('click', function () { menu.hidden = true; });
  }

  // ── brush link ─────────────────────────────────────────────────────────────────────────────
  function matchesFilters(item) {
    if (state.status && item.status !== state.status) return false;
    if (state.currency && item.currency !== state.currency) return false;
    return true;
  }

  function rankOf(item) {
    var items = window.Viz.store.items;
    var count = 0;
    for (var i = 0; i < items.length; i++) {
      var it = items[i];
      if (!matchesFilters(it)) continue;
      if (it.id === item.id) continue;
      var before;
      if (state.sort === 'created_at') {
        before = it.instant < item.instant ||
          (it.instant === item.instant && it.id < item.id);
      } else if (state.sort === '-created_at') {
        before = it.instant > item.instant ||
          (it.instant === item.instant && it.id > item.id);
      } else if (state.sort === 'amount_minor') {
        before = it.amount_minor < item.amount_minor ||
          (it.amount_minor === item.amount_minor && it.id < item.id);
      } else {
        before = it.amount_minor > item.amount_minor ||
          (it.amount_minor === item.amount_minor && it.id > item.id);
      }
      if (before) count++;
    }
    return count;
  }

  function navigateToPayment(id, flash) {
    var n = window.Viz.store.byId.get(id);
    if (n === undefined) return Promise.resolve(false);
    var item = window.Viz.store.items[n];
    if (!matchesFilters(item)) return Promise.resolve(false);
    var rank = rankOf(item);
    state.offset = Math.floor(rank / state.limit) * state.limit;
    return loadTable().then(function () {
      var row = document.querySelector('tr[data-id="' + id + '"]');
      if (row) {
        // Scroll the table's OWN pane only — never the page (the 3D panel must not
        // move under the user's pointer mid-interaction).
        var cont = row.closest('.table-scroll');
        if (cont) {
          cont.scrollTop += row.getBoundingClientRect().top -
            cont.getBoundingClientRect().top - 26;
        }
      }
      return true;
    });
  }

  window.Viz.onBrush(function (ids, info) {
    var set = {};
    ids.forEach(function (id) { set[id] = true; });
    document.querySelectorAll('#table-body tr').forEach(function (tr) {
      var id = tr.getAttribute('data-id');
      if (set[id]) tr.setAttribute('data-brushed', 'true');
      else tr.removeAttribute('data-brushed');
    });
    $('brush-count').textContent = ids.length + ' selected';
    if (info && info.added && info.from3d) navigateToPayment(info.id);
  });

  window.Viz.onBatch(function () {
    loadSummary();
  });

  // ── sync ───────────────────────────────────────────────────────────────────────────────────
  function runSync() {
    var btn = $('sync-now');
    btn.disabled = true;
    btn.setAttribute('data-state', 'syncing');
    btn.textContent = 'Syncing…';
    api('/api/sync', { method: 'POST' }).then(function (r) {
      if (!r.ok) {
        return r.json().catch(function () { return {}; }).then(function () {
          showNotice('Sync failed — the vendor is unreachable; showing local '
                     + 'data. Retry when the vendor is back.');
        });
      }
      return r.json().then(function () {
        showNotice('Sync complete.', 'ok');
      });
    }).catch(function () {
      showNotice('Sync failed — the vendor is unreachable; showing local data.');
    }).then(function () {
      btn.disabled = false;
      btn.removeAttribute('data-state');
      btn.textContent = 'Sync now';
      loadSummary();
      loadTable();
      pollNotifications();
    });
  }

  // ── notifications feed ─────────────────────────────────────────────────────────────────────
  function kindClass(kind) {
    if (/approved/.test(kind)) return 'approved';
    if (/rejected/.test(kind)) return 'rejected';
    if (/reversal/.test(kind)) return 'reversal';
    return '';
  }

  function pollNotifications() {
    return api('/api/notifications?limit=20').then(function (r) {
      if (!r.ok) throw new Error('feed ' + r.status);
      return r.json();
    }).then(function (body) {
      var list = $('notifications');
      list.setAttribute('data-state', 'live');
      $('feed-flag').textContent = 'LIVE';
      $('feed-flag').className = 'live';
      list.textContent = '';
      (body.data || []).forEach(function (row) {
        var li = document.createElement('li');
        li.className = 'notif-item';
        li.setAttribute('data-event-seq', row.event_seq);
        li.setAttribute('data-kind', row.kind);
        var chip = document.createElement('span');
        chip.className = 'ntf-kind ' + kindClass(row.kind);
        chip.textContent = row.kind;
        var msg = document.createElement('span');
        msg.textContent = row.message;
        var time = document.createElement('span');
        time.className = 'ntf-time';
        time.textContent = fmtDate(row.at);
        li.appendChild(chip);
        li.appendChild(msg);
        li.appendChild(time);
        list.appendChild(li);
      });
    }).catch(function () {
      var list = $('notifications');
      list.setAttribute('data-state', 'degraded');
      $('feed-flag').textContent = 'DEGRADED';
      $('feed-flag').className = 'degraded';
    });
  }

  // ── drafts ─────────────────────────────────────────────────────────────────────────────────
  var STATE_RANK = { draft: 0, submitted: 1, approved: 2, rejected: 2, sent: 3 };

  function effectiveState(d) {
    var opt = state.optimistic[d.id];
    if (opt && performance.now() < opt.until &&
        (STATE_RANK[d.state] || 0) < (STATE_RANK[opt.state] || 0)) {
      return opt.state;
    }
    return d.state;
  }

  function renderDrafts() {
    var list = $('draft-list');
    list.textContent = '';
    state.drafts.forEach(function (d) {
      var row = document.createElement('div');
      row.className = 'draft-row';
      row.setAttribute('data-draft-id', d.id);
      var st = effectiveState(d);
      row.setAttribute('data-state', st);
      if (state.selectedDraft === d.id) row.setAttribute('data-selected', 'true');
      var chip = document.createElement('span');
      chip.className = 'draft-state';
      chip.textContent = st;
      var idEl = document.createElement('span');
      idEl.textContent = d.id;
      var amt = document.createElement('span');
      amt.textContent = fmtMoney(d.amount_minor, d.currency);
      var cp = document.createElement('span');
      cp.textContent = (d.counterparty && d.counterparty.name) || '';
      row.appendChild(chip);
      row.appendChild(idEl);
      row.appendChild(amt);
      row.appendChild(cp);
      row.addEventListener('click', function () {
        state.selectedDraft = d.id;
        list.querySelectorAll('.draft-row').forEach(function (r2) {
          r2.toggleAttribute('data-selected', false);
          r2.removeAttribute('data-selected');
        });
        row.setAttribute('data-selected', 'true');
        updateActionButtons();
      });
      list.appendChild(row);
    });
    updateActionButtons();
  }

  function updateActionButtons() {
    var sel = state.drafts.find(function (d) { return d.id === state.selectedDraft; });
    var st = sel ? effectiveState(sel) : null;
    $('submit-btn').disabled = !(sel && st === 'draft' && state.token);
    $('approve-btn').disabled = !(sel && st === 'submitted' && state.token);
    $('reject-btn').disabled = !(sel && st === 'submitted' && state.token);
  }

  function loadDrafts() {
    if (!state.token) return Promise.resolve();
    return api('/api/drafts', { auth: true }).then(function (r) {
      if (r.status === 401) {
        showNotice('That token is not recognized.');
        throw new Error('unauthorized');
      }
      if (!r.ok) throw new Error('drafts ' + r.status);
      return r.json();
    }).then(function (body) {
      state.drafts = body.data || [];
      renderDrafts();
    }).catch(function () {});
  }

  function draftAction(kind) {
    var sel = state.drafts.find(function (d) { return d.id === state.selectedDraft; });
    if (!sel) return;
    var target = { submit: 'submitted', approve: 'approved', reject: 'rejected' }[kind];
    state.optimistic[sel.id] = { state: target, until: performance.now() + 4000 };
    renderDrafts();                                  // paints before the network answers
    api('/api/drafts/' + sel.id + '/' + kind, { method: 'POST', auth: true, body: {} })
      .then(function (r) {
        if (!r.ok) {
          delete state.optimistic[sel.id];
          return r.json().catch(function () { return {}; }).then(function (body) {
            var code = body && body.error && body.error.code;
            if (code === 'approval_forbidden') {
              showNotice('Four-eyes: the approver must not be the submitter.');
            } else if (r.status === 401) {
              showNotice('That token is not recognized.');
            } else if (r.status === 403) {
              showNotice('This token’s role cannot ' + kind + ' drafts.');
            } else {
              showNotice('The ' + kind + ' was refused: ' +
                         ((body.error && body.error.message) || r.status));
            }
            renderDrafts();
          });
        }
        return r.json().then(function (draft) {
          var i = state.drafts.findIndex(function (d) { return d.id === draft.id; });
          if (i >= 0) state.drafts[i] = draft;
          renderDrafts();
          loadDrafts();
          [120, 400, 900, 1800].forEach(function (ms) {
            setTimeout(pollNotifications, ms);
          });
          if (kind === 'approve') watchSent(draft.id);
        });
      })
      .catch(function () {
        delete state.optimistic[sel.id];
        showNotice('The ' + kind + ' could not reach the backend.');
        renderDrafts();
      });
  }

  function watchSent(draftId) {
    var t0 = performance.now();
    var tick = function () {
      if (performance.now() - t0 > 25000 || !state.token) return;
      api('/api/drafts', { auth: true }).then(function (r) {
        if (!r.ok) throw new Error('drafts ' + r.status);
        return r.json();
      }).then(function (body) {
        var d = (body.data || []).find(function (x) { return x.id === draftId; });
        if (d && d.state === 'sent' && d.sent_payment_id) {
          state.drafts = body.data;
          renderDrafts();
          waitForRecordThenNavigate(d.sent_payment_id, performance.now());
          return;
        }
        setTimeout(tick, 800);
      }).catch(function () { setTimeout(tick, 1200); });
    };
    setTimeout(tick, 800);
  }

  function waitForRecordThenNavigate(paymentId, t0) {
    if (window.Viz.store.byId.has(paymentId)) {
      navigateToPayment(paymentId);
      showNotice('Payment ' + paymentId + ' was created at the vendor and is now in '
                 + 'the table.', 'ok');
      return;
    }
    if (performance.now() - t0 > 15000) return;
    setTimeout(function () { waitForRecordThenNavigate(paymentId, t0); }, 600);
  }

  // ── wiring ─────────────────────────────────────────────────────────────────────────────────
  function init() {
    setTableState('Loading payments…');
    buildDropdown('status-filter', 'All statuses', STATUSES, function (v) {
      state.status = v;
      state.offset = 0;
      loadTable();
    });
    buildDropdown('currency-filter', 'All currencies', CURRENCIES, function (v) {
      state.currency = v;
      state.offset = 0;
      loadTable();
    });
    $('th-date').addEventListener('click', function () { setSort('created_at'); });
    $('th-amount').addEventListener('click', function () { setSort('amount_minor'); });
    $('prev').addEventListener('click', function () {
      state.offset = Math.max(0, state.offset - state.limit);
      loadTable();
    });
    $('next').addEventListener('click', function () {
      if (state.offset + state.limit < state.total) {
        state.offset += state.limit;
        loadTable();
      }
    });
    $('sync-now').addEventListener('click', runSync);
    $('set-token').addEventListener('click', function () {
      state.token = $('role-token').value.trim();
      $('role-name').textContent = state.token ? 'token active' : '';
      loadDrafts();
    });
    $('role-token').addEventListener('keydown', function (e) {
      if (e.key === 'Enter') {
        state.token = $('role-token').value.trim();
        $('role-name').textContent = state.token ? 'token active' : '';
        loadDrafts();
      }
    });
    $('draft-form').addEventListener('submit', function (e) {
      e.preventDefault();
      if (!state.token) {
        showNotice('Set a maker or checker token first.');
        return;
      }
      var amount = parseInt($('draft-amount').value, 10);
      var body = {
        amount_minor: isNaN(amount) ? $('draft-amount').value : amount,
        currency: $('draft-currency').value.trim().toUpperCase(),
        counterparty: {
          name: $('draft-cp-name').value.trim(),
          country: $('draft-country').value.trim().toUpperCase(),
        },
        note: $('draft-note').value,
      };
      api('/api/drafts', { method: 'POST', auth: true, body: body }).then(function (r) {
        if (!r.ok) {
          return r.json().catch(function () { return {}; }).then(function (b) {
            showNotice('The draft was refused: ' +
                       ((b.error && b.error.message) || r.status));
          });
        }
        return r.json().then(function () {
          $('draft-form').reset();
          loadDrafts();
        });
      }).catch(function () {
        showNotice('The draft could not reach the backend.');
      });
    });
    $('submit-btn').addEventListener('click', function () { draftAction('submit'); });
    $('approve-btn').addEventListener('click', function () { draftAction('approve'); });
    $('reject-btn').addEventListener('click', function () { draftAction('reject'); });

    loadSummary();
    loadTable();
    pollNotifications();
    setInterval(pollNotifications, 2500);
    setInterval(function () { if (state.token) loadDrafts(); }, 3000);
    window.Viz.start();
  }

  if (document.readyState === 'loading') {
    document.addEventListener('DOMContentLoaded', init);
  } else {
    init();
  }
})();
