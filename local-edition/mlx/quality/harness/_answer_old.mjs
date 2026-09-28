import { chromium } from '/Users/mihaiperdum/Projects/goose/ui/node_modules/playwright-core/index.mjs';
const [sid, optText] = process.argv.slice(2);
const b = await chromium.connectOverCDP('http://127.0.0.1:9333');
const p = b.contexts()[0].pages().find((x) => x.url().includes('main_window') && !x.url().includes('engine-glance'));
const base = p.url().split('#')[0];
await p.goto(`${base}#/pair?resumeSessionId=${sid}`); await p.waitForTimeout(6000);
const fold = p.getByTestId('needs-you-fold-summary'); if (await fold.count()) { await fold.first().click(); await p.waitForTimeout(800); }
const cards = p.getByTestId('needs-you-card'); console.log('cards open:', await cards.count());
for (let k = 0; k < await cards.count(); k++) console.log(' Q:', (await cards.nth(k).getByTestId('needs-you-question').innerText()).slice(0, 140));
const opt = p.getByTestId('needs-you-option').filter({ hasText: optText });
console.log('matching options:', await opt.count());
if (await opt.count()) { await opt.first().click(); console.log('clicked', optText); }
await p.waitForTimeout(4000);
console.log('cards after:', await cards.count());
await b.close();
