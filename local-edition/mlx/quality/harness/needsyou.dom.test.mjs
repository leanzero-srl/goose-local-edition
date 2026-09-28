// node --test local-edition/mlx/quality/harness/needsyou.dom.test.mjs — Q-376's DOM half against a FIXTURE
// that copies NeedsYouCard.tsx's markup (data-testids, the fold band, the stack bar, hidden bodies, the Queued
// row) in a headless Chromium. It proves the selectors and the click path agree with that markup — not that the
// installed app draws it; the live prove is the next E2E. Never touches the running app.
import { test } from 'node:test';
import { existsSync, readdirSync } from 'node:fs';
import assert from 'node:assert/strict';
import { chromium } from '/Users/mihaiperdum/Projects/goose/ui/node_modules/playwright-core/index.mjs';
import { readTray, readChat, answerCard, planClick } from './needsyou.mjs';

const card = (id, question, recommended, chips, folded) => `
  <section data-testid="needs-you-card" data-item-id="${id}" data-folded="${folded}">
    <button type="button" data-testid="needs-you-fold" aria-expanded="${!folded}">Needs you
      ${folded ? `<span data-testid="needs-you-fold-summary">${question}</span>` : ''}</button>
    <div class="body" ${folded ? 'hidden' : ''}>
      <p data-testid="needs-you-question">${question}</p>
      <button type="button" data-testid="needs-you-recommended">${recommended}</button>
      ${chips.map((c) => `<button type="button" data-testid="needs-you-option">${c}</button>`).join('')}
      <textarea data-testid="needs-you-input"></textarea>
      <button type="button" data-testid="needs-you-answer">Answer</button>
    </div>
  </section>`;

const PAGE = `<!doctype html><body>
  <div id="stop"></div>
  <div class="goose-message">earlier reply</div>
  <div hidden><div data-testid="needs-you-tray"><section data-testid="needs-you-card" data-item-id="ny_other_chat"></section></div></div>
  <div data-testid="needs-you-tray">
    <button type="button" data-testid="needs-you-stack" aria-expanded="false">Needs you · 2 questions
      <span data-testid="needs-you-stack-first">— Want X set as this chat's folder?</span></button>
    <div data-testid="needs-you-list" hidden>
      ${card('ny_folder', "Want X set as this chat's folder?", 'Yes', ['No — keep it on the home folder'], true)}
      ${card('ny_edi', 'Is inactive the active field or last_login?', 'The active field', [], false)}
    </div>
  </div>
  <script>
    window.busy = false; window.answers = [];
    const setBusy = (b) => { window.busy = b; document.getElementById('stop').innerHTML = b ? '<button aria-label="Stop">Stop</button>' : ''; };
    window.setBusy = setBusy;
    document.addEventListener('click', (e) => {
      const t = e.target.closest('button'); if (!t) return;
      const id = t.dataset.testid; const c = t.closest('[data-testid=needs-you-card]');
      if (id === 'needs-you-stack') { const l = document.querySelector('[data-testid=needs-you-list]'); l.hidden = !l.hidden; t.setAttribute('aria-expanded', String(!l.hidden)); }
      if (id === 'needs-you-fold') { const f = c.dataset.folded === 'true'; c.dataset.folded = String(!f); c.querySelector('.body').hidden = !f; }
      const text = id === 'needs-you-recommended' || id === 'needs-you-option' ? t.textContent.trim() : id === 'needs-you-answer' ? c.querySelector('textarea').value : null;
      if (text === null) return;
      window.answers.push({ item: c.dataset.itemId, text });
      if (window.busy) c.querySelector('.body').insertAdjacentHTML('afterbegin', '<div data-testid="needs-you-queued">Queued · answers when this turn ends<p data-testid="needs-you-queued-answer">' + text + '</p></div>');
      else document.body.insertAdjacentHTML('beforeend', '<div data-testid="user-message-body">Answer to your question "' + c.querySelector('[data-testid=needs-you-question]').textContent + '": ' + text + '</div>');
    });
  </script></body>`;

// ui's playwright-core pins a headless-shell build that may not be downloaded; any cached one drives this fixture.
const CACHE = `${process.env.HOME}/Library/Caches/ms-playwright`;
const shell = existsSync(CACHE) ? readdirSync(CACHE).filter((d) => d.startsWith('chromium_headless_shell-')).sort().reverse()
  .map((d) => `${CACHE}/${d}/chrome-headless-shell-mac-arm64/chrome-headless-shell`).find(existsSync) : undefined;
let browser;
try { browser = await chromium.launch(shell ? { executablePath: shell } : {}); } catch (e) { browser = null; console.log(`# SKIP: no headless chromium (${String(e.message).split('\n')[0]})`); }

test('reads only the visible chat\'s tray, folded bar and folded card included', { skip: !browser }, async () => {
  const p = await browser.newPage(); await p.setContent(PAGE);
  const t = await readTray(p);
  assert.equal(t.trays, 1); assert.equal(t.stackFolded, true); assert.equal(t.busy, false);
  assert.deepEqual(t.cards.map((c) => [c.id, c.folded, c.recommended, c.chips]), [
    ['ny_folder', true, 'Yes', ['No — keep it on the home folder']],
    ['ny_edi', false, 'The active field', []],
  ]);
  await p.close();
});

test('idle: opens the folded bar and the folded card, clicks the recommended answer; the answer lands in the chat', { skip: !browser }, async () => {
  const p = await browser.newPage(); await p.setContent(PAGE);
  const [c] = (await readTray(p)).cards;
  await answerCard(p, c.id, planClick(c, 'Yes'));
  assert.deepEqual(await p.evaluate(() => window.answers), [{ item: 'ny_folder', text: 'Yes' }]);
  assert.equal((await readTray(p)).stackFolded, false);
  assert.match((await readChat(p)).users.at(-1), /^Answer to your question "Want X/);
  await p.close();
});

test('busy: a typed answer goes through the text box + Answer and the card shows Queued', { skip: !browser }, async () => {
  const p = await browser.newPage(); await p.setContent(PAGE); await p.evaluate(() => window.setBusy(true));
  const c = (await readTray(p)).cards[1];
  const el = await answerCard(p, c.id, planClick(c, 'By last_login, 24 months.'));
  await el.getByTestId('needs-you-queued').waitFor({ state: 'attached' });
  const t = await readTray(p);
  assert.equal(t.busy, true);
  assert.deepEqual(t.cards.map((x) => [x.id, x.queued, x.queuedAnswer]), [['ny_folder', false, ''], ['ny_edi', true, 'By last_login, 24 months.']]);
  assert.deepEqual((await readChat(p)).users, [], 'nothing is sent mid-turn');
  await p.close();
});

test('an option chip is clicked by index only when its words are the planned ones', { skip: !browser }, async () => {
  const p = await browser.newPage(); await p.setContent(PAGE);
  const [c] = (await readTray(p)).cards;
  await assert.rejects(answerCard(p, c.id, { kind: 'option', index: 0, text: 'Something else' }), /reads/);
  await answerCard(p, c.id, planClick(c, 'No — keep it on the home folder'));
  assert.deepEqual(await p.evaluate(() => window.answers), [{ item: 'ny_folder', text: 'No — keep it on the home folder' }]);
  await p.close();
});

test.after(async () => { await browser?.close(); });
