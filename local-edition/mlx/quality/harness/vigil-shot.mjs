// vigil-shot.mjs [outDir] — screenshot the goose MAIN window over CDP (port 9333) WITHOUT navigating it, so the
// vigil LOOKS at what the person sees every tick, even while an E2E drives the same window. Owner, 2026-09-29, on
// a crammed "Active now" list no tick had noticed: "as part of your constant vigil … I don't know why you didn't
// see this". Read-only: page.screenshot only — no goto, no click, no key.
import { chromium } from '/Users/mihaiperdum/Projects/goose/ui/node_modules/playwright-core/index.mjs';
import { mainPage } from './mainpage.mjs';
import { mkdirSync } from 'node:fs';
const out = process.argv[2] || `${process.env.HOME}/goose-builds/quality/VIGIL-${new Date().toISOString().slice(0, 10)}`;
mkdirSync(out, { recursive: true });
const b = await chromium.connectOverCDP('http://127.0.0.1:9333');
const p = await mainPage(b);
const file = `${out}/${new Date().toISOString().slice(11, 19).replace(/:/g, '')}.png`;
await p.screenshot({ path: file });
console.log(file);
process.exit(0);
