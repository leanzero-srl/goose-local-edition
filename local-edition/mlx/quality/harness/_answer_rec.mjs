import { chromium } from '/Users/mihaiperdum/Projects/goose/ui/node_modules/playwright-core/index.mjs';
const b = await chromium.connectOverCDP('http://127.0.0.1:9333');
const p = b.contexts()[0].pages().find((x) => x.url().includes('main_window') && !x.url().includes('engine-glance'));
const rec = p.getByTestId('needs-you-recommended').first();
console.log('clicking:', (await rec.innerText()).slice(0, 80)); await rec.click();
const t0 = Date.now();
for (let k = 0; k < 120; k++) {
  await p.waitForTimeout(5000);
  const cards = await p.getByTestId('needs-you-card').count();
  const stop = await p.locator('button[aria-label="Stop"]').count();
  if (k % 6 === 0 || (!stop && k > 2)) console.log(`${((Date.now()-t0)/1000)|0}s cards=${cards} running=${stop}`);
  if (!stop && k > 2) break;
}
const msgs = await p.evaluate(() => [...document.querySelectorAll('[data-testid*="message"], .message')].slice(-2).map((n) => n.innerText.slice(0, 400)));
console.log('last:', JSON.stringify(msgs).slice(0, 900));
await b.close();
