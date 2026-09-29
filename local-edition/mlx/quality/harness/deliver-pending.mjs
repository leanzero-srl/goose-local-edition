// deliver-pending.mjs — AFTER an E2E has ended: act on the note drafts a chat still holds, the way r1 now does
// at every turn boundary (notes.mjs actOnNote), and record the evidence. Written for E2E #3w, whose turn 20
// drafted a note to "Harbourline Jira Migration Assessment" (20260928_47) from 20260929_12 that nobody clicked
// — r1 did not act on notes then.
//
// usage: node deliver-pending.mjs <session> --out <dir> [--brief <brief.json>] [--action steer|leave|cancel]
//                                  [--cdp <port>] [--dry-run]
//   --brief   the brief's notes.actions guidance picks the action (as r1); no guidance → Leave it there
//   --action  the operator's explicit action for every pending draft (overrides the brief)
//   --dry-run READ-ONLY: prints each pending draft and what would be clicked; no CDP, no app
// #3w: node deliver-pending.mjs 20260929_12 --out ~/goose-builds/quality/RU-2026-09-29-3w-split-tensor-cafe \
//        --brief ../briefs/2026-09-29-4-cafe-allergen-menu-site.json
//
// The chat is acted on in a window of its own: an existing window already showing it, else a NEW window
// (the session list's "Open in new window") that is closed at the end — whatever the main window shows is
// never navigated. A Steer to a chat no window shows opens THAT chat in a new window too (notes.mjs), and B's
// turn is waited on by progress (its Stop button), never by a clock. Evidence: <out>/notes.tsv (appended),
// <out>/deliver-pending.json, <out>/events.log. Exit 0 = every note acted on and proven; 1 = a FAIL line;
// 3 = refused (an E2E driver is still running, or no such chat).
import { execFileSync } from 'node:child_process';
import { appendFileSync, existsSync, readFileSync, writeFileSync, mkdirSync } from 'node:fs';
import { resolve } from 'node:path';
import {
  loadNoteGuidance, chooseNoteAction, planNoteSteps, readNotes, readChatRow, actOnNote, noteFails, noteRow,
  openChatWindow, closeChatWindow, NOTES_TSV_HEADER,
} from './notes.mjs';

const argv = process.argv.slice(2);
const arg = (k) => { const i = argv.indexOf(k); return i >= 0 ? argv[i + 1] : ''; };
const session = argv[0] && !argv[0].startsWith('--') ? argv[0] : '';
const dry = argv.includes('--dry-run');
const out = arg('--out') ? resolve(arg('--out')) : '';
if (!session || (!dry && !out)) { console.error('usage: node deliver-pending.mjs <session> --out <dir> [--brief <brief.json>] [--action steer|leave|cancel] [--cdp <port>] [--dry-run]'); process.exit(2); }
const forced = arg('--action');
if (forced && !['steer', 'leave', 'cancel'].includes(forced)) { console.error(`--action must be steer | leave | cancel (not-this-chat needs a pick: put it in the brief's notes.actions)`); process.exit(2); }
const guidance = loadNoteGuidance(arg('--brief') ? JSON.parse(readFileSync(arg('--brief'), 'utf8')) : null);
const choiceFor = (d) => (forced ? { match: '', action: forced, pick: '', then: 'leave', source: 'operator --action' } : chooseNoteAction(d, guidance));

const chat = readChatRow(session);
if (!chat) { console.error(`no chat ${session} in sessions.db`); process.exit(3); }
const a = readNotes(session);
if (!a.ok) { console.error(`the notes of ${session} could not be read: ${a.error}`); process.exit(3); }
const pending = a.state.drafts.filter((d) => d.status === 'draft');
console.log(JSON.stringify({ session, name: chat.name, drafts: a.state.drafts.length, pending: pending.length, guidance: forced ? `--action ${forced}` : `${guidance.length} entr${guidance.length === 1 ? 'y' : 'ies'}` }));

if (dry) {
  for (const d of pending) {
    const c = choiceFor(d); const plan = planNoteSteps(d, c);
    console.log(JSON.stringify({ note: d.id, to: d.to_query, target: d.target ? `${d.target.session_id} "${d.target.name}"` : `none (${d.resolution})`, action: c.action, source: c.source, match: c.match, clicks: plan.steps?.map((s) => s.testid ?? `pick "${s.words}"`) ?? null, unactionable: plan.unactionable ?? null, text: d.text.slice(0, 160) }, null, 1));
  }
  if (!pending.length) console.log(`no pending note drafts in ${session} — nothing to deliver`);
  process.exit(0);
}
if (!pending.length) { console.log(`no pending note drafts in ${session} — nothing to deliver`); process.exit(0); }

// An E2E driver on this app must not have a second hand on it (its own chat, its own clicks): refuse.
let drivers = '';
try { drivers = execFileSync('pgrep', ['-fl', 'r1.mjs|load.py'], { encoding: 'utf8' }).trim(); } catch { drivers = ''; }
if (drivers) { console.error(`refused: an E2E driver is still running — deliver-pending acts after the run ends:\n${drivers}`); process.exit(3); }

mkdirSync(out, { recursive: true });
const iso = () => new Date().toISOString();
const note = (line) => appendFileSync(`${out}/events.log`, `${iso()} deliver-pending ${line}\n`);
const tsv = `${out}/notes.tsv`;
if (!existsSync(tsv)) writeFileSync(tsv, NOTES_TSV_HEADER);

const { chromium } = await import('/Users/mihaiperdum/Projects/goose/ui/node_modules/playwright-core/index.mjs');
const { mainPage } = await import('./mainpage.mjs');
const b = await chromium.connectOverCDP(`http://127.0.0.1:${arg('--cdp') || '9333'}`);
let page = b.contexts()[0].pages().find((pg) => pg.url().includes(`resumeSessionId=${session}`));
let opened = false;
if (!page) { page = await openChatWindow(b, await mainPage(b), chat); opened = true; note(`opened ${session} in a new window (Open in new window)`); }
await page.waitForTimeout(3000);

const recs = [];
for (const d of pending) {
  const rec = await actOnNote({ browser: b, page, sessionA: session, turn: 'deliver-pending', hangSecs: () => null, note, choice: forced ? choiceFor(d) : undefined }, d, guidance);
  recs.push(rec);
  appendFileSync(tsv, noteRow(rec));
  note(`${d.id} ${rec.action} → ${rec.inboxStatus || rec.draftStatus} ${rec.fails.length ? `FAIL ${rec.fails.join(' | ')}` : 'proven'}`);
  console.log(JSON.stringify({ note: d.id, action: rec.action, source: rec.source, target: rec.targetName, draft: rec.draftStatus, inbox: rec.inboxStatus, deliveredHow: rec.deliveredHow, windowOpened: rec.windowOpened, noteMsgInB: rec.noteMsg, markerInB: rec.markerInB, replyUses: rec.replyUses, card: rec.card, fails: rec.fails }, null, 1));
}
if (opened) { await closeChatWindow(page); note(`closed the window of ${session}`); }
const fails = noteFails(readNotes(session), recs);
writeFileSync(`${out}/deliver-pending.json`, JSON.stringify({ session, at: iso(), recs: recs.map((r) => ({ ...r, reply: r.reply.slice(0, 600) })), fails }, null, 2) + '\n');
for (const f of fails) { note(f); console.log(f); }
if (!fails.length) console.log(`every pending note of ${session} acted on and proven (${recs.length})`);
await b.close();
process.exit(fails.length ? 1 : 0);
