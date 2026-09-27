// livecheck.mjs [--title <session title>] [--shot <png>] — READ-ONLY: does every surface a user looks at agree
// with what is live right now? Q-147 (2026-09-26): a 27B split session wrote 17k tokens for 30 minutes while the
// sidebar listed it as "27m ago", styled exactly like the idle sessions under it (one of them with the same
// title). The Engine card said "Writing · Serving · Chat · <title>". Every critic round missed it: the critic's
// scope was the MLX surfaces, and it walked the app at REST — this is a defect that exists only while work is live.
//
// Truth: the engine's own request list (window.electron.mlxEngineActivity) and, when given, the title of the
// session the caller knows is running (r1.mjs passes its own). Each surface is read from the DOM as a user sees it.
// Never clicks, never navigates. Prints one JSON line; exit 0 = consistent, 3 = at least one contradiction.
// Also importable: `liveCheck(page, { title })` returns the same object (r1.mjs calls it every minute).
import { chromium } from '/Users/mihaiperdum/Projects/goose/ui/node_modules/playwright-core/index.mjs';
import { mainPage } from './mainpage.mjs';
import { fileURLToPath } from 'node:url';

// A session row "says live" if it carries a running word, a busy/live ARIA state, or a live data-state —
// whatever the implementation, a user must be able to see it. "N ago" on a running session is a contradiction.
const LIVE_WORDS = /\b(running|writing|thinking|generating|working|live|reading|running tools?)\b/i;

export async function liveCheck(p, { title = '' } = {}) {
  const act = await p.evaluate(async () => {
    try { return await window.electron.mlxEngineActivity(); } catch (e) { return { error: String(e) }; }
  });
  const engineBusy = (act?.stats?.numRunning ?? 0) > 0;
  const rows = await p.evaluate((liveSrc) => {
    const live = new RegExp(liveSrc, 'i');
    // Session rows carry data-testid="session-row-<id>" (Layout/ProjectsSection, tree.tsx) wherever they are listed.
    const cands = [...document.querySelectorAll('[data-testid^=session-row-]')]
      .filter((e) => e.offsetParent)
      .map((e) => {
        const text = e.innerText.replace(/\s+/g, ' ').trim();
        // Q-185: `background` = goose still working for the session after its reply (the fact check) —
        // a quieter mark than running, but a mark: the row must not read idle while the engine serves it.
        const busy = e.getAttribute('aria-busy') === 'true' || !!e.querySelector('[aria-busy=true]')
          || /running|live|busy|active-turn|background/i.test(e.getAttribute('data-state') ?? '')
          || !!e.querySelector('[data-state*=running],[data-state*=live],[data-state*=background],[data-running=true],[data-live=true]');
        return { id: e.dataset.testid.slice('session-row-'.length), text, busy, liveWord: live.test(text) };
      });
    // What the Engine card says it is serving (MlxStateTile's serving list), when that view is open.
    // A row goose tagged with its own work (Q-185) reads "<work> · <session>" and carries data-work.
    const serving = [...document.querySelectorAll('[data-testid=mlx-serving-row]')].map((e) => {
      const text = e.innerText.replace(/\s+/g, ' ').trim();
      return e.dataset.work ? text.replace(/^[^·]+ · /, 'Chat · ') : text;
    });
    return { cands, serving };
  }, LIVE_WORDS.source);
  const { cands, serving } = rows;
  return judge(act, engineBusy, cands, serving, title, p.url());
}

function judge(act, engineBusy, rows, serving, title, url) {
  const findings = [];
  // A probe that sees no rows is BLIND, never a pass and never a product finding.
  if (rows.length === 0) findings.push({ kind: 'PROBE_BLIND', surface: 'sidebar', says: 'no [data-testid^=session-row-] visible', truth: 'cannot judge' });
  const marked = rows.filter((r) => r.busy || r.liveWord);
  if (rows.length && engineBusy && marked.length === 0) {
    findings.push({ kind: 'LIVE_STATE_MISSING', surface: 'sidebar session list',
      says: `${rows.length} session rows, none marked running`, truth: `engine generating ${act.stats.numRunning} request(s)` });
  }
  // The serving row reads "Chat · <title>" and, since 3.0.57, a trailing "· N requests" / "N requests".
  const names = [...new Set([title, ...serving.map((t) => t.replace(/^Chat · /, '').replace(/\s*·?\s*\d+ requests?$/, ''))].filter(Boolean))];
  for (const name of names) {
    const mine = rows.filter((r) => r.text.includes(name));
    if (rows.length && mine.length === 0) findings.push({ kind: 'LIVE_SESSION_NOT_LISTED', surface: 'sidebar', says: 'no row', truth: name });
    if (mine.length && !mine.some((r) => r.busy || r.liveWord)) {
      findings.push({ kind: 'LIVE_STATE_MISSING', surface: 'sidebar row', says: mine.map((r) => r.text), truth: `${name} is running` });
    }
    const same = rows.filter((r) => r.text.replace(/\s+\S+ ago$|\s+now$/i, '').trim() === name);
    if (same.length > 1) findings.push({ kind: 'AMBIGUOUS_TITLE', surface: 'sidebar', says: same.map((r) => r.text), truth: `${same.length} rows read "${name}"` });
  }
  return { at: new Date().toISOString(), url: url.split('#')[1] ?? '', engineBusy, serving, rows: rows.length, marked: marked.length, findings };
}

if (process.argv[1] === fileURLToPath(import.meta.url)) {
  const arg = (k) => { const i = process.argv.indexOf(k); return i > 0 ? process.argv[i + 1] : ''; };
  const b = await chromium.connectOverCDP('http://127.0.0.1:9333');
  const p = await mainPage(b);
  const r = await liveCheck(p, { title: arg('--title') });
  if (arg('--shot')) await p.screenshot({ path: arg('--shot') });
  console.log(JSON.stringify(r));
  process.exit(r.findings.length ? 3 : 0);
}
