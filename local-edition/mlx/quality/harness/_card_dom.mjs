import { chromium } from '/Users/mihaiperdum/Projects/goose/ui/node_modules/playwright-core/index.mjs';
const b = await chromium.connectOverCDP('http://127.0.0.1:9333');
const p = b.contexts()[0].pages().find((x) => x.url().includes('main_window') && !x.url().includes('engine-glance'));
const c = p.getByTestId('needs-you-card').first();
console.log((await c.innerText()).slice(0, 1200));
console.log('testids:', await c.evaluate((n) => [...n.querySelectorAll('[data-testid]')].map((e) => e.dataset.testid + ':' + e.tagName + ':' + (e.innerText||'').slice(0,40).replace(/\n/g,' '))));
await b.close();
