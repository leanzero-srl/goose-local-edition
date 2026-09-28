// split-start.mjs [--model <text>] — after EVERY install: start the split through Run it, as a user does, and
// prove it serves a completion. Q-113: 3.0.44 shipped a split that died 2 s after every launch, and nothing
// noticed until an E2E round tried to use it. Exit 0 = split up and answered; 1 = it failed (the goosed log's
// distributed WARN is printed); 2 = no split Run button on screen.
import { chromium } from '/Users/mihaiperdum/Projects/goose/ui/node_modules/playwright-core/index.mjs';
import { mainPage } from './mainpage.mjs';
import { readdirSync, readFileSync } from 'node:fs';
const b = await chromium.connectOverCDP('http://127.0.0.1:9333');
const p = await mainPage(b);
p.setDefaultTimeout(15000);
await p.goto(p.url().split('#')[0] + '#/leanzero-swarm?tab=mlx&mlx=engine'); await p.waitForTimeout(5000);
await p.getByText('LeanZero MLX', { exact: true }).first().click().catch(() => {}); await p.waitForTimeout(4000);
// --model <text>: pick that model in the Engine picker first (the picker is how a user chooses what Run starts).
const mi = process.argv.indexOf('--model');
if (mi > 0) {
  await p.locator('button:visible').filter({ hasText: /GB/ }).filter({ hasText: /\// }).first().click(); await p.waitForTimeout(1200);
  await p.locator('[role=option],[role=menuitem]').filter({ hasText: process.argv[mi + 1] }).first().click(); await p.waitForTimeout(3000);
  console.log('picked', process.argv[mi + 1]);
}
const owners = await p.evaluate(() => [...document.querySelectorAll('button')].filter((b) => b.innerText.trim() === 'Run' && b.offsetParent).map((b) => {
  let n = b;
  for (let k = 0; k < 8 && n; k++) { n = n.parentElement; const h = n?.innerText.match(/Run on this Mac|Run on Work.s Mac Studio|Run across both Macs/g) || []; if (h.length === 1) return h[0]; if (h.length > 1) return '?'; }
  return 'none';
}));
const status = () => p.evaluate(async () => { const s = await window.electron.mlxEngineActivity(); return `${s.engine}/${s.mode} ${s.statusDetail ?? ''}`; });
// The split must serve the model that was picked: on 3.0.52 the relaunch restored the previous Flash split,
// and the running-split shortcut passed a "27B" smoke against Flash (2026-09-26).
const want = mi > 0 ? process.argv[mi + 1].toLowerCase() : '';
const served = async () => {
  const m = await fetch('http://127.0.0.1:8091/v1/models').then((x) => x.json()).catch(() => ({ data: [] }));
  return (m.data ?? []).flatMap((d) => [d.id, ...(d.aliases ?? [])]).filter(Boolean);
};
const servesWanted = async () => !want || (await served()).some((n) => n.toLowerCase().includes(want));
const complete = async (secs) => {
  const names = await served();
  if (!(await servesWanted())) { console.log(`${secs}s split serves ${JSON.stringify(names)}, not "${want}"`); process.exit(1); }
  const r = await fetch('http://127.0.0.1:8091/v1/chat/completions', { method: 'POST', headers: { 'content-type': 'application/json' }, body: JSON.stringify({ messages: [{ role: 'user', content: 'Say OK.' }], max_tokens: 8 }) }).then((x) => x.json()).catch((e) => ({ error: String(e) }));
  console.log(`${secs}s split up serving ${names[0]}; completion:`, JSON.stringify(r.choices?.[0]?.message ?? r).slice(0, 200));
  process.exit(r.choices ? 0 : 1);
};
// A restore after install may already have brought the split back: then there is no Run to press.
if (/distributed\/running/.test(await status()) && (await servesWanted())) await complete(0);
// 3.0.61 install (2026-09-27): the restore was still STARTING when this looked — no Run button, not yet running —
// and it exited 2 on a split that answered 20 s later. A restore in flight is waited for, not reported missing.
// 3.0.68 (2026-09-28): the restore passed through a state this list lacked and the script exited 2 on a split
// that was Ready seconds later — every in-flight state is waited for, not only three.
for (let k = 0; /^distributed\/(mounting|unknown|reconnecting|starting|loading|warming|restoring|recovering)/.test(await status()); k++) {
  if (k % 6 === 0) console.log(`restore in flight: ${await status()}`);
  await p.waitForTimeout(5000);
  if (/distributed\/running/.test(await status()) && (await servesWanted())) await complete('restored');
}
// --place studio runs the model on the Studio alone (remote single over Link) — used when this Mac cannot hold its
// rank (2026-09-28: 12 GiB free under other sessions' load; the split's restore refused, loud and correct).
const place = process.argv.includes('--place') ? process.argv[process.argv.indexOf('--place') + 1] : 'split';
const wantOwner = place === 'studio' ? 'Run on Work’s Mac Studio' : 'Run across both Macs';
const i = owners.findIndex((o) => o.replace(/[’']/g, "'") === wantOwner.replace(/[’']/g, "'"));
if (i < 0) { console.log(`no ${place} Run button`, JSON.stringify(owners)); process.exit(2); }
const t0 = Date.now();
await p.locator('button:visible', { hasText: /^Run$/ }).nth(i).click();
// A switch may ask first (stopping the way it replaces). Record the words, then confirm the switch — a script
// that leaves a dialog open leaves it on the owner's screen (2026-09-26).
await p.waitForTimeout(1500);
const dlg = p.getByRole('dialog');
if (await dlg.count()) {
  console.log('dialog:', (await dlg.first().innerText()).replace(/\s+/g, ' ').slice(0, 300));
  const btns = dlg.first().getByRole('button');
  const names = await btns.allInnerTexts();
  const confirm = names.findIndex((n) => !/cancel|keep|close|not now|^$/i.test(n.trim()));
  if (confirm >= 0) { console.log('confirming:', names[confirm]); await btns.nth(confirm).click(); }
}
const lastWarn = () => {
  const d = `${process.env.HOME}/.local/state/goose/logs/cli`; const day = readdirSync(d).sort().at(-1);
  const f = readdirSync(`${d}/${day}`).sort().at(-1);
  const w = readFileSync(`${d}/${day}/${f}`, 'utf8').split('\n').filter((l) => /distributed engine/.test(l) && /"WARN"/.test(l)).at(-1);
  return w ? { at: Date.parse(JSON.parse(w).timestamp), text: JSON.parse(w).fields.message } : { at: 0, text: '' };
};
while (true) {
  await p.waitForTimeout(5000);
  const s = await status(); const secs = ((Date.now() - t0) / 1000).toFixed(0);
  // The card's "Failed" badge can be the previous attempt's; only a WARN logged after this click counts.
  const warn = lastWarn(); const failed = warn.at > t0 && /preflight|exited|ended|refused|failed|stale/i.test(warn.text);
  // A switch keeps the previous split running until it stops: running alone is not up — it must serve the pick.
  if (/distributed\/running/.test(s) && (await servesWanted())) await complete(secs);
  // A Studio single is served over the Link relay, not 8091: running is read from goose's own activity.
  if (place === 'studio' && /\/running/.test(s) && !/^distributed/.test(s)) { console.log(`${secs}s studio single up: ${s}`); process.exit(0); }
  if (failed) { console.log(`${secs}s split FAILED:\n${warn.text}`); process.exit(1); }
  if (Number(secs) % 30 < 5) console.log(`${secs}s ${s}`);
}
