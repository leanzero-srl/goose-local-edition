/* VendorSync Pro — page behavior: summary, table, filters, sync, optimistic notes, viz wiring.
 * Server-driven pagination/filtering/sorting throughout; the page never fetches the whole
 * collection. No native selects, no alert/confirm/prompt — custom dropdowns, inline editors,
 * and the #notice status element. */
'use strict';

(function () {
  const STATUSES = ['settled', 'pending', 'refunded', 'failed'];
  const CURRENCIES = ['EUR', 'USD', 'JPY', 'KWD'];
  const EXPONENT = { EUR: 2, USD: 2, JPY: 0, KWD: 3 };

  const $ = (id) => document.getElementById(id);
  const state = { limit: 50, offset: 0, status: '', currency: '', sortKey: 'created_at', desc: false };
  let currentRows = [];
  let latestBuckets = null;
  let viz = null;
  let vizSupported = true;

  // ── formatting ──────────────────────────────────────────────────────────────────────────────
  const dayFmt = new Intl.DateTimeFormat('en-GB', { day: 'numeric', month: 'short', year: 'numeric' });
  const dateTimeFmt = new Intl.DateTimeFormat(undefined, {
    day: 'numeric', month: 'short', year: 'numeric', hour: '2-digit', minute: '2-digit',
  });
  // Seconds on purpose: two syncs inside the same minute must still read as two syncs.
  const lastSyncFmt = new Intl.DateTimeFormat(undefined, {
    day: 'numeric', month: 'short', year: 'numeric',
    hour: '2-digit', minute: '2-digit', second: '2-digit',
  });

  function fmtDay(iso) {
    return dayFmt.format(new Date(iso + 'T00:00:00'));
  }

  function fmtDateTime(iso) {
    return dateTimeFmt.format(new Date(iso));
  }

  function fmtLastSync(iso) {
    return lastSyncFmt.format(new Date(iso));
  }

  function fmtMoney(minor, currency) {
    const exp = EXPONENT[currency] != null ? EXPONENT[currency] : 2;
    const value = minor / Math.pow(10, exp);
    return new Intl.NumberFormat(undefined, {
      style: 'currency', currency: currency,
      minimumFractionDigits: exp, maximumFractionDigits: exp,
    }).format(value);
  }

  // ── plumbing ────────────────────────────────────────────────────────────────────────────────
  async function fetchJSON(url, options) {
    const resp = await fetch(url, options);
    let body = null;
    try { body = await resp.json(); } catch (err) { /* non-JSON error body */ }
    if (!resp.ok) {
      const error = new Error((body && body.error && body.error.message) || ('HTTP ' + resp.status));
      error.status = resp.status;
      error.code = body && body.error && body.error.code;
      throw error;
    }
    return body;
  }

  let noticeTimer = null;
  function showNotice(message, ok) {
    const el = $('notice');
    el.textContent = message;
    el.classList.toggle('ok', !!ok);
    el.hidden = false;
    clearTimeout(noticeTimer);
    noticeTimer = setTimeout(() => { el.hidden = true; }, 4000);
  }

  // ── custom dropdowns ────────────────────────────────────────────────────────────────────────
  function makeDropdown(container, allLabel, values, onChange) {
    const button = document.createElement('button');
    button.type = 'button';
    button.className = 'dd-button';
    button.setAttribute('aria-haspopup', 'listbox');
    const label = document.createElement('span');
    label.textContent = allLabel;
    button.appendChild(label);
    const list = document.createElement('ul');
    list.className = 'dd-list';
    list.setAttribute('role', 'listbox');
    const options = [{ value: '', label: allLabel }]
      .concat(values.map((v) => ({ value: v, label: v.charAt(0).toUpperCase() + v.slice(1) })));
    for (const opt of options) {
      const li = document.createElement('li');
      li.setAttribute('role', 'option');
      li.dataset.option = opt.value;
      li.textContent = opt.label;
      li.addEventListener('click', () => {
        select(opt.value, true);
        container.classList.remove('open');
      });
      list.appendChild(li);
    }
    button.addEventListener('click', () => container.classList.toggle('open'));
    container.appendChild(button);
    container.appendChild(list);

    function select(value, fire) {
      container.dataset.value = value;
      container.setAttribute('data-value', value);
      const opt = options.find((o) => o.value === value) || options[0];
      label.textContent = opt.label;
      for (const li of list.children) {
        li.classList.toggle('selected', li.dataset.option === opt.value);
      }
      if (fire) onChange(opt.value);
    }
    select('', false);
    return { select: select };
  }

  document.addEventListener('click', (e) => {
    for (const dd of document.querySelectorAll('.dropdown.open')) {
      if (!dd.contains(e.target)) dd.classList.remove('open');
    }
  });

  // ── summary ─────────────────────────────────────────────────────────────────────────────────
  async function loadSummary() {
    const summary = await fetchJSON('/api/summary');
    const wrap = $('cur-totals');
    wrap.textContent = '';
    for (const entry of summary.by_currency) {
      const card = document.createElement('div');
      card.className = 'cur-total';
      card.setAttribute('data-currency', entry.currency);
      card.innerHTML = '<div class="cur-code">' + entry.currency + '</div>' +
        '<div class="cur-amount"></div><div class="cur-count"></div>';
      card.querySelector('.cur-amount').textContent = fmtMoney(entry.total_minor, entry.currency);
      card.querySelector('.cur-count').textContent = entry.count + ' payments';
      wrap.appendChild(card);
    }
    $('last-sync').textContent = summary.last_sync ? fmtLastSync(summary.last_sync) : 'Never synced';
    return summary;
  }

  // ── table ───────────────────────────────────────────────────────────────────────────────────
  function sortParam() {
    return (state.desc ? '-' : '') + state.sortKey;
  }

  async function loadTable() {
    const params = new URLSearchParams({
      limit: String(state.limit), offset: String(state.offset), sort: sortParam(),
    });
    if (state.status) params.set('status', state.status);
    if (state.currency) params.set('currency', state.currency);
    const page = await fetchJSON('/api/payments?' + params.toString());
    currentRows = page.data;
    renderRows(page);
    return page;
  }

  function renderRows(page) {
    const tbody = $('rows');
    tbody.textContent = '';
    for (const row of page.data) {
      tbody.appendChild(renderRow(row));
    }
    if (!page.data.length) {
      const tr = document.createElement('tr');
      const td = document.createElement('td');
      td.colSpan = 5;
      td.textContent = 'No payments match the current filters.';
      tr.appendChild(td);
      tbody.appendChild(tr);
    }
    const from = page.total === 0 ? 0 : page.offset + 1;
    const to = Math.min(page.offset + page.data.length, page.total);
    $('range').textContent = 'Showing ' + from + '–' + to + ' of ' + page.total;
    $('prev').disabled = page.offset <= 0;
    $('next').disabled = page.offset + page.data.length >= page.total;
    $('table-wrap').hidden = false;
  }

  function renderRow(row) {
    const tr = document.createElement('tr');
    tr.dataset.id = row.id;
    const dateTd = document.createElement('td');
    dateTd.textContent = fmtDateTime(row.created_at);
    const amountTd = document.createElement('td');
    amountTd.className = 'amount';
    amountTd.dataset.currency = row.currency;
    amountTd.textContent = fmtMoney(row.amount_minor, row.currency);
    const statusTd = document.createElement('td');
    const badge = document.createElement('span');
    badge.className = 'badge ' + row.status;
    badge.textContent = row.status;
    statusTd.appendChild(badge);
    const cpTd = document.createElement('td');
    cpTd.textContent = row.counterparty_name + (row.country ? ' · ' + row.country : '');
    const noteTd = document.createElement('td');
    noteTd.className = 'note';
    renderNoteCell(noteTd, tr, row);
    tr.append(dateTd, amountTd, statusTd, cpTd, noteTd);
    return tr;
  }

  function renderNoteCell(td, tr, row) {
    td.textContent = '';
    const span = document.createElement('span');
    span.className = 'note-text' + (row.note ? '' : ' empty');
    span.textContent = row.note || 'Add note';
    span.title = 'Click to edit';
    span.addEventListener('click', () => openNoteEditor(td, tr, row));
    td.appendChild(span);
  }

  function openNoteEditor(td, tr, row) {
    td.textContent = '';
    const box = document.createElement('div');
    box.className = 'note-editor';
    const input = document.createElement('input');
    input.type = 'text';
    input.maxLength = 280;
    input.value = row.note || '';
    const save = document.createElement('button');
    save.type = 'button';
    save.className = 'save';
    save.textContent = 'Save';
    const cancel = document.createElement('button');
    cancel.type = 'button';
    cancel.className = 'cancel';
    cancel.textContent = 'Cancel';
    box.append(input, save, cancel);
    td.appendChild(box);
    input.focus();
    const commit = () => {
      const value = input.value.trim();
      if (!value || value === row.note) {
        renderNoteCell(td, tr, row);
        return;
      }
      saveNote(td, tr, row, value);
    };
    save.addEventListener('click', commit);
    cancel.addEventListener('click', () => renderNoteCell(td, tr, row));
    input.addEventListener('keydown', (e) => {
      if (e.key === 'Enter') commit();
      if (e.key === 'Escape') renderNoteCell(td, tr, row);
    });
  }

  function saveNote(td, tr, row, value) {
    const previous = row.note || '';
    // Optimistic: paint the new value NOW, before the network answers.
    row.note = value;
    renderNoteCell(td, tr, row);
    tr.dataset.state = 'saving';
    fetchJSON('/api/payments/' + encodeURIComponent(row.id) + '/note', {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({ note: value }),
    }).then((resp) => {
      row.version = resp.version;
      row.note = resp.note;
      renderNoteCell(td, tr, row);
      tr.dataset.state = 'saved';
    }).catch((err) => {
      row.note = previous;
      renderNoteCell(td, tr, row);
      tr.dataset.state = '';
      if (err.status === 409) {
        showNotice('Someone else edited this payment first — your note was not saved.');
      } else {
        showNotice('The note could not be saved: ' + err.message);
      }
    });
  }

  // ── viz panel ───────────────────────────────────────────────────────────────────────────────
  function allZero(buckets) {
    return !(buckets.cells || []).some((c) => c.count > 0);
  }

  function renderFallbackTable(buckets) {
    const tbody = $('viz-fallback').querySelector('tbody');
    tbody.textContent = '';
    const byKey = {};
    for (const cell of buckets.cells || []) byKey[cell.day + '|' + cell.status] = cell.count;
    for (const day of buckets.days || []) {
      const tr = document.createElement('tr');
      const dayTd = document.createElement('td');
      dayTd.className = 'day';
      dayTd.textContent = fmtDay(day);
      tr.appendChild(dayTd);
      let total = 0;
      for (const status of STATUSES) {
        const td = document.createElement('td');
        const count = byKey[day + '|' + status] || 0;
        total += count;
        td.setAttribute('data-day', day);
        td.setAttribute('data-status', status);
        td.textContent = String(count);
        tr.appendChild(td);
      }
      const totalTd = document.createElement('td');
      totalTd.textContent = String(total);
      tr.appendChild(totalTd);
      tbody.appendChild(tr);
    }
  }

  function renderLegend() {
    const legend = $('viz-legend');
    legend.textContent = '';
    const colors = { settled: '#16A34A', pending: '#F59E0B', refunded: '#8B5CF6', failed: '#DC2626' };
    for (const status of STATUSES) {
      const chip = document.createElement('span');
      chip.className = 'chip';
      const swatch = document.createElement('span');
      swatch.className = 'swatch';
      swatch.style.background = colors[status];
      chip.appendChild(swatch);
      chip.appendChild(document.createTextNode(status));
      legend.appendChild(chip);
    }
  }

  function showFallbackOnly() {
    $('viz3d').hidden = true;
    $('viz-fallback').hidden = false;
    $('viz-toggle').setAttribute('aria-pressed', 'true');
  }

  async function loadBuckets() {
    let buckets;
    try {
      buckets = await fetchJSON('/api/buckets');
    } catch (err) {
      $('viz-error').hidden = false;
      $('viz-empty').hidden = true;
      $('viz3d').hidden = true;
      $('viz-fallback').hidden = true;
      return null;
    }
    latestBuckets = buckets;
    $('viz-error').hidden = true;
    $('viz-empty').hidden = !allZero(buckets);
    renderFallbackTable(buckets);
    if (vizSupported) {
      viz.setBuckets(buckets);
    }
    return buckets;
  }

  function initViz() {
    viz = window.VsproViz.mount($('viz3d'), {
      tooltipEl: $('viz-tooltip'),
      formatDay: fmtDay,
      onPickStatus: (status) => setStatusFilter(status),
    });
    vizSupported = viz.supported;
    if (!vizSupported) {
      // No WebGL on this machine: degrade to the 2D table, visibly, without crashing.
      $('viz-nogl').hidden = false;
      showFallbackOnly();
    }
    renderLegend();
  }

  $('viz-toggle').addEventListener('click', () => {
    const button = $('viz-toggle');
    const pressed = button.getAttribute('aria-pressed') === 'true';
    if (!vizSupported) {
      // The canvas cannot come back without WebGL; the table stays.
      button.setAttribute('aria-pressed', 'true');
      return;
    }
    const next = !pressed;
    button.setAttribute('aria-pressed', String(next));
    $('viz-fallback').hidden = !next;
    $('viz3d').hidden = next;
    if (!next) viz.render();
  });

  // ── filters, sorting, paging ────────────────────────────────────────────────────────────────
  let statusDropdown = null;

  function setStatusFilter(status) {
    statusDropdown.select(status, false);
    state.status = status;
    state.offset = 0;
    loadTable().catch(() => showNotice('The payment list could not be refreshed.'));
  }

  function initControls() {
    statusDropdown = makeDropdown($('status-filter'), 'All statuses', STATUSES, (value) => {
      state.status = value;
      state.offset = 0;
      loadTable().catch(() => showNotice('The payment list could not be refreshed.'));
    });
    makeDropdown($('currency-filter'), 'All currencies', CURRENCIES, (value) => {
      state.currency = value;
      state.offset = 0;
      loadTable().catch(() => showNotice('The payment list could not be refreshed.'));
    });

    for (const th of document.querySelectorAll('th.sortable')) {
      th.addEventListener('click', () => {
        const key = th.dataset.sort;
        if (state.sortKey === key) {
          state.desc = !state.desc;
        } else {
          state.sortKey = key;
          state.desc = false;
        }
        for (const other of document.querySelectorAll('th.sortable')) {
          other.setAttribute('aria-sort', other === th ? (state.desc ? 'descending' : 'ascending') : 'none');
        }
        state.offset = 0;
        loadTable().catch(() => showNotice('The payment list could not be refreshed.'));
      });
    }

    $('prev').addEventListener('click', () => {
      state.offset = Math.max(0, state.offset - state.limit);
      loadTable().catch(() => showNotice('The payment list could not be refreshed.'));
    });
    $('next').addEventListener('click', () => {
      state.offset += state.limit;
      loadTable().catch(() => showNotice('The payment list could not be refreshed.'));
    });

    $('sync-now').addEventListener('click', doSync);
    $('empty-sync').addEventListener('click', doSync);
  }

  async function doSync() {
    const button = $('sync-now');
    button.dataset.state = 'syncing';
    button.disabled = true;
    $('empty-sync').disabled = true;
    try {
      const result = await fetchJSON('/api/sync', { method: 'POST' });
      showNotice('Synced ' + result.fetched + ' payments (' + result.inserted + ' new, ' +
        result.updated + ' updated).', true);
      $('state-empty').hidden = true;
      await Promise.all([loadSummary(), loadTable(), loadBuckets()]);
    } catch (err) {
      showNotice('Sync failed: ' + err.message);
    } finally {
      button.dataset.state = '';
      button.disabled = false;
      $('empty-sync').disabled = false;
    }
  }

  // ── boot ────────────────────────────────────────────────────────────────────────────────────
  async function boot() {
    initViz();
    initControls();
    $('state-loading').hidden = false;
    try {
      const [page] = await Promise.all([loadTable(), loadSummary(), loadBuckets()]);
      $('state-loading').hidden = true;
      const nothingLocal = page.total === 0 && !state.status && !state.currency;
      $('state-empty').hidden = !nothingLocal;
      $('table-wrap').hidden = nothingLocal;
    } catch (err) {
      $('state-loading').hidden = true;
      $('table-wrap').hidden = true;
      $('state-error').hidden = false;
    }
  }

  boot();
})();
