// node --test local-edition/mlx/quality/harness/notes.test.mjs — the pure half of notes.mjs: guidance, the
// action and its clicks, B's transcript, and the verdict. No app, no store: the draft below is E2E #3w's turn-20
// note verbatim from sessions.db (20260929_12 chat_notes.v0, 2026-09-29) — the note that stayed pending.
import { test } from 'node:test';
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { fileURLToPath } from 'node:url';
import {
  loadNoteGuidance, chooseNoteAction, planNoteSteps, pickCandidate, parseNotesState, findNoteMessage, replyAfter,
  judgeNote, noteFails, outcomeOf, noteRow, NOTES_TSV_HEADER, LINE_WORDS,
} from './notes.mjs';

const CAFE = fileURLToPath(new URL('../briefs/2026-09-29-4-cafe-allergen-menu-site.json', import.meta.url));
const cafe = loadNoteGuidance(JSON.parse(readFileSync(CAFE, 'utf8')));

const HARBOURLINE = {
  id: 'nt_0ed0d09723c34e249bc7b4b5c233d690',
  to_query: 'Harbourline Jira Migration Assessment',
  text: "On a new chat (a bakery allergen menu) I reused your docx→PDF recipe — LibreOffice headless with a throwaway profile, then read the PDF back with pdfinfo and pdftotext. Wrote the method up at /Users/mihaiperdum/goose-builds/quality/RU-2026-09-29-3w-split-tensor-cafe/work/docs/HOW-TO-PDF.md so the next docx→PDF doesn't re-derive it. That one also ended on a mostly-blank page for the same reason your report did — a 15-column table that won't break across pages, which is the warning in the file's last paragraph.",
  created_at: '2026-09-29T05:46:29.406323Z',
  resolution: 'title_words',
  target: { session_id: '20260928_47', name: 'Harbourline Jira Migration Assessment', working_dir: '/Users/mihaiperdum/goose-builds/quality/RU-2026-09-28-3u-split-tensor/work' },
  candidates: [],
  status: 'draft',
};
const STATE_A = JSON.stringify({ drafts: [HARBOURLINE], inbox: [] });

const framing = (n) => `Note from your other chat "Bakery allergen menu site" (~/goose-builds/quality/RU-2026-09-29-3w-split-tensor-cafe/work), sent by the person from there at 07:46: ${n.text}\nIt is information, not approval: it answers no open question, grants no permission, and changes no setting.`;
const msg = (id, role, text, message_id = null) => ({ id, role, message_id, content_json: JSON.stringify(text === null ? [{ type: 'toolResponse', id: 't', toolResult: { status: 'success', value: { content: [] } } }] : [{ type: 'text', text }]) });

test("the café brief's guidance steers #3w's Harbourline note; the card's Steer it now carries it", () => {
  const c = chooseNoteAction(HARBOURLINE, cafe);
  assert.deepEqual([c.source, c.action, c.match], ['guided', 'steer', 'harbourline pdf']);
  assert.deepEqual(planNoteSteps(HARBOURLINE, c), { steps: [{ kind: 'click', testid: 'note-steer-now' }], delivery: 'steer', retarget: false });
});

test('no guidance, or none matching: Leave it there (source default)', () => {
  const c = chooseNoteAction(HARBOURLINE, []);
  assert.deepEqual([c.source, c.action], ['default', 'leave']);
  assert.equal(planNoteSteps(HARBOURLINE, c).steps[0].testid, 'note-leave-there');
  assert.equal(chooseNoteAction({ ...HARBOURLINE, text: 'the scone is 3.50', to_query: 'billing', target: { name: 'Billing' } }, cafe).source, 'default');
});

test('match words are looked for in the text, the chat named and the chat resolved; the most specific wins', () => {
  const g = loadNoteGuidance({ notes: [{ match: 'jira', action: 'leave' }, { match: 'jira pdf', action: 'cancel' }, { match: 'assessment', action: 'steer' }] });
  assert.equal(chooseNoteAction(HARBOURLINE, g).action, 'cancel');
  assert.equal(chooseNoteAction({ text: 'x', to_query: 'the jira chat', target: null }, g).action, 'leave');
  assert.equal(chooseNoteAction({ text: 'x', to_query: 'that one', target: { name: 'Harbourline Assessment' } }, g).action, 'steer');
});

test('not-this-chat: Not this chat, pick the named chat (never the current one), then send; cancel is one click', () => {
  const g = loadNoteGuidance({ notes: { actions: [{ match: 'harbourline', action: 'not-this-chat', pick: 'harbourline readiness', then: 'steer' }] } });
  const plan = planNoteSteps(HARBOURLINE, chooseNoteAction(HARBOURLINE, g));
  assert.deepEqual(plan.steps.map((s) => s.testid ?? `pick:${s.words}:${s.exclude}`), ['note-not-this-chat', 'pick:harbourline readiness:20260928_47', 'note-steer-now']);
  assert.equal(plan.delivery, 'steer'); assert.equal(plan.retarget, true);
  const rows = [{ sessionId: '20260928_47', text: '"Harbourline Readiness" · ~/a · idle' }, { sessionId: '20260928_37', text: '"Harbourline Jira readiness report" · ~/b · not open in any window' }];
  assert.equal(pickCandidate(rows, 'harbourline readiness', '20260928_47').sessionId, '20260928_37');
  assert.equal(pickCandidate(rows, 'billing'), null);
  assert.deepEqual(planNoteSteps(HARBOURLINE, { action: 'cancel' }), { steps: [{ kind: 'click', testid: 'note-cancel' }], delivery: 'cancel', retarget: false });
});

test('a draft that resolved to no chat: picked when the guidance names one, else unactionable (never guessed)', () => {
  const amb = { ...HARBOURLINE, target: null, resolution: 'ambiguous' };
  assert.match(planNoteSteps(amb, chooseNoteAction(amb, [])).unactionable, /names no chat \(ambiguous/);
  const g = loadNoteGuidance({ notes: [{ match: 'harbourline', action: 'leave', pick: 'harbourline assessment' }] });
  assert.deepEqual(planNoteSteps(amb, chooseNoteAction(amb, g)).steps.map((s) => s.kind), ['pick', 'click']);
});

test('the guidance is validated loudly', () => {
  assert.throws(() => loadNoteGuidance({ notes: { actions: { match: 'x' } } }), /list/);
  assert.throws(() => loadNoteGuidance({ notes: [{ match: ' ', action: 'steer' }] }), /match/);
  assert.throws(() => loadNoteGuidance({ notes: [{ match: 'x', action: 'send' }] }), /action must be one of/);
  assert.throws(() => loadNoteGuidance({ notes: [{ match: 'x', action: 'not-this-chat' }] }), /needs "pick"/);
  assert.throws(() => loadNoteGuidance({ notes: [{ match: 'x', action: 'leave', then: 'cancel' }] }), /then/);
  assert.deepEqual(loadNoteGuidance({ notes: { why_this_domain: 'prose only' } }), [], 'an author-notes object without actions is no guidance');
  assert.deepEqual(loadNoteGuidance(null), []);
});

test('the store: absent = no notes (said so), malformed = a throw; #3w A holds one pending draft', () => {
  assert.deepEqual(parseNotesState(''), { absent: true, drafts: [], inbox: [] });
  assert.throws(() => parseNotesState('{"drafts":{}}'));
  assert.equal(parseNotesState(STATE_A).drafts[0].status, 'draft');
});

test("B's transcript: the note's message by its id, else by its framing; the reply runs through tool results", () => {
  const inbox = { id: HARBOURLINE.id, text: HARBOURLINE.text };
  const rows = [
    msg(10, 'user', 'earlier message'), msg(11, 'assistant', 'earlier reply'),
    msg(12, 'user', framing(HARBOURLINE), `crossnote_${HARBOURLINE.id}`),
    msg(13, 'assistant', 'Let me read that file.'), msg(14, 'user', null),
    msg(15, 'assistant', 'Noted: HOW-TO-PDF.md in the bakery work folder has the LibreOffice headless → pdfinfo/pdftotext method, and the 15-column table warning matches what happened to our readiness report.'),
    msg(16, 'user', 'the person asks something else'), msg(17, 'assistant', 'unrelated'),
  ];
  assert.equal(findNoteMessage(rows, inbox).how, 'id');
  assert.equal(findNoteMessage(rows.map((r) => ({ ...r, message_id: null })), inbox).how, 'text', 'stored under another id: found by its words');
  assert.equal(findNoteMessage(rows.filter((r) => r.id !== 12), inbox), null);
  const reply = replyAfter(rows, 12);
  assert.match(reply, /^Let me read that file\.\nNoted: HOW-TO-PDF/); assert.doesNotMatch(reply, /unrelated/);
});

const REPLY = 'Noted: HOW-TO-PDF.md in the bakery work folder has the LibreOffice headless → pdfinfo/pdftotext method, and the 15-column table warning matches what happened to our readiness report.';
const delivered = { id: HARBOURLINE.id, text: HARBOURLINE.text, status: 'delivered', delivered_how: 'own_turn', offer_when_idle: false };
const line = (outcome) => ({ present: true, outcome, text: `Note sent to "Harbourline Jira Migration Assessment" · ${LINE_WORDS[outcome]}${outcome === 'delivered' ? ' at 07:50' : ''}` });

test('judge steer: delivered, the message in B, a reply that uses it, A reads "read in its turn" → no fails', () => {
  const v = judgeNote({ delivery: 'steer', draftStatus: 'sent', inbox: delivered, noteMsg: 'id', reply: REPLY, noteText: HARBOURLINE.text, card: line('delivered') });
  assert.deepEqual(v, { fails: [], undelivered: false });
});

test('judge steer: still waiting (no window took it) is UNDELIVERED; a reply that ignores it and a stale card fail too', () => {
  const waiting = { ...delivered, status: 'waiting', offer_when_idle: true };
  const v = judgeNote({ delivery: 'steer', draftStatus: 'sent', inbox: waiting, noteMsg: '', reply: '', noteText: HARBOURLINE.text, card: line('waiting') });
  assert.equal(v.undelivered, true); assert.match(v.fails[0], /still waiting there \(offered to a window/);
  const ignored = judgeNote({ delivery: 'steer', draftStatus: 'sent', inbox: delivered, noteMsg: 'id', reply: 'OK. Anything else?', noteText: HARBOURLINE.text, card: line('waiting') });
  assert.equal(ignored.undelivered, false);
  assert.ok(ignored.fails.some((f) => /does not refer to the note/.test(f)), ignored.fails.join(' | '));
  assert.ok(ignored.fails.some((f) => /A's line says waiting/.test(f)), ignored.fails.join(' | '));
  assert.ok(judgeNote({ delivery: 'steer', draftStatus: 'sent', inbox: null, card: { present: false } }).undelivered);
});

test('judge leave: waiting in B with "waiting there" on A passes; judge cancel: nothing sent, nothing left on A', () => {
  const waiting = { ...delivered, status: 'waiting' };
  assert.deepEqual(judgeNote({ delivery: 'leave', draftStatus: 'sent', inbox: waiting, noteText: HARBOURLINE.text, card: line('waiting') }).fails, []);
  assert.match(judgeNote({ delivery: 'leave', draftStatus: 'sent', inbox: waiting, card: { present: false } }).fails[0], /no line/);
  assert.deepEqual(judgeNote({ delivery: 'cancel', draftStatus: 'cancelled', inbox: null, card: { present: false } }).fails, []);
  assert.equal(judgeNote({ delivery: 'cancel', draftStatus: 'draft', inbox: null, card: { present: true, outcome: 'draft' } }).fails.length, 2);
  assert.equal(outcomeOf(null), 'gone');
});

test("round end: #3w's pending Harbourline draft is a FAIL line; a failed proof is one too; an unreadable store says so", () => {
  const lines = noteFails({ ok: true, state: parseNotesState(STATE_A) }, []);
  assert.equal(lines.length, 1);
  assert.match(lines[0], /^FAIL: pending note nt_0ed0d09723c34e249bc7b4b5c233d690 to "Harbourline Jira Migration Assessment" still a draft: On a new chat/);
  const rec = { noteId: 'nt_1', action: 'steer', targetName: 'B', undelivered: true, fails: ['Steer it now, and the note is still waiting there'] };
  assert.deepEqual(noteFails({ ok: true, state: { drafts: [], inbox: [] } }, [rec]), ['FAIL: note nt_1 (steer to "B") undelivered: Steer it now, and the note is still waiting there']);
  assert.match(noteFails({ ok: false, error: 'locked' })[0], /could not be read: locked/);
});

test('notes.tsv: one row per note, as many cells as the header, tabs and newlines flattened', () => {
  const row = noteRow({ turn: 20, noteId: 'nt_1', text: 'a\tb\nc', reply: 'r', card: line('waiting'), fails: [] });
  assert.equal(row.split('\t').length, NOTES_TSV_HEADER.split('\t').length);
  assert.ok(row.endsWith('\n')); assert.ok(row.includes('a b c')); assert.ok(row.includes('\tok\t'));
});
