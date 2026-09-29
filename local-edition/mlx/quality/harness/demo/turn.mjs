// node turn.mjs <label> <message>  — send one message in the open chat, record every surface change until the turn ends
import { page, shot, DIR } from './lib.mjs';
import { appendFileSync } from 'node:fs';
const [label, msg] = process.argv.slice(2);
const { p } = await page();
const log = (o) => { const l = JSON.stringify({ t: new Date().toISOString(), label, ...o }); appendFileSync(`${DIR}/turns.jsonl`, l + '\n'); if (o.ev !== 'change') console.log(l.slice(0, 300)); };
if (process.env.SESSION) { await p.goto(p.url().split('#')[0] + '#/pair?resumeSessionId=' + process.env.SESSION); await p.waitForTimeout(4000); }
if (process.env.NOWAIT) { const ta0 = p.locator('textarea[data-testid=chat-input]:visible').first(); await ta0.click(); await ta0.fill(msg); await ta0.press('Enter'); log({ ev: 'sent-nowait', msg }); await p.waitForTimeout(3000); await shot(p, label + '-sent'); process.exit(0); }
const read = () => p.evaluate(() => {
  const ta = [...document.querySelectorAll('textarea[data-testid=chat-input]')].find((e) => e.offsetParent);
  let c = ta; for (let i = 0; i < 6 && c?.parentElement; i++) c = c.parentElement;
  const composer = (c?.innerText ?? '').replace(/\s+/g, ' ').trim();
  const stop = !!document.querySelector('button[aria-label="Stop"]');
  const eng = document.querySelector('button[aria-label="Open the Engine"]');
  let g = eng; for (let i = 0; i < 5 && g?.parentElement; i++) { g = g.parentElement; if ((g.innerText || '').length > 120) break; }
  const glance = (g?.innerText ?? '').replace(/\s+/g, ' ').trim().slice(0, 400);
  const msgs = [...document.querySelectorAll('[data-testid*=message], .goose-message, [data-role=assistant]')];
  const main = document.querySelector('[data-testid=chat-messages]') || document.querySelector('main') || document.body;
  const mt = (main.innerText || '');
  const cards = [...document.querySelectorAll('*')].filter((e) => e.childElementCount < 40 && /delegate|subagent/i.test(e.getAttribute('data-testid') || '')).map((e) => e.innerText.replace(/\s+/g, ' ').slice(0, 200));
  const dl = mt.split('\n').filter((x) => /delegate|subagent|Loading .* for this|^on (Qwen|deepseek)|can.t run|failed to load|Waiting for/i.test(x)).map((x) => x.slice(0, 200)).slice(-12);
  return { dl, composer: composer.slice(0, 500), stop, glance, mainLen: mt.length, mainTail: mt.slice(-700).replace(/\s+/g, ' '), cards: cards.slice(0, 6) };
});
const ta = p.locator('textarea[data-testid=chat-input]:visible').first();
await ta.click(); await ta.fill(msg); await p.waitForTimeout(300);
const t0 = Date.now();
await ta.press('Enter');
log({ ev: 'sent', msg });
let prev = {}; let idle = 0; let shots = 0; let firstStream = null;
for (let i = 0; i < 1800; i++) {
  await p.waitForTimeout(2000);
  let r; try { r = await read(); } catch (e) { log({ ev: 'readErr', e: String(e).slice(0, 200) }); continue; }
  const secs = +((Date.now() - t0) / 1000).toFixed(1);
  const ch = {};
  for (const k of ['composer', 'stop', 'glance']) if (JSON.stringify(r[k]) !== JSON.stringify(prev[k])) ch[k] = r[k];
  if (JSON.stringify(r.cards) !== JSON.stringify(prev.cards)) ch.cards = r.cards;
  if (JSON.stringify(r.dl) !== JSON.stringify(prev.dl)) ch.dl = r.dl;
  if (Object.keys(ch).length) { log({ ev: 'change', secs, ...ch }); if (shots < 8) { await shot(p, `${label}-${String(++shots).padStart(2, '0')}-${Math.round(secs)}s`); } }
  if (firstStream == null && prev.mainLen && r.mainLen > prev.mainLen + 20 && r.stop) { firstStream = secs; log({ ev: 'textGrowing', secs }); }
  prev = r;
  if (!r.stop && secs > 6) { idle++; if (idle >= 3) break; } else idle = 0;
}
const secs = +((Date.now() - t0) / 1000).toFixed(1);
await shot(p, `${label}-end`);
log({ ev: 'end', secs, tail: prev.mainTail });
process.exit(0);
