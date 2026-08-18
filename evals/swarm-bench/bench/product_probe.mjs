#!/usr/bin/env node
// product_probe.mjs — browser-truth probe for the swarm-bench scorer.
// Usage: node product_probe.mjs <load|sync|error|empty> <baseUrl>
// Prints EXACTLY ONE JSON object to stdout; all diagnostics go to stderr.
// Exit 0 always, including failed checks and the 90s hard cap (timedOut:true);
// nonzero exit only when the probe itself crashes.

import { createRequire } from 'module';
import { execSync } from 'child_process';
import { join, dirname } from 'path';
import { mkdirSync } from 'fs';

const err = (...a) => console.error('[probe]', ...a);

function loadPlaywright() {
  const attempts = [];
  try {
    return createRequire(import.meta.url)('playwright');
  } catch (e) {
    attempts.push('local: ' + e.message);
  }
  try {
    const g = execSync('npm root -g', { encoding: 'utf8' }).trim();
    return createRequire(join(g, '__probe__.js'))('playwright');
  } catch (e) {
    attempts.push('npm-root-g: ' + e.message);
  }
  try {
    const g = join(dirname(process.execPath), '..', 'lib', 'node_modules');
    return createRequire(join(g, '__probe__.js'))('playwright');
  } catch (e) {
    attempts.push('execPath: ' + e.message);
  }
  throw new Error('cannot resolve playwright: ' + attempts.join(' | '));
}

const args = process.argv.slice(2);
// Optional: --block-api makes the error scenario usable when the caller cannot kill the
// backend — the document loads, its data fetches are refused, and the app's own error UI
// is what gets measured. Without it, a dead backend simply fails the navigation.
const blockApi = args.includes('--block-api');
const positional = args.filter((a) => !a.startsWith('--'));
const scenario = positional[0];
const baseUrl = positional[1];
if (!['load', 'sync', 'error', 'empty'].includes(scenario) || !baseUrl) {
  err('usage: node product_probe.mjs <load|sync|error|empty> <baseUrl> [--block-api]');
  process.exit(2);
}

const HARD_MS = 90000;
const startedAt = Date.now();
const budgetLeft = () => HARD_MS - (Date.now() - startedAt);
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));

const result = { scenario, baseUrl, timedOut: false };
let printed = false;
let browser = null;

function emit(extra, cb) {
  if (printed) return;
  printed = true;
  process.stdout.write(JSON.stringify({ ...result, ...extra }) + '\n', cb || (() => {}));
}

const hardTimer = setTimeout(() => {
  err('hard 90s cap hit — emitting partial result');
  emit({ timedOut: true }, async () => {
    try {
      await Promise.race([browser && browser.close(), sleep(1500)]);
    } catch {}
    process.exit(0);
  });
}, HARD_MS);

// ---------- page-side functions (self-contained; serialized by playwright) ----------

// Installed before any page script: stamps the exact moment the first data row lands,
// so the reported time is the app's render time, not the probe's polling latency.
function initFirstDataStamp() {
  window.__probeFirstDataMs = null;
  const check = () => {
    if (window.__probeFirstDataMs != null) return true;
    let rows = Array.from(document.querySelectorAll('tbody tr')).filter(
      (r) => r.querySelectorAll('td,th').length >= 2
    );
    if (rows.length === 0) {
      rows = Array.from(document.querySelectorAll('[role="row"]')).filter(
        (r) =>
          r.querySelectorAll('[role="cell"],[role="gridcell"]').length >= 2 &&
          !r.querySelector('[role="columnheader"]')
      );
    }
    if (rows.some((r) => (r.textContent || '').trim().length > 0)) {
      window.__probeFirstDataMs = performance.now();
      return true;
    }
    return false;
  };
  const start = () => {
    if (check()) return;
    const mo = new MutationObserver(() => {
      if (check()) mo.disconnect();
    });
    mo.observe(document.documentElement, { childList: true, subtree: true, characterData: true });
  };
  if (document.documentElement) start();
  else document.addEventListener('readystatechange', start, { once: true });
}

function pageFirstDataMs() {
  if (window.__probeFirstDataMs != null) return window.__probeFirstDataMs;
  // Rendered-means-seen here too: innerText of a display:none row falls back to textContent in
  // Chromium, so without the rects check this timer fires on rows the user never saw.
  const visible = (el) =>
    !!(el.getClientRects && el.getClientRects().length) &&
    getComputedStyle(el).visibility !== 'hidden';
  let rows = Array.from(document.querySelectorAll('tbody tr')).filter(
    (r) => r.querySelectorAll('td,th').length >= 2 && visible(r)
  );
  if (rows.length === 0) {
    rows = Array.from(document.querySelectorAll('[role="row"]')).filter(
      (r) =>
        r.querySelectorAll('[role="cell"],[role="gridcell"]').length >= 2 &&
        !r.querySelector('[role="columnheader"]') &&
        visible(r)
    );
  }
  rows = rows.filter((r) => (r.innerText || '').trim().length > 0);
  return rows.length > 0 ? performance.now() : null;
}

function pageAnalyzeLoad() {
  const visible = (el) =>
    !!(el.getClientRects && el.getClientRects().length) &&
    getComputedStyle(el).visibility !== 'hidden';

  function dataRows() {
    let rows = Array.from(document.querySelectorAll('tbody tr')).filter(
      (r) => r.querySelectorAll('td,th').length >= 2
    );
    if (rows.length === 0) {
      rows = Array.from(document.querySelectorAll('[role="row"]')).filter(
        (r) =>
          r.querySelectorAll('[role="cell"],[role="gridcell"]').length >= 2 &&
          !r.querySelector('[role="columnheader"]')
      );
    }
    return rows;
  }
  // RENDERED means SEEN. Run 9 shipped a page that loads all 247 payments, appends its rows,
  // THROWS in the same render pass (an undefined identifier), and paints "Backend unreachable"
  // over a display:none table — and this counter reported the hidden rows as rendered, scoring a
  // page no human could read as if it worked. A row counts only if the browser would paint it;
  // the raw DOM count stays as a diagnostic so the gap itself is visible in the probe output.
  const domRows = dataRows();
  const rows = domRows.filter(visible);
  const cellsOf = (r) => Array.from(r.querySelectorAll('td,th,[role="cell"],[role="gridcell"]'));
  const renderedRowCount = rows.length;
  const domRowCount = domRows.length;

  const dateRe =
    /(\d{4}-\d{2}-\d{2})|(\d{1,2}[/.\-]\d{1,2}[/.\-]\d{2,4})|\b(jan|feb|mar|apr|may|jun|jul|aug|sep|oct|nov|dec)[a-z]*\.?\s+\d{1,2}\b|\b\d{1,2}\s+(jan|feb|mar|apr|may|jun|jul|aug|sep|oct|nov|dec)[a-z]*\b/i;
  let dateCol = 0;
  if (rows.length) {
    const idx = cellsOf(rows[0]).findIndex((c) => dateRe.test(c.innerText || ''));
    if (idx >= 0) dateCol = idx;
  }
  const dateTexts = rows
    .slice(0, 3)
    .map((r) => ((cellsOf(r)[dateCol] || {}).innerText || '').trim())
    .filter(Boolean);

  const statusWordRe =
    /^(pending|processing|in.progress|complete(d)?|success(ful)?|paid|unpaid|failed|failure|error|synced|syncing|active|inactive|overdue|refunded|cancel(l)?ed|declined|posted|settled|new|open|closed)$/i;
  let statusIdx = -1;
  const table = rows.length ? rows[0].closest('table') : null;
  if (table) {
    let headerCells = Array.from(table.querySelectorAll('thead th, thead td'));
    if (headerCells.length === 0) {
      const firstTr = table.querySelector('tr');
      if (firstTr && rows.indexOf(firstTr) === -1) headerCells = Array.from(firstTr.children);
    }
    statusIdx = headerCells.findIndex((h) => /status|state/i.test(h.innerText || ''));
  }
  const statusStyles = {};
  for (const r of rows) {
    const cells = cellsOf(r);
    const cell =
      statusIdx >= 0
        ? cells[statusIdx]
        : cells.find((c) => statusWordRe.test((c.innerText || '').trim()));
    if (!cell) continue;
    const label = (cell.innerText || '').trim();
    if (!label || label.length > 40 || statusStyles[label]) continue;
    let carrier = cell;
    let descended = true;
    while (descended) {
      descended = false;
      for (const ch of carrier.children) {
        if ((ch.innerText || '').trim() === label) {
          carrier = ch;
          descended = true;
          break;
        }
      }
    }
    const cs = getComputedStyle(carrier);
    let bg = cs.backgroundColor;
    let node = carrier;
    while (node && node !== r.parentElement && (bg === 'rgba(0, 0, 0, 0)' || bg === 'transparent')) {
      node = node.parentElement;
      if (node) bg = getComputedStyle(node).backgroundColor;
    }
    statusStyles[label] = { color: cs.color, backgroundColor: bg };
  }

  let totalClaimedInDom = null;
  {
    // Counts live in split markup ("<div>Payments: <strong>247</strong></div>"), so scan
    // element innerText, not raw text nodes. Strip money/dates first: a currency total
    // would otherwise swamp the row count under a largest-integer rule.
    const scrub = (s) =>
      s
        .replace(/\d{4}-\d{2}-\d{2}[T\s0-9:.+Z-]*/g, ' ')
        .replace(/[€$£¥₹]\s?[\d,]+(?:\.\d+)?/g, ' ')
        .replace(/[\d,]+(?:\.\d+)?\s?(?:EUR|USD|GBP|CHF|JPY)\b/gi, ' ')
        .replace(/\b\d+(?:\.\d+)+\b/g, ' ');
    const kw =
      '(?:payments?|records?|results?|rows|items|entries|transactions|invoices?|count|total)';
    const pats = [
      /\bof\s+([\d,]+)\b/gi,
      new RegExp('\\b([\\d,]+)\\s+' + kw + '\\b', 'gi'),
      new RegExp('\\b' + kw + '\\b[^0-9a-z]{0,12}([\\d,]+)\\b', 'gi'),
    ];
    const take = (raw) => {
      const t = scrub(raw);
      for (const re of pats) {
        re.lastIndex = 0;
        let m;
        while ((m = re.exec(t))) {
          const v = parseInt(m[1].replace(/,/g, ''), 10);
          if (Number.isFinite(v) && (totalClaimedInDom === null || v > totalClaimedInDom))
            totalClaimedInDom = v;
        }
      }
    };
    const els = Array.from(document.querySelectorAll('body *')).filter((el) => {
      if (el.closest('script,style,td,th,[role="cell"],[role="gridcell"]')) return false;
      if (el.querySelector('table,tbody')) return false;
      if (!visible(el)) return false;
      const t = el.innerText || '';
      return t.length > 0 && t.length <= 250 && /\d/.test(t);
    });
    for (const el of els) take(el.innerText || '');
  }

  const paginationControls = (() => {
    const shortCtl = /^(prev(ious)?|next|first|last|page\s*\d+|[«»‹›]|[<>]{1,2})$/i;
    const ctl = Array.from(document.querySelectorAll('button, a, [role="button"]')).some((el) => {
      if (!visible(el)) return false;
      const own = (el.innerText || '').trim();
      const aria = (el.getAttribute('aria-label') || '').trim();
      return shortCtl.test(own) || /^(prev(ious)?|next)( page)?$/i.test(aria);
    });
    if (ctl) return true;
    if (document.querySelector('nav[aria-label*="pag" i], [class*="pagin" i]')) return true;
    return /showing\s+[\d,]+(\s*(?:[-–—]|to)\s*[\d,]+)?\s+of\s+[\d,]+/i.test(document.body.innerText || '');
  })();

  const filterControl = (() => {
    const re = /filter|status/i;
    const els = Array.from(
      document.querySelectorAll('select, input, button, [role="combobox"], [role="listbox"], [role="radiogroup"]')
    );
    for (const el of els) {
      if (!visible(el)) continue;
      let name =
        (el.getAttribute('aria-label') || '') +
        ' ' +
        (el.getAttribute('placeholder') || '') +
        ' ' +
        (el.name || '') +
        ' ' +
        (el.id || '') +
        ' ' +
        (el.title || '') +
        ' ' +
        (el.className && el.className.baseVal === undefined ? el.className : '');
      if (el.tagName === 'BUTTON') name += ' ' + (el.innerText || '');
      if (el.labels) for (const l of el.labels) name += ' ' + (l.innerText || '');
      const wrap = el.closest('label');
      if (wrap) name += ' ' + (wrap.innerText || '');
      const parent = el.parentElement;
      // Buttons match only on their OWN text/label/class. The parent-text heuristic let ANY
      // visible button near the words "filter"/"status" count (measured: the Sync button next
      // to a "Filter by status" label passed v_filter while the actual controls were
      // display:none) — a hole a real app could ride to a vacuous pass.
      if (el.tagName !== 'BUTTON' && parent && (parent.innerText || '').length < 60)
        name += ' ' + parent.innerText;
      if (re.test(name)) return true;
      if (
        el.tagName === 'SELECT' &&
        Array.from(el.options).filter((o) =>
          /^(all|pending|processing|complete(d)?|paid|failed|synced|active|refunded|cancel(l)?ed)$/i.test(
            (o.text || '').trim()
          )
        ).length >= 2
      )
        return true;
    }
    // The class-name fallback must see a VISIBLE element — an invisible .filter-group is
    // exactly what the drop_filter control injects, and what a broken app would ship.
    return Array.from(document.querySelectorAll('[class*="filter" i]')).some(visible);
  })();

  const nav = performance.getEntriesByType('navigation')[0];
  const pageWeightBytes = Math.round(
    ((nav && nav.transferSize) || 0) +
      performance.getEntriesByType('resource').reduce((s, e) => s + (e.transferSize || 0), 0)
  );

  const styling = (() => {
    const hasStylesheet =
      Array.from(document.querySelectorAll('style')).some((s) => (s.textContent || '').trim().length > 0) ||
      Array.from(document.querySelectorAll('link[rel~="stylesheet" i]')).some((l) => l.href);
    const bodyFontFamily = getComputedStyle(document.body).fontFamily;
    const els = document.querySelectorAll(
      'header, thead, th, table, button, [class*="summary" i], [class*="card" i], [class*="header" i]'
    );
    const bgs = new Set();
    for (const el of els) {
      const bg = getComputedStyle(el).backgroundColor;
      if (bg && bg !== 'rgba(0, 0, 0, 0)' && bg !== 'transparent') bgs.add(bg);
    }
    return { hasStylesheet, bodyFontFamily, distinctBackgroundCount: bgs.size };
  })();

  return {
    renderedRowCount,
    domRowCount,
    totalClaimedInDom,
    dateTexts,
    statusStyles,
    paginationControls,
    filterControl,
    pageWeightBytes,
    styling,
  };
}

function pageHorizontalScroll() {
  const sw = Math.max(
    document.documentElement ? document.documentElement.scrollWidth : 0,
    document.body ? document.body.scrollWidth : 0
  );
  return sw > window.innerWidth + 1;
}

function pageSyncState() {
  const cands = Array.from(
    document.querySelectorAll('button, input[type="button"], input[type="submit"], [role="button"], a')
  );
  const acc = (el) =>
    (el.innerText || el.value || '') +
    ' ' +
    (el.getAttribute('aria-label') || '') +
    ' ' +
    (el.title || '');
  const el = cands.find((c) => /sync/i.test(acc(c)) && c.getClientRects().length > 0);
  if (!el) return { found: false };
  return {
    found: true,
    text: (el.innerText || el.value || el.getAttribute('aria-label') || '').trim().slice(0, 60),
    disabled:
      el.disabled === true ||
      el.hasAttribute('disabled') ||
      el.getAttribute('aria-disabled') === 'true' ||
      el.getAttribute('aria-busy') === 'true',
  };
}

function pageClickSync() {
  const cands = Array.from(
    document.querySelectorAll('button, input[type="button"], input[type="submit"], [role="button"], a')
  );
  const acc = (el) =>
    (el.innerText || el.value || '') +
    ' ' +
    (el.getAttribute('aria-label') || '') +
    ' ' +
    (el.title || '');
  const el = cands.find((c) => /sync/i.test(acc(c)) && c.getClientRects().length > 0);
  if (!el) return false;
  el.scrollIntoView({ block: 'center' });
  el.click();
  return true;
}

function pageViewSnapshot() {
  // Same rendered-means-seen rule as pageAnalyzeLoad: rows behind display:none do not exist to
  // the user, so they do not exist to the sync/pagination scenarios either.
  const visible = (el) =>
    !!(el.getClientRects && el.getClientRects().length) &&
    getComputedStyle(el).visibility !== 'hidden';
  let rows = Array.from(document.querySelectorAll('tbody tr')).filter(
    (r) => r.querySelectorAll('td,th').length >= 2 && visible(r)
  );
  if (rows.length === 0) {
    rows = Array.from(document.querySelectorAll('[role="row"]')).filter(
      (r) =>
        r.querySelectorAll('[role="cell"],[role="gridcell"]').length >= 2 &&
        !r.querySelector('[role="columnheader"]') &&
        visible(r)
    );
  }
  let lastSyncText = null;
  const re = /last\s*sync|synced|updated|refreshed|as of/i;
  const walker = document.createTreeWalker(document.body, NodeFilter.SHOW_TEXT);
  let n;
  while ((n = walker.nextNode())) {
    const t = (n.textContent || '').trim();
    if (!t || t.length > 150 || !re.test(t)) continue;
    const p = n.parentElement;
    if (!p || p.closest('script,style,td,th,button,[role="cell"],[role="gridcell"]')) continue;
    if (!(p.getClientRects && p.getClientRects().length)) continue;
    lastSyncText = t;
    break;
  }
  let hash = 0;
  const text = rows.map((r) => r.innerText || '').join('|');
  for (let i = 0; i < text.length; i++) hash = (hash * 31 + text.charCodeAt(i)) | 0;
  return { rowCount: rows.length, lastSyncText, tableHash: hash };
}

function pageErrorBanner() {
  const re = /error|unable|unreachable|failed|try again|retry/i;
  const els = Array.from(document.querySelectorAll('body *'));
  for (const el of els) {
    if (el.children.length > 0 && !el.matches('[role="alert"],[class*="error" i],[class*="alert" i],[class*="banner" i]'))
      continue;
    if (el.closest('td,th,[role="cell"],[role="gridcell"],script,style')) continue;
    if (!(el.getClientRects && el.getClientRects().length)) continue;
    const t = (el.innerText || '').trim();
    if (t && t.length < 300 && re.test(t)) return t.slice(0, 120);
  }
  return null;
}

function pageEmptyState() {
  const re = /no\s+payments|nothing|empty|no\s+(records|results|data|items|transactions)/i;
  const els = Array.from(document.querySelectorAll('body *'));
  for (const el of els) {
    if (el.children.length > 0) continue;
    if (el.closest('script,style')) continue;
    if (!(el.getClientRects && el.getClientRects().length)) continue;
    const t = (el.innerText || '').trim();
    if (t && t.length < 200 && re.test(t)) return t.slice(0, 120);
  }
  return null;
}

function pageBlankAndBody() {
  const t = document.body ? (document.body.innerText || '').trim() : '';
  return { blankPage: t.length < 20, bodyTextLength: t.length };
}

// ---------- scenarios ----------

async function main() {
  const playwright = loadPlaywright();
  browser = await playwright.chromium.launch({ headless: true });
  const context = await browser.newContext({ viewport: { width: 1280, height: 800 } });
  await context.addInitScript(initFirstDataStamp);
  if (blockApi) {
    await context.route('**/*', (route) => {
      const url = route.request().url();
      if (route.request().resourceType() === 'document') return route.continue();
      if (/\/(api|data|graphql)(\/|\?|$)/i.test(url) || /\.json(\?|$)/i.test(url))
        return route.abort('connectionrefused');
      return route.continue();
    });
  }
  const page = await context.newPage();

  // QUALITY SCREENSHOTS (product contract 2026-08-17): when BENCH_SHOTS_DIR is set, each
  // scenario leaves a PNG named <epoch>-<name>.png. The probe runs during the engine's
  // repair/verify rounds, so successive epochs show the page AS THE SWARM REPAIRS IT; the
  // publisher picks first/last epochs for the before/after story. Never fatal: a failed
  // screenshot logs to stderr and the probe's JSON verdict is unaffected.
  const shotsDir = process.env.BENCH_SHOTS_DIR || '';
  const shotEpoch = Math.floor(Date.now() / 1000);
  async function saveShot(name) {
    if (!shotsDir) return;
    try {
      mkdirSync(shotsDir, { recursive: true });
      await page.screenshot({ path: join(shotsDir, `${shotEpoch}-${name}.png`), timeout: 5000 });
    } catch (e) {
      err('screenshot failed:', String((e && e.message) || e).slice(0, 200));
    }
  }
  async function saveShotMobile() {
    if (!shotsDir) return;
    try {
      await page.setViewportSize({ width: 375, height: 800 });
      await sleep(400);
      await page.screenshot({
        path: join(shotsDir, `${shotEpoch}-mobile.png`),
        timeout: 5000,
      });
      await page.setViewportSize({ width: 1280, height: 800 });
    } catch (e) {
      err('mobile screenshot failed:', String((e && e.message) || e).slice(0, 200));
    }
  }

  const consoleErrorTexts = [];
  page.on('console', (m) => {
    if (m.type() === 'error') consoleErrorTexts.push(m.text());
  });
  page.on('pageerror', (e) => consoleErrorTexts.push(String(e)));
  const consoleErrors = () => ({
    count: consoleErrorTexts.length,
    texts: consoleErrorTexts.slice(0, 3).map((t) => String(t).slice(0, 300)),
  });

  const clean = (s) =>
    String(s || '')
      // eslint-disable-next-line no-control-regex
      .replace(/\[[0-9;]*m/g, '')
      .split('\n')[0]
      .trim()
      .slice(0, 200);

  async function safeGoto(timeoutMs) {
    try {
      await page.goto(baseUrl, { waitUntil: 'domcontentloaded', timeout: timeoutMs });
      return null;
    } catch (e) {
      const msg = clean(e.message || e);
      err('goto failed:', msg);
      return msg;
    }
  }

  // A failed navigation can destroy the execution context mid-evaluate; one retry after a
  // settle keeps that race from being reported as a missing error state.
  async function evalRetry(fn, fallback) {
    for (let i = 0; i < 2; i++) {
      try {
        return await page.evaluate(fn);
      } catch (e) {
        err('evaluate failed (attempt ' + (i + 1) + '):', clean(e.message || e));
        await sleep(300);
      }
    }
    return fallback;
  }

  async function waitIdle(capMs) {
    try {
      await page.waitForLoadState('networkidle', { timeout: capMs });
    } catch {
      err('networkidle not reached within', capMs, 'ms (continuing)');
    }
  }

  async function pollFirstData(capMs) {
    const deadline = Date.now() + Math.min(capMs, Math.max(budgetLeft() - 20000, 1000));
    while (Date.now() < deadline) {
      const v = await page.evaluate(pageFirstDataMs).catch(() => null);
      if (v != null) return Math.round(v);
      await sleep(100);
    }
    return null;
  }

  if (scenario === 'load') {
    const navigationError = await safeGoto(20000);
    if (navigationError) {
      emit({ navigationError, consoleErrors: consoleErrors() });
      return;
    }
    const timeToFirstDataMs = await pollFirstData(15000);
    await waitIdle(10000);
    const analysis = await page.evaluate(pageAnalyzeLoad).catch((e) => {
      err('analysis evaluate failed:', e.message);
      return {};
    });
    let horizontalScroll = null;
    try {
      await page.setViewportSize({ width: 375, height: 812 });
      await page.reload({ waitUntil: 'domcontentloaded', timeout: 15000 });
      await waitIdle(5000);
      horizontalScroll = await page.evaluate(pageHorizontalScroll);
    } catch (e) {
      err('viewport375 check failed:', e.message.split('\n')[0]);
    }
    await saveShot('loaded');
    await saveShotMobile();
    emit({
      consoleErrors: consoleErrors(),
      timeToFirstDataMs,
      ...analysis,
      viewport375: { horizontalScroll },
    });
  } else if (scenario === 'sync') {
    const navigationError = await safeGoto(20000);
    if (navigationError) {
      emit({ navigationError, found: false, consoleErrors: consoleErrors() });
      return;
    }
    await waitIdle(10000);
    await pollFirstData(5000);
    const before = await page.evaluate(pageViewSnapshot).catch(() => null);
    const state = await page.evaluate(pageSyncState).catch(() => ({ found: false }));
    if (!state.found) {
      emit({
        found: false,
        disabledDuringSync: null,
        completedWithinMs: null,
        viewRefreshed: null,
        consoleErrors: consoleErrors(),
      });
      return;
    }
    err('sync button found:', JSON.stringify(state.text));
    const clicked = await page.evaluate(pageClickSync).catch(() => false);
    const clickAt = Date.now();
    if (!clicked) {
      emit({
        found: true,
        buttonText: state.text,
        clicked: false,
        disabledDuringSync: null,
        completedWithinMs: null,
        viewRefreshed: null,
        consoleErrors: consoleErrors(),
      });
      return;
    }

    let disabledDuringSync = false;
    while (Date.now() - clickAt < 1200) {
      const s = await page.evaluate(pageSyncState).catch(() => null);
      if (s && (!s.found || s.disabled)) {
        disabledDuringSync = true;
        break;
      }
      await sleep(50);
    }

    let completed = false;
    let completedWithinMs = null;
    let failedAfterMs = null;
    let errorBanner = null;
    let everDisabled = disabledDuringSync;
    let buttonPresentAfter = true;
    const capMs = Math.min(70000, Math.max(budgetLeft() - 8000, 2000));
    while (Date.now() - clickAt < capMs) {
      const s = await page.evaluate(pageSyncState).catch(() => null);
      errorBanner = await page.evaluate(pageErrorBanner).catch(() => null);
      if (s && s.found && s.disabled) everDisabled = true;
      buttonPresentAfter = !!(s && s.found);
      const enabled = s && s.found && !s.disabled;
      const elapsed = Date.now() - clickAt;
      if (enabled && !errorBanner && (everDisabled || elapsed > 1500)) {
        completed = true;
        completedWithinMs = elapsed;
        break;
      }
      // Terminal failure: the app surfaced an error and is no longer working. Some apps
      // re-enable the button, others replace the whole view (button included) with the
      // error UI — without this second case the loop would spin to the full cap.
      if (errorBanner && everDisabled && (enabled || !buttonPresentAfter)) {
        failedAfterMs = elapsed;
        break;
      }
      await sleep(250);
    }

    let after = await page.evaluate(pageViewSnapshot).catch(() => null);
    let viewRefreshed =
      !!(before && after) &&
      (after.rowCount !== before.rowCount || (after.lastSyncText || '') !== (before.lastSyncText || ''));
    if (!viewRefreshed && before && budgetLeft() > 5000) {
      await sleep(1500);
      after = await page.evaluate(pageViewSnapshot).catch(() => after);
      viewRefreshed =
        !!(before && after) &&
        (after.rowCount !== before.rowCount || (after.lastSyncText || '') !== (before.lastSyncText || ''));
    }
    const tableHashChanged = !!(before && after) && after.tableHash !== before.tableHash;

    await saveShot('synced');
    emit({
      found: true,
      buttonText: state.text,
      disabledDuringSync,
      completed,
      completedWithinMs,
      failedAfterMs,
      buttonPresentAfter,
      errorBanner,
      viewRefreshed,
      tableHashChanged,
      rowCountBefore: before ? before.rowCount : null,
      rowCountAfter: after ? after.rowCount : null,
      lastSyncTextBefore: before ? before.lastSyncText : null,
      lastSyncTextAfter: after ? after.lastSyncText : null,
      consoleErrors: consoleErrors(),
    });
  } else if (scenario === 'error') {
    const navigationError = await safeGoto(15000);
    if (!navigationError) {
      await waitIdle(5000);
      await sleep(1000);
    }
    const banner = await evalRetry(pageErrorBanner, null);
    const blank = await evalRetry(pageBlankAndBody, { blankPage: true, bodyTextLength: 0 });
    await saveShot('error');
    emit({
      navigationError,
      errorStateVisible: banner != null,
      actionableText: banner,
      blankPage: blank.blankPage,
      bodyTextLength: blank.bodyTextLength,
      consoleErrors: consoleErrors(),
    });
  } else if (scenario === 'empty') {
    const navigationError = await safeGoto(20000);
    if (navigationError) {
      emit({ navigationError, emptyStateVisible: false, renderedRowCount: 0, consoleErrors: consoleErrors() });
      return;
    }
    await waitIdle(10000);
    let emptyText = null;
    let rowCount = 0;
    const deadline = Date.now() + Math.min(8000, Math.max(budgetLeft() - 10000, 1000));
    while (Date.now() < deadline) {
      emptyText = await page.evaluate(pageEmptyState).catch(() => null);
      const snap = await page.evaluate(pageViewSnapshot).catch(() => null);
      rowCount = snap ? snap.rowCount : 0;
      if (emptyText != null || rowCount > 0) break;
      await sleep(250);
    }
    await saveShot('empty');
    emit({
      emptyStateVisible: emptyText != null,
      emptyStateText: emptyText,
      renderedRowCount: rowCount,
      consoleErrors: consoleErrors(),
    });
  }
}

main()
  .then(async () => {
    clearTimeout(hardTimer);
    if (!printed) emit({ probeIncomplete: true });
    try {
      await Promise.race([browser && browser.close(), sleep(3000)]);
    } catch {}
  })
  .catch(async (e) => {
    clearTimeout(hardTimer);
    err('PROBE CRASH:', e && e.stack ? e.stack : e);
    try {
      await Promise.race([browser && browser.close(), sleep(1500)]);
    } catch {}
    process.exit(1);
  });
