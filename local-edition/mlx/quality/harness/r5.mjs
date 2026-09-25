// R5 switch races, driven through the installed app's Run it card (CDP 9333).
// usage: node r5.mjs <evidence-dir>   — each step: act, wait for the tile to settle, census both Macs.
import { chromium } from '/Users/mihaiperdum/Projects/goose/ui/node_modules/playwright-core/index.mjs';
import { execSync } from 'node:child_process';
const dir = process.argv[2];
const census = (label) =>
  execSync(`CENSUS_OUT=${dir}/census.jsonl /Users/mihaiperdum/Projects/goose/local-edition/mlx/quality/harness/census.sh ${label}`, { encoding: 'utf8' }).trim();
const b = await chromium.connectOverCDP('http://127.0.0.1:9333');
const p = b.contexts()[0].pages().find((x) => x.url().includes('index.html'));
p.setDefaultTimeout(15000);
const log = (...a) => console.log(new Date().toISOString().slice(11, 19), ...a);
async function openEngine() {
  await p.goto(p.url().split('#')[0] + '#/leanzero-swarm'); await p.waitForTimeout(2500);
  await p.getByText('LeanZero MLX', { exact: true }).first().click(); await p.waitForTimeout(4000);
}
const WAYS = { here: /Run on this Mac/, studio: /Run on Work.s Mac Studio/, both: /Run across both Macs/ };
async function buttonIndex(label, way) {
  return p.evaluate(({ label, src }) => {
    const re = new RegExp(src);
    const bs = [...document.querySelectorAll('button')].filter((b) => b.innerText.trim() === label);
    return bs.findIndex((b) => { let n = b; for (let k = 0; k < 8 && n; k++) { n = n.parentElement; if (!n) break;
      const heads = n.innerText.match(/Run on this Mac|Run on Work.s Mac Studio|Run across both Macs/g) || [];
      if (heads.length === 1) return re.test(heads[0]); if (heads.length > 1) return false; } return false; });
  }, { label, src: WAYS[way].source });
}
async function click(label, way) {
  const i = await buttonIndex(label, way);
  if (i < 0) { log(`NO ${label} button on ${way}`); return false; }
  await p.getByRole('button', { name: new RegExp(`^${label}$`) }).nth(i).click();
  log(`clicked ${label} on ${way}`); return true;
}
async function tile() {
  return p.evaluate(() => { const s = document.body.innerText; const a = s.indexOf('Sampling'); const r = s.indexOf('RUN IT');
    return (s.slice(a + 9, a + 170) + ' || ' + s.slice(r, r + 60)).replace(/\n+/g, ' | '); });
}
async function settle(tag, maxS = 420) {
  const t0 = Date.now(); let last = '', same = 0;
  while ((Date.now() - t0) / 1000 < maxS) {
    await p.waitForTimeout(3000);
    const t = await tile();
    same = t === last ? same + 1 : 0; last = t;
    if (same >= 3 && !/Mounting|Loading|Starting|Stopping|Checking|Restoring|Building/.test(t)) break;
  }
  log(`${tag} settled: ${last.slice(0, 220)}`);
  await p.screenshot({ path: `${dir}/${tag}.png` });
  log(census(tag));
}
await openEngine();
await settle('r5-0-start', 60);
// a) Run across both Macs, then — while it is still starting — Run on the Studio.
if (await click('Run', 'both')) { await p.waitForTimeout(4000); await click('Run', 'studio'); await settle('r5-a-both-then-studio'); }
// b) Run on this Mac, double-clicked.
{ const i = await buttonIndex('Run', 'here'); if (i >= 0) { const btn = p.getByRole('button', { name: /^Run$/ }).nth(i); await btn.dblclick(); log('double-clicked Run on here'); await settle('r5-b-here-double'); } }
// c) back to the Studio, then Run on this Mac while the Studio is still mounting.
if (await click('Run', 'studio')) { await p.waitForTimeout(3000); await click('Run', 'here'); await settle('r5-c-studio-then-here'); }
// d) end where the owner left it: the Studio.
if (await click('Run', 'studio')) await settle('r5-d-end-studio');
await b.close();
