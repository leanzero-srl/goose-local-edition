// split-start.mjs — after EVERY install: start the split through Run it, as a user does, and
// prove it serves a completion. Q-113: 3.0.44 shipped a split that died 2 s after every launch, and nothing
// noticed until an E2E round tried to use it. Exit 0 = split up and answered; 1 = it failed (the goosed log's
// distributed WARN is printed); 2 = no split Run button on screen.
import { chromium } from '/Users/mihaiperdum/Projects/goose/ui/node_modules/playwright-core/index.mjs';
import { readdirSync, readFileSync } from 'node:fs';
const b = await chromium.connectOverCDP('http://127.0.0.1:9333');
const p = b.contexts()[0].pages().find((x) => x.url().includes('index.html'));
p.setDefaultTimeout(15000);
await p.goto(p.url().split('#')[0] + '#/leanzero-swarm'); await p.waitForTimeout(5000);
await p.getByText('LeanZero MLX', { exact: true }).first().click().catch(() => {}); await p.waitForTimeout(4000);
const owners = await p.evaluate(() => [...document.querySelectorAll('button')].filter((b) => b.innerText.trim() === 'Run').map((b) => {
  let n = b;
  for (let k = 0; k < 8 && n; k++) { n = n.parentElement; const h = n?.innerText.match(/Run on this Mac|Run on Work.s Mac Studio|Run across both Macs/g) || []; if (h.length === 1) return h[0]; if (h.length > 1) return '?'; }
  return 'none';
}));
const status = () => p.evaluate(async () => { const s = await window.electron.mlxEngineActivity(); return `${s.engine}/${s.mode} ${s.statusDetail ?? ''}`; });
const complete = async (secs) => {
  const r = await fetch('http://127.0.0.1:8091/v1/chat/completions', { method: 'POST', headers: { 'content-type': 'application/json' }, body: JSON.stringify({ messages: [{ role: 'user', content: 'Say OK.' }], max_tokens: 8 }) }).then((x) => x.json()).catch((e) => ({ error: String(e) }));
  console.log(`${secs}s split up; completion:`, JSON.stringify(r.choices?.[0]?.message ?? r).slice(0, 200));
  process.exit(r.choices ? 0 : 1);
};
// A restore after install may already have brought the split back: then there is no Run to press.
if (/distributed\/running/.test(await status())) await complete(0);
const i = owners.indexOf('Run across both Macs');
if (i < 0) { console.log('no split Run button', JSON.stringify(owners)); process.exit(2); }
const t0 = Date.now();
await p.getByRole('button', { name: /^Run$/ }).nth(i).click();
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
  if (/distributed\/running/.test(s)) await complete(secs);
  if (failed) { console.log(`${secs}s split FAILED:\n${warn.text}`); process.exit(1); }
  if (Number(secs) % 30 < 5) console.log(`${secs}s ${s}`);
}
