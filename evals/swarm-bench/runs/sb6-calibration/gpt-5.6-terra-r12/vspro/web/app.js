(()=>{
'use strict';
const $ = selector => document.querySelector(selector);
const rows = $('#payment-rows');
const state = $('#table-state');
const notice = $('#notice');
const PAGE_SIZE = 50;
const REQUEST_TIMEOUT = 12000;
const exponents = {EUR:2, USD:2, JPY:0, KWD:3};
let page = {offset:0, limit:PAGE_SIZE, status:'', currency:'', sort:'created_at', total:0};
let tableRequest = 0;
let tableController = null;
let savedTimer = null;

function escapeHtml(value) { return String(value ?? '').replace(/[&<>"']/g, char => ({'&':'&amp;','<':'&lt;','>':'&gt;','"':'&quot;',"'":'&#39;'}[char])); }
function formatMoney(amount, currency) {
  const exponent = exponents[currency] ?? 2;
  const sign = amount < 0 ? '-' : '';
  const digits = String(Math.abs(Number(amount) || 0)).padStart(exponent + 1, '0');
  const whole = exponent ? digits.slice(0, -exponent) : digits;
  const fraction = exponent ? '.' + digits.slice(-exponent) : '';
  const symbol = {EUR:'€', USD:'$', JPY:'¥'}[currency];
  return sign + (symbol ? symbol + Number(whole).toLocaleString() + fraction : currency + ' ' + Number(whole).toLocaleString() + fraction);
}
function formatDate(value) { const date = new Date(value); return Number.isNaN(date.valueOf()) ? 'Unknown date' : date.toLocaleString(undefined, {day:'numeric', month:'short', year:'numeric', hour:'2-digit', minute:'2-digit'}); }
function errorMessage(error, fallback) { return error && error.message ? error.message : fallback; }

async function request(url, options = {}) {
  const controller = new AbortController();
  const timeout = setTimeout(() => controller.abort('timeout'), REQUEST_TIMEOUT);
  const signal = options.signal ? AbortSignal.any([options.signal, controller.signal]) : controller.signal;
  try {
    const response = await fetch(url, {...options, signal});
    let body = null;
    try { body = await response.json(); } catch (_) { /* A non-JSON proxy response is still an error envelope. */ }
    if (!response.ok) {
      const message = body?.error?.message || 'The server could not complete this request.';
      throw Object.assign(new Error(message), {status:response.status, body});
    }
    return body;
  } catch (error) {
    if (controller.signal.aborted && !options.signal?.aborted) throw Object.assign(new Error('The request timed out. Please try again.'), {code:'timeout'});
    if (error?.name === 'AbortError') throw error;
    if (error instanceof Error) throw error;
    throw new Error('The network connection failed. Please try again.');
  } finally { clearTimeout(timeout); }
}
function paymentQuery() {
  const query = new URLSearchParams({limit:String(page.limit), offset:String(page.offset), sort:page.sort});
  if (page.status) query.set('status', page.status);
  if (page.currency) query.set('currency', page.currency);
  return query;
}
function setPager(offset, count, total) {
  const from = total ? offset + 1 : 0;
  const to = Math.min(offset + count, total);
  $('#showing').textContent = 'Showing ' + from + '–' + to + ' of ' + total;
  $('#prev').disabled = offset === 0;
  $('#next').disabled = offset + page.limit >= total;
}
function renderRows(data) {
  if (!data.length) return;
  rows.innerHTML = data.map(row => '<tr data-id="' + escapeHtml(row.id) + '"><td>' + formatDate(row.created_at) + '</td><td>' + formatMoney(row.amount_minor, row.currency) + '</td><td><span class="badge ' + escapeHtml(row.status) + '">' + escapeHtml(row.status) + '</span></td><td>' + escapeHtml(row.counterparty_name) + '<small> · ' + escapeHtml(row.country) + '</small></td><td class="note-cell"><button type="button" class="note-display" aria-label="' + (row.note ? 'Edit note' : 'Add note') + '">' + (escapeHtml(row.note) || 'Add note') + '</button></td></tr>').join('');
  rows.querySelectorAll('.note-cell').forEach(cell => { cell.querySelector('.note-display').addEventListener('click', () => beginNoteEdit(cell)); });
}
async function loadRows() {
  const requestId = ++tableRequest;
  tableController?.abort();
  tableController = new AbortController();
  state.textContent = 'Loading payments…';
  rows.replaceChildren();
  try {
    const data = await request('/api/payments?' + paymentQuery(), {signal:tableController.signal});
    if (requestId !== tableRequest) return;
    page.total = Number(data.total) || 0;
    page.offset = Number(data.offset) || 0;
    state.replaceChildren();
    if (!Array.isArray(data.data) || data.data.length === 0) {
      state.textContent = 'No payments match these filters.';
      if (page.total === 0) {
        const action = document.createElement('button');
        action.type = 'button'; action.className = 'empty-action'; action.textContent = 'Sync now'; action.addEventListener('click', sync);
        state.append(' ', action);
      }
    } else renderRows(data.data);
    setPager(Number(data.offset) || 0, Array.isArray(data.data) ? data.data.length : 0, page.total);
  } catch (error) {
    if (requestId !== tableRequest || error?.name === 'AbortError') return;
    page.total = 0;
    state.textContent = errorMessage(error, 'Payments could not be loaded. Check the connection and try again.');
    setPager(0, 0, 0);
  }
}
function restoreNote(cell, old) {
  cell.innerHTML = '<button type="button" class="note-display" aria-label="' + (old ? 'Edit note' : 'Add note') + '">' + (escapeHtml(old) || 'Add note') + '</button>';
  cell.querySelector('.note-display').addEventListener('click', () => beginNoteEdit(cell));
}
function beginNoteEdit(cell) {
  if (cell.querySelector('input')) return;
  const display = cell.querySelector('.note-display');
  const old = display.textContent === 'Add note' ? '' : display.textContent;
  const row = cell.closest('tr');
  const helpId = 'note-help-' + String(row.dataset.id).replace(/[^A-Za-z0-9_-]/g, '');
  cell.innerHTML = '<div class="note-edit"><div><input maxlength="280" value="' + escapeHtml(old) + '" aria-label="Payment note" aria-describedby="' + helpId + '"><span id="' + helpId + '" class="note-error" aria-live="polite"></span></div><button type="button">Save</button><button type="button" class="cancel-note">Cancel</button></div>';
  const input = cell.querySelector('input');
  const save = cell.querySelector('button');
  const cancel = cell.querySelector('.cancel-note');
  const help = cell.querySelector('.note-error');
  const validate = () => {
    const value = input.value.trim();
    const valid = value.length >= 1 && value.length <= 280;
    input.setAttribute('aria-invalid', String(!valid));
    help.textContent = valid ? '' : 'Enter a note between 1 and 280 characters.';
    save.disabled = !valid;
    return valid ? value : null;
  };
  const cancelEdit = () => restoreNote(cell, old);
  const submit = async () => {
    const value = validate();
    if (value === null) { input.focus(); return; }
    save.disabled = true; cancel.disabled = true;
    row.dataset.state = 'saving';
    cell.innerHTML = '<span class="note-display" aria-live="polite">' + escapeHtml(value) + '</span>';
    try {
      const result = await request('/api/payments/' + encodeURIComponent(row.dataset.id) + '/note', {method:'POST', headers:{'Content-Type':'application/json'}, body:JSON.stringify({note:value})});
      const savedNote = result.note || value;
      cell.innerHTML = '<button type="button" class="note-display" aria-live="polite" aria-label="Edit note">' + escapeHtml(savedNote) + '</button>';
      cell.querySelector('.note-display').addEventListener('click', () => beginNoteEdit(cell));
      row.dataset.state = 'saved';
      clearTimeout(savedTimer); savedTimer = setTimeout(() => delete row.dataset.state, 1450);
    } catch (error) {
      delete row.dataset.state;
      restoreNote(cell, old);
      notice.textContent = error?.status === 409
        ? 'This note changed elsewhere; your edit was not saved.'
        : errorMessage(error, 'The note could not be saved. Please try again.');
    }
  };
  input.addEventListener('input', validate);
  input.addEventListener('keydown', event => { if (event.key === 'Enter') { event.preventDefault(); submit(); } if (event.key === 'Escape') cancelEdit(); });
  save.addEventListener('click', submit); cancel.addEventListener('click', cancelEdit);
  input.focus(); validate();
}
async function loadSummary() {
  try {
    const data = await request('/api/summary');
    const totals = Array.isArray(data.by_currency) ? data.by_currency : [];
    $('#summary').innerHTML = totals.map(item => '<div class="cur-total" data-currency="' + escapeHtml(item.currency) + '"><strong>' + formatMoney(item.total_minor, item.currency) + '</strong>' + item.count + ' payment' + (item.count === 1 ? '' : 's') + ' · ' + escapeHtml(item.currency) + '</div>').join('') + '<div id="last-sync">' + (data.last_sync ? 'Last sync ' + formatDate(data.last_sync) : 'Never synced') + '</div>';
  } catch (_) { $('#summary').innerHTML = '<div id="last-sync">Summary unavailable</div>'; }
}
async function loadBuckets() { try { window.VSViz.setLoading(); const data = await request('/api/buckets'); window.VSViz.setData(data); } catch (_) { window.VSViz.error(); } }
async function sync() {
  const button = $('#sync-now'); button.disabled = true; button.dataset.state = 'syncing'; button.textContent = 'Syncing…'; notice.textContent = '';
  try { await request('/api/sync', {method:'POST'}); await refresh(); }
  catch (error) { notice.textContent = errorMessage(error, 'Sync failed. Meridian may be temporarily unavailable.'); }
  finally { button.disabled = false; delete button.dataset.state; button.textContent = 'Sync now'; }
}
async function refresh() { await Promise.all([loadRows(), loadSummary(), loadBuckets()]); }
function bindSelect(root, field) {
  const trigger = root.querySelector(':scope > button');
  const choices = [...root.querySelectorAll('[role=option]')];
  const close = focus => { root.classList.remove('open'); trigger.setAttribute('aria-expanded', 'false'); if (focus) trigger.focus(); };
  const open = focusIndex => { root.classList.add('open'); trigger.setAttribute('aria-expanded', 'true'); if (Number.isInteger(focusIndex)) choices[focusIndex].focus(); };
  const choose = choice => { root.dataset.value = choice.dataset.value; trigger.textContent = choice.textContent; choices.forEach(item => item.setAttribute('aria-selected', String(item === choice))); close(true); page[field] = choice.dataset.value; page.offset = 0; loadRows(); };
  trigger.addEventListener('click', () => root.classList.contains('open') ? close(false) : open());
  trigger.addEventListener('keydown', event => { if (['ArrowDown','ArrowUp','Home','End'].includes(event.key)) { event.preventDefault(); const selected = Math.max(0, choices.findIndex(item => item.dataset.value === root.dataset.value)); open(event.key === 'ArrowUp' || event.key === 'End' ? choices.length - 1 : event.key === 'Home' ? 0 : selected); } if (event.key === 'Escape') close(false); });
  choices.forEach((choice, index) => { choice.addEventListener('click', () => choose(choice)); choice.addEventListener('keydown', event => { if (event.key === 'Escape') { event.preventDefault(); close(true); } else if (event.key === 'ArrowDown' || event.key === 'ArrowUp' || event.key === 'Home' || event.key === 'End') { event.preventDefault(); const next = event.key === 'Home' ? 0 : event.key === 'End' ? choices.length - 1 : (index + (event.key === 'ArrowDown' ? 1 : choices.length - 1)) % choices.length; choices[next].focus(); } else if (event.key === 'Tab') close(false); }); });
}
function bindSort(element, key) { const activate = () => { const active = page.sort.replace('-', '') === key; const descending = active && !page.sort.startsWith('-'); page.sort = (descending ? '-' : '') + key; page.offset = 0; $('#date-sort').setAttribute('aria-sort', key === 'created_at' ? (descending ? 'descending' : 'ascending') : 'none'); $('#amount-sort').setAttribute('aria-sort', key === 'amount_minor' ? (descending ? 'descending' : 'ascending') : 'none'); element.setAttribute('aria-label', 'Sort by ' + (key === 'created_at' ? 'date' : 'amount') + ', ' + (descending ? 'descending' : 'ascending')); loadRows(); }; element.addEventListener('click', activate); element.addEventListener('keydown', event => { if (event.key === 'Enter' || event.key === ' ') { event.preventDefault(); activate(); } }); }
bindSelect($('#status-filter'), 'status'); bindSelect($('#currency-filter'), 'currency');
document.addEventListener('click', event => document.querySelectorAll('.custom-select.open').forEach(root => { if (!root.contains(event.target)) { root.classList.remove('open'); root.querySelector(':scope > button').setAttribute('aria-expanded', 'false'); } }));
$('#prev').addEventListener('click', () => { page.offset = Math.max(0, page.offset - page.limit); loadRows(); });
$('#next').addEventListener('click', () => { if (page.offset + page.limit < page.total) { page.offset += page.limit; loadRows(); } });
bindSort($('#date-sort'), 'created_at'); bindSort($('#amount-sort'), 'amount_minor');
window.addEventListener('vspro-status', event => { const value = event.detail; const root = $('#status-filter'); const choice = [...root.querySelectorAll('[role=option]')].find(item => item.dataset.value === value); if (!choice) return; root.dataset.value = value; root.querySelector(':scope > button').textContent = choice.textContent; root.querySelectorAll('[role=option]').forEach(item => item.setAttribute('aria-selected', String(item === choice))); page.status = value; page.offset = 0; loadRows(); });
$('#sync-now').addEventListener('click', sync);
refresh();
})();