// node --test local-edition/mlx/quality/harness/notes.dom.test.mjs — notes.mjs's DOM half against a FIXTURE that
// copies NoteDraftCard.tsx's markup (note-draft-tray, note-draft-card[data-note-id], note-target, note-target-live
// [data-live], note-text, the four buttons, the picker's filter and candidate rows, note-sent-line[data-outcome])
// in a headless Chromium. It proves the selectors and the click path agree with that markup — not that the
// installed app draws it; the live prove is the next E2E. Never touches the running app.
import { test } from 'node:test';
import { existsSync, readdirSync } from 'node:fs';
import assert from 'node:assert/strict';
import { chromium } from '/Users/mihaiperdum/Projects/goose/ui/node_modules/playwright-core/index.mjs';
import { readNoteTray, clickNoteSteps, planNoteSteps, chooseNoteAction, loadNoteGuidance } from './notes.mjs';

const card = (id, target, live) => `
  <section data-testid="note-draft-card" data-note-id="${id}">
    <div>Note to another chat</div>
    <p data-testid="note-target"><span>"${target}"</span><span> · ~/x · <span data-testid="note-target-live" data-live="${live}">not open in any window</span></span></p>
    <label><span>The note</span><textarea data-testid="note-text">Wrote the method up at docs/HOW-TO-PDF.md</textarea></label>
    <div><button type="button" data-testid="note-steer-now">Steer it now</button><button type="button" data-testid="note-leave-there">Leave it there</button>
      <button type="button" data-testid="note-not-this-chat">Not this chat</button><button type="button" data-testid="note-cancel">Cancel</button></div>
  </section>`;

const PAGE = `<!doctype html><body>
  <div hidden><div data-testid="note-draft-tray">${card('nt_other_chat', 'Hidden', 'idle')}</div></div>
  <div data-testid="note-draft-tray">${card('nt_1', 'Harbourline Jira Migration Assessment', 'not_open')}</div>
  <script>
    window.sent = [];
    const picker = () => '<div data-testid="note-picker"><input data-testid="note-picker-filter">' +
      '<button type="button" data-testid="note-candidate" data-session-id="20260928_47">"Harbourline Jira Migration Assessment" · ~/a</button>' +
      '<button type="button" data-testid="note-candidate" data-session-id="20260928_37">"Harbourline readiness report" · ~/b</button></div>';
    document.addEventListener('click', (e) => {
      const t = e.target.closest('button'); if (!t) return;
      const c = t.closest('[data-testid=note-draft-card]'); const id = t.dataset.testid;
      if (id === 'note-not-this-chat') { c.querySelector('[data-testid=note-target]').remove(); t.remove(); c.insertAdjacentHTML('afterbegin', picker()); }
      if (id === 'note-candidate') { const name = t.textContent.split('"')[1]; c.querySelector('[data-testid=note-picker]').remove();
        c.insertAdjacentHTML('afterbegin', '<p data-testid="note-target"><span>"' + name + '"</span></p>'); window.sent.push({ retarget: t.dataset.sessionId }); }
      if (id === 'note-steer-now' || id === 'note-leave-there') { window.sent.push({ note: c.dataset.noteId, delivery: id });
        c.outerHTML = '<div data-testid="note-sent-line" data-note-id="' + c.dataset.noteId + '" data-outcome="waiting"><span>Note sent to "x" · waiting there</span></div>'; }
      if (id === 'note-cancel') { window.sent.push({ cancel: c.dataset.noteId }); c.remove(); }
    });
  </script></body>`;

// ui's playwright-core pins a headless-shell build that may not be downloaded; any cached one drives this fixture.
const CACHE = `${process.env.HOME}/Library/Caches/ms-playwright`;
const shell = existsSync(CACHE) ? readdirSync(CACHE).filter((d) => d.startsWith('chromium_headless_shell-')).sort().reverse()
  .map((d) => `${CACHE}/${d}/chrome-headless-shell-mac-arm64/chrome-headless-shell`).find(existsSync) : undefined;
let browser;
try { browser = await chromium.launch(shell ? { executablePath: shell } : {}); } catch (e) { browser = null; console.log(`# SKIP: no headless chromium (${String(e.message).split('\n')[0]})`); }

const DRAFT = { id: 'nt_1', to_query: 'Harbourline Jira Migration Assessment', text: 'docx→PDF method at docs/HOW-TO-PDF.md', target: { session_id: '20260928_47', name: 'Harbourline Jira Migration Assessment' } };

test("reads only the visible chat's draft card: target, live state, the note's text", { skip: !browser }, async () => {
  const p = await browser.newPage(); await p.setContent(PAGE);
  const t = await readNoteTray(p);
  assert.deepEqual(t.drafts.map((d) => [d.id, d.live, d.text, d.picker]), [['nt_1', 'not_open', 'Wrote the method up at docs/HOW-TO-PDF.md', false]]);
  assert.match(t.drafts[0].target, /^"Harbourline Jira Migration Assessment"/);
  assert.deepEqual(t.sent, []);
  await p.close();
});

test('Steer it now: one click on the card, and the card becomes its sent line', { skip: !browser }, async () => {
  const p = await browser.newPage(); await p.setContent(PAGE);
  const plan = planNoteSteps(DRAFT, chooseNoteAction(DRAFT, loadNoteGuidance({ notes: [{ match: 'harbourline pdf', action: 'steer' }] })));
  assert.equal(await clickNoteSteps(p, 'nt_1', plan.steps), 'note-steer-now');
  assert.deepEqual(await p.evaluate(() => window.sent), [{ note: 'nt_1', delivery: 'note-steer-now' }]);
  assert.deepEqual((await readNoteTray(p)).sent, [{ id: 'nt_1', outcome: 'waiting', text: 'Note sent to "x" · waiting there' }]);
  await p.close();
});

test('Not this chat: the picker opens, the filter takes the words, the named chat (not the current one) is picked, then sent', { skip: !browser }, async () => {
  const p = await browser.newPage(); await p.setContent(PAGE);
  const g = loadNoteGuidance({ notes: [{ match: 'harbourline', action: 'not-this-chat', pick: 'harbourline', then: 'leave' }] });
  const clicks = await clickNoteSteps(p, 'nt_1', planNoteSteps(DRAFT, chooseNoteAction(DRAFT, g)).steps);
  assert.equal(clicks, 'note-not-this-chat → pick 20260928_37 → note-leave-there');
  assert.deepEqual(await p.evaluate(() => window.sent), [{ retarget: '20260928_37' }, { note: 'nt_1', delivery: 'note-leave-there' }]);
  await p.close();
});

test('Cancel: the card goes and nothing is sent', { skip: !browser }, async () => {
  const p = await browser.newPage(); await p.setContent(PAGE);
  await clickNoteSteps(p, 'nt_1', planNoteSteps(DRAFT, { action: 'cancel' }).steps);
  const t = await readNoteTray(p);
  assert.deepEqual([t.drafts.length, t.sent.length], [0, 0]);
  await p.close();
});

test.after(async () => { await browser?.close(); });
