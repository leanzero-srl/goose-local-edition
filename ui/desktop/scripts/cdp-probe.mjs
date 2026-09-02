#!/usr/bin/env node
/**
 * Read the RUNNING packaged app over CDP — the "verify in the running app" rule's tool.
 * Compiling and green tests are not evidence; two committed UI changes were dead on arrival
 * until someone actually opened the app. This is how you look, from the terminal.
 *
 * Launch the app with a debugging port first:
 *   pnpm run package
 *   open -n out/Goose-darwin-arm64/Goose.app --args --remote-debugging-port=9897
 *
 * Then:
 *   node scripts/cdp-probe.mjs --eval "document.title"
 *   node scripts/cdp-probe.mjs --shot /tmp/app.png
 *   node scripts/cdp-probe.mjs --shot /tmp/nav.png --clip 0,40,250,130 --scale 5
 *   node scripts/cdp-probe.mjs --port 9897 --eval "..."          # default port 9897
 *
 * --eval prints the value (JSON.stringify your expression to get structure back). --clip is
 * x,y,w,h in CSS pixels. No dependencies: Node 24's global WebSocket does the talking.
 */
import { writeFileSync } from 'node:fs';

const args = process.argv.slice(2);
const opt = (name, dflt) => {
  const i = args.indexOf(name);
  return i >= 0 ? args[i + 1] : dflt;
};
const port = Number(opt('--port', '9897'));
const expr = opt('--eval', null);
const shot = opt('--shot', null);
const clip = opt('--clip', null);
const scale = Number(opt('--scale', '2'));
const waitMs = Number(opt('--wait', '600'));

if (!expr && !shot) {
  console.error('cdp-probe: give --eval <js> and/or --shot <out.png>');
  process.exit(2);
}

let targets;
try {
  targets = await (await fetch(`http://127.0.0.1:${port}/json/list`)).json();
} catch {
  console.error(`cdp-probe: nothing listening on 127.0.0.1:${port} — launch the packaged app with --remote-debugging-port=${port}`);
  process.exit(1);
}
const page = targets.find((t) => t.type === 'page');
if (!page) {
  console.error('cdp-probe: the app is up but has no page target (still booting?)');
  process.exit(1);
}

const ws = new WebSocket(page.webSocketDebuggerUrl);
await new Promise((res) => ws.addEventListener('open', res));
let seq = 0;
const send = (method, params) =>
  new Promise((res) => {
    const id = ++seq;
    const handler = (m) => {
      const d = JSON.parse(m.data);
      if (d.id === id) {
        ws.removeEventListener('message', handler);
        res(d);
      }
    };
    ws.addEventListener('message', handler);
    ws.send(JSON.stringify({ id, method, params }));
  });

if (expr) {
  const r = await send('Runtime.evaluate', { expression: expr, returnByValue: true, awaitPromise: true });
  if (r.result?.exceptionDetails) {
    console.error('cdp-probe: evaluate threw —', r.result.exceptionDetails.text);
    process.exit(1);
  }
  console.log(JSON.stringify(r.result?.result?.value ?? null, null, 1));
}

if (shot) {
  await new Promise((res) => setTimeout(res, waitMs));
  const params = { format: 'png' };
  if (clip) {
    const [x, y, width, height] = clip.split(',').map(Number);
    params.clip = { x, y, width, height, scale };
  }
  const r = await send('Page.captureScreenshot', params);
  if (!r.result?.data) {
    console.error('cdp-probe: screenshot returned no data');
    process.exit(1);
  }
  writeFileSync(shot, Buffer.from(r.result.data, 'base64'));
  console.log(`saved ${shot}`);
}

ws.close();
