import { chromium } from '/Users/mihaiperdum/Projects/goose/ui/node_modules/playwright-core/index.mjs';
import { mainPage } from '/Users/mihaiperdum/Projects/goose/local-edition/mlx/quality/harness/mainpage.mjs';
export const DIR = '/Users/mihaiperdum/goose-builds/quality/DEMO-2026-09-28-strategy';
export async function page() {
  const b = await chromium.connectOverCDP('http://127.0.0.1:9333');
  const p = await mainPage(b); p.setDefaultTimeout(15000); return { b, p };
}
export const shot = (p, name) => p.screenshot({ path: `${DIR}/${name}.png` }).then(() => console.log('shot', name));
export const txt = (p, sel = 'body') => p.locator(sel).first().innerText().then((t) => t.replace(/\n{2,}/g, '\n'));
