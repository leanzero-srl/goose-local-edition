// Public entrant utility. It contains no scorer logic or private fixtures.
import { createRequire } from 'node:module';
import path from 'node:path';

const args = process.argv.slice(2);
const gate = args[0] === 'load';
const [url, screenshot = 'self-test.png'] = gate ? args.slice(1) : args;
if (!url || !process.env.BENCH_BROWSER_MODULE || !process.env.BENCH_BROWSER_EXECUTABLE) {
  throw new Error('Usage: node browser-self-test.mjs http://127.0.0.1:PORT [screenshot.png]');
}
const { chromium } = createRequire(import.meta.url)(process.env.BENCH_BROWSER_MODULE);
const browser = await chromium.launch({
  headless: true,
  executablePath: process.env.BENCH_BROWSER_EXECUTABLE,
  args: ['--enable-unsafe-swiftshader'],
});
try {
  const page = await browser.newPage({ viewport: { width: 1440, height: 1000 } });
  const errors = [];
  const sources = [];
  page.on('pageerror', error => { errors.push(error.message); sources.push(error.stack || ''); });
  page.on('console', message => {
    if (message.type() === 'error') { errors.push(message.text()); sources.push(message.location().url || ''); }
  });
  // Payment updates keep an SSE connection open; network-idle is not page readiness.
  await page.goto(url, { waitUntil: 'domcontentloaded' });
  let readinessError = null;
  try { await page.locator('tbody tr').first().waitFor({ state: 'visible', timeout: 10000 }); }
  catch (error) { readinessError = error.message; }
  await page.screenshot({ path: path.resolve(screenshot), fullPage: true });
  const renderedRowCount = await page.locator('tbody tr').evaluateAll(rows =>
    rows.filter(row => row.getClientRects().length > 0 && row.querySelectorAll('td').length > 1).length);
  console.log(JSON.stringify({ title: await page.title(), errors, readinessError, screenshot: path.resolve(screenshot),
    renderedRowCount, consoleErrors: { count: errors.length, texts: errors, sources } }));
} finally {
  await browser.close();
}
