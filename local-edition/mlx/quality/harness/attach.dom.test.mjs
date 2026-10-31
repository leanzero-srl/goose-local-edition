// node --test local-edition/mlx/quality/harness/attach.dom.test.mjs — attach's DOM half against a FIXTURE that copies
// ChatInput.tsx's composer markup: the [data-drop-zone] root, its hidden <input type=file> whose change handler
// reads files[0] and clears the input, the image tile (spinner, then <img alt="Pasted image …" src=data:…>) and
// MediaAttachment's chip ([data-testid=media-attachment][data-kind], the name in title, <audio src> once read).
// It proves the selector, the one-file-per-change path and the chip wait agree with that markup — not that the
// installed app draws it; that is the live check. Never touches the running app. Also turnend.mjs's notice read.
import { test } from 'node:test';
import assert from 'node:assert/strict';
import { existsSync, readdirSync, mkdtempSync, mkdirSync, copyFileSync, rmSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { fileURLToPath } from 'node:url';
import { chromium } from './pw.mjs';
import { attachPlan, attachFiles, composerHasAttachments } from './attach.mjs';
import { readNotices, turnEndVerdict } from './turnend.mjs';

const COMPOSER = `<!doctype html><body>
  <div data-drop-zone="true" id="root">
    <input type="file" style="display:none" accept="*/*">
    <form><textarea data-testid="chat-input"></textarea></form>
  </div>
  <script>
    window.changes = 0; window.drawChips = true; window.readMs = 300;
    const root = document.getElementById('root'); const input = root.querySelector('input[type=file]');
    const row = () => root.querySelector('.preview') ?? root.appendChild(Object.assign(document.createElement('div'), { className: 'preview flex flex-wrap gap-2 p-4 mt-2 border-t' }));
    input.addEventListener('change', (e) => {
      window.changes++;
      const f = e.target.files[0]; if (!f) return;
      if (window.drawChips) {
        const id = Math.random().toString(36).slice(2);
        if (f.type.startsWith('image/')) {
          const t = Object.assign(document.createElement('div'), { className: 'relative group w-20 h-20', innerHTML: '<div class="absolute inset-0"><div class="animate-spin"></div></div>' });
          row().appendChild(t);
          const r = new FileReader(); r.onload = () => setTimeout(() => { t.innerHTML = '<img alt="Pasted image upload-' + id + '" class="w-full h-full">'; t.querySelector('img').src = r.result; }, window.readMs); r.readAsDataURL(f);
        } else if (f.type.startsWith('audio/') || f.type.startsWith('video/')) {
          const kind = f.type.startsWith('audio/') ? 'audio' : 'video';
          const t = Object.assign(document.createElement('div'), { className: 'relative group' });
          t.innerHTML = '<div class="relative"><div data-testid="media-attachment" data-kind="' + kind + '"><div><span title="' + f.name + '">' + f.name + '</span></div></div><div class="absolute inset-0"><div class="animate-spin"></div></div></div>';
          row().appendChild(t);
          const r = new FileReader(); r.onload = () => setTimeout(() => { const m = t.querySelector('[data-testid=media-attachment]'); const a = document.createElement(kind); a.src = 'data:' + f.type + ';base64,AA'; m.prepend(a); t.querySelector('.animate-spin').parentElement.remove(); }, window.readMs); r.readAsArrayBuffer(f);
        }
      }
      e.target.value = '';
    });
  </script></body>`;

const CACHE = `${process.env.HOME}/Library/Caches/ms-playwright`;
const shell = existsSync(CACHE) ? readdirSync(CACHE).filter((d) => d.startsWith('chromium_headless_shell-')).sort().reverse()
  .map((d) => `${CACHE}/${d}/chrome-headless-shell-mac-arm64/chrome-headless-shell`).find(existsSync) : undefined;
let browser;
try { browser = await chromium.launch(shell ? { executablePath: shell } : {}); } catch (e) { browser = null; console.log(`# SKIP: no headless chromium (${String(e.message).split('\n')[0]})`); }

const SEED = fileURLToPath(new URL('../briefs/seeds/2026-10-31-3', import.meta.url));
const work = mkdtempSync(`${tmpdir()}/attach-dom-`); mkdirSync(`${work}/inbox`);
for (const f of ['booking-screenshot.png', 'voice-note-1.wav', 'voice-note-2.m4a']) copyFileSync(`${SEED}/${f}`, `${work}/inbox/${f}`);
const page = async () => { const p = await browser.newPage(); await p.setContent(COMPOSER); return p; };

test('a screenshot and two voice notes: one change each, every chip waited for, all attached', { skip: !browser }, async () => {
  const p = await page();
  const plan = attachPlan({ attach: ['{WORK}/inbox/booking-screenshot.png', '{WORK}/inbox/voice-note-1.wav', 'inbox/voice-note-2.m4a'] }, work);
  const t0 = Date.now();
  const res = await attachFiles(p, plan, { waitMs: 5000, pollMs: 50 });
  assert.deepEqual(res.map((r) => [r.name, r.ok, r.why]), [
    ['booking-screenshot.png', true, 'image tile drawn with its data'],
    ['voice-note-1.wav', true, 'audio chip drawn with its data'],
    ['voice-note-2.m4a', true, 'audio chip drawn with its data'],
  ]);
  assert.ok(Date.now() - t0 >= 900, 'it waited for each read (3 x 300 ms), it did not run past the spinners');
  assert.equal(await p.evaluate(() => window.changes), 3);
  assert.equal(await composerHasAttachments(p), true);
  await p.close();
});

test('a chip that never appears fails loudly after the wait — the turn is never reported attached', { skip: !browser }, async () => {
  const p = await page(); await p.evaluate(() => { window.drawChips = false; });
  const res = await attachFiles(p, attachPlan({ attach: ['inbox/voice-note-1.wav', 'inbox/booking-screenshot.png'] }, work), { waitMs: 800, pollMs: 50 });
  assert.deepEqual(res.map((r) => r.ok), [false, false]);
  assert.match(res[0].why, /^no chip after 0\.8s: no audio chip named voice-note-1\.wav$/);
  assert.match(res[1].why, /^no chip after 0\.8s: no new image tile$/);
  await p.close();
});

test('a missing file is never handed to the input', { skip: !browser }, async () => {
  const p = await page();
  const res = await attachFiles(p, attachPlan({ attach: ['inbox/not-there.png'] }, work), { waitMs: 500, pollMs: 50 });
  assert.deepEqual(res.map((r) => [r.ok, r.why]), [[false, 'missing file']]);
  assert.equal(await p.evaluate(() => window.changes), 0);
  await p.close();
});

test('Q-527: the provider error notice (no "Ran into this error" words on screen) ends the turn as error', { skip: !browser }, async () => {
  const p = await browser.newPage();
  await p.setContent(`<body>
    <div class="goose-message"><p>an earlier reply</p><div role="alert" data-testid="provider-error-notice" data-class="other"><p>old error</p></div></div>
    <div class="goose-message"><div role="alert" data-testid="provider-error-notice" data-class="execution" data-transient="false">
      <p data-testid="provider-error-headline">goose could not send this request</p>
      <p data-testid="provider-error-said">swarm chat: no swarm block in config.yaml</p></div></div></body>`);
  const n = await readNotices(p, 1);
  assert.deepEqual(n.map((x) => [x.testid, x.cls]), [['provider-error-notice', 'execution']]);
  assert.match(n[0].text, /goose could not send this request swarm chat: no swarm block/);
  const v = turnEndVerdict('done', n);
  assert.equal(v.ended, 'error');
  assert.match(v.notice, /^\[provider-error-notice:execution\] goose could not send this request/);
  await p.setContent('<body><div class="goose-message"><p>All tests pass.</p></div></body>');
  assert.deepEqual(await readNotices(p, 0), []);
  await p.close();
});

test.after(async () => { await browser?.close(); rmSync(work, { recursive: true, force: true }); });
