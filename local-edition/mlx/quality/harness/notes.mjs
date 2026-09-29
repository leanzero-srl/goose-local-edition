// notes.mjs — the E2E acts on every note goose drafts to ANOTHER chat (`send_note`, Q-358) and proves what
// became of it. Owner, 2026-09-29: "reply to and test outcomes for when something need you or the note to
// another chat … this stuff won't be remaining stagnant". E2E #3w's turn 20 drafted a note to "Harbourline
// Jira Migration Assessment" (20260928_47) and it stayed pinned, undelivered: the gap this closes.
//
// The product (DESIGN-Q358-TRANSCRIPTS §2–3, crates/goose/src/chat_notes.rs): `send_note` SENDS NOTHING — it
// pins a draft card in chat A (`chat_notes.v0`.drafts in A's extension_data) with [Steer it now] [Leave it
// there] [Not this chat] [Cancel]. Only the person's click moves it: the note lands in B's inbox
// (`chat_notes.v0`.inbox in B's extension_data). "Steer it now" steers it into B's running turn, or — B idle —
// goosed offers it as B's own turn to the windows SHOWING B; no window shows B → it waits until one does.
// A's card becomes one line: "… · waiting there" → "… · read in its turn at 14:05" / "… · dismissed there".
//
// Pure half (unit-tested in notes.test.mjs): the brief's guidance, the action and its clicks, and the verdict
// from what the store and the card say. IO half: sessions.db READ-ONLY (A's drafts, B's inbox, B's messages).
// DOM half: A's draft tray in the chat on screen (never navigates that window), and — only for "Steer it now"
// on a B no window shows — B opened in a NEW window, exactly what the session list's "Open in new window"
// does (SessionListView handleOpenInNewWindow: createChatWindow({dir, resumeSessionId, viewType:'pair'})),
// closed again once B's turn has ended.
//
// GUIDANCE (optional) in the brief. `notes` is already each brief's author-notes object, so the list lives
// at `notes.actions` (a brief whose `notes` IS a list is read as the list):
//   "notes": { …, "actions": [ { "match": "<words>", "action": "steer" | "leave" | "not-this-chat" | "cancel",
//                                "pick": "<words of the chat to pick>", "then": "steer" | "leave" } ] }
// `match` words are looked for in the note's text + the chat it names + the chat it resolved to; the most
// specific entry wins (needsyou.mjs matchGuidance). No match → "leave" (source `default`). `pick` is required
// for not-this-chat, and also serves a draft that resolved to no chat (ambiguous / no match); `then` is what
// is sent once the chat is picked (default leave).
import { execFileSync } from 'node:child_process';
import { SESSIONS_DB, matchGuidance, norm, words, textUse, textUseCell } from './needsyou.mjs';

export const NOTE_ACTIONS = ['steer', 'leave', 'not-this-chat', 'cancel'];
const SENDS = ['steer', 'leave'];
export const BUTTON = { steer: 'note-steer-now', leave: 'note-leave-there', 'not-this-chat': 'note-not-this-chat', cancel: 'note-cancel' };
export const MESSAGE_ID_PREFIX = 'crossnote_'; // chat_notes.rs MESSAGE_ID_PREFIX / noteIds.ts
const FRAMING_HEAD = 'note from your other chat'; // chat_notes.rs framing(): what B's model reads, first words
// What A's sent line says per outcome (noteWords.ts sentWaiting / sentRead / sentDismissed). Steering and
// with-next-message draw the waiting words (NoteDraftCard NoteSentLine).
export const LINE_WORDS = { waiting: 'waiting there', steering: 'waiting there', with_next_message: 'waiting there', delivered: 'read in its turn', dismissed: 'dismissed there' };

// ---------------------------------------------------------------- pure: guidance, action, clicks

/** The brief's note guidance, validated once — a malformed entry is a loud error, never skipped. */
export function loadNoteGuidance(brief) {
  const raw = Array.isArray(brief?.notes) ? brief.notes : brief?.notes?.actions ?? [];
  if (!Array.isArray(raw)) throw new Error('brief.notes.actions must be a list of {match, action}');
  return raw.map((g, i) => {
    const at = `brief.notes.actions[${i}]`;
    if (typeof g?.match !== 'string' || !words(g.match).length) throw new Error(`${at}.match must be non-empty words`);
    if (!NOTE_ACTIONS.includes(g.action)) throw new Error(`${at}.action must be one of ${NOTE_ACTIONS.join(' | ')}, not ${JSON.stringify(g.action)}`);
    if (g.pick !== undefined && (typeof g.pick !== 'string' || !words(g.pick).length)) throw new Error(`${at}.pick must be non-empty words when present`);
    if (g.action === 'not-this-chat' && !g.pick) throw new Error(`${at}: not-this-chat needs "pick" — the words of the chat the person picks instead`);
    if (g.then !== undefined && !SENDS.includes(g.then)) throw new Error(`${at}.then must be steer | leave`);
    return { match: g.match, action: g.action, pick: g.pick ?? '', then: g.then ?? 'leave' };
  });
}

/** What the person does with this draft. draft = the store's DraftNote (to_query, text, target?). */
export function chooseNoteAction(draft, guidance = []) {
  const haystack = `${draft?.text ?? ''} ${draft?.to_query ?? ''} ${draft?.target?.name ?? ''}`;
  const m = matchGuidance(haystack, guidance);
  if (m) return { ...m.entry, source: 'guided' };
  return { match: '', action: 'leave', pick: '', then: 'leave', source: 'default' };
}

/**
 * The clicks on A's draft card, in order: { steps, delivery: 'steer'|'leave'|'cancel', retarget } or
 * { unactionable: reason } when no click can send it (a draft that resolved to no chat, and no `pick`).
 * Steps: { kind: 'click', testid } · { kind: 'pick', words, exclude } (the picker's filter, then its row).
 */
export function planNoteSteps(draft, choice) {
  const click = (a) => ({ kind: 'click', testid: BUTTON[a] });
  if (choice.action === 'cancel') return { steps: [click('cancel')], delivery: 'cancel', retarget: false };
  const send = choice.action === 'not-this-chat' ? choice.then : choice.action;
  if (draft?.target && choice.action !== 'not-this-chat') return { steps: [click(send)], delivery: send, retarget: false };
  if (draft?.target) return { steps: [click('not-this-chat'), { kind: 'pick', words: choice.pick, exclude: draft.target.session_id }, click(send)], delivery: send, retarget: true };
  if (!choice.pick) return { unactionable: `the draft names no chat (${draft?.resolution ?? 'unresolved'}: "${draft?.to_query ?? ''}") and the guidance names none to pick` };
  return { steps: [{ kind: 'pick', words: choice.pick, exclude: '' }, click(send)], delivery: send, retarget: true };
}

/** The picker row the person clicks: the first (most recent) whose words hold every pick word. */
export function pickCandidate(rows, pick, exclude = '') {
  const want = words(pick);
  return rows.find((r) => r.sessionId !== exclude && want.every((w) => new Set(words(r.text)).has(w))) ?? null;
}

// ---------------------------------------------------------------- pure: the store and B's transcript

/** `chat_notes.v0` as sqlite prints it: absent key = no notes (said so), malformed = a throw. */
export function parseNotesState(out) {
  const s = String(out ?? '').trim();
  if (!s) return { absent: true, drafts: [], inbox: [] };
  const v = JSON.parse(s);
  if (!Array.isArray(v?.drafts) || !Array.isArray(v?.inbox)) throw new Error(`chat_notes.v0 has no drafts/inbox lists: ${s.slice(0, 120)}`);
  return { absent: false, drafts: v.drafts, inbox: v.inbox };
}

const parts = (row) => { try { return JSON.parse(row.content_json); } catch { return []; } };
export const textOf = (row) => parts(row).filter((c) => c?.type === 'text').map((c) => c.text ?? '').join('\n');

/** The note's message in B: by its id (crossnote_<id>), else by its framing words. → { row, how } | null. */
export function findNoteMessage(rows, note) {
  const byId = rows.find((r) => r.role === 'user' && r.message_id === `${MESSAGE_ID_PREFIX}${note.id}`);
  if (byId) return { row: byId, how: 'id' };
  const head = norm(note.text).slice(0, 80);
  const byText = rows.find((r) => r.role === 'user' && norm(textOf(r)).startsWith(FRAMING_HEAD) && norm(textOf(r)).includes(head));
  return byText ? { row: byText, how: 'text' } : null;
}

/** goose's reply to the message at rowid `after`: every assistant text until the next message with text from
 * the person's side (a tool result carries no text part, so a tool loop stays one reply). */
export function replyAfter(rows, after) {
  const out = [];
  for (const r of rows.filter((x) => x.id > after).sort((a, b) => a.id - b.id)) {
    if (r.role === 'user' && textOf(r).trim()) break;
    if (r.role === 'assistant') { const t = textOf(r).trim(); if (t) out.push(t); }
  }
  return out.join('\n');
}

export const outcomeOf = (inboxNote) => (inboxNote ? inboxNote.status : 'gone');

/**
 * The verdict for one acted-on note, from what was observed — never from what was clicked.
 * rec: { delivery, draftStatus, inbox (B's InboxNote | null), noteMsg ('id'|'text'|''), reply, noteText,
 *        card: { present, outcome, text } }  → { fails: [..], undelivered }
 */
export function judgeNote(rec) {
  const fails = [];
  const card = rec.card ?? { present: false };
  if (rec.delivery === 'cancel') {
    if (rec.draftStatus !== 'cancelled') fails.push(`Cancel left the draft ${rec.draftStatus || 'missing'}, not cancelled`);
    if (rec.inbox) fails.push(`Cancel, yet the note is in the other chat's inbox (${rec.inbox.status})`);
    if (card.present) fails.push(`Cancel, yet A still shows the note (${card.outcome || 'draft card'})`);
    return { fails, undelivered: false };
  }
  if (rec.draftStatus !== 'sent') fails.push(`the draft is ${rec.draftStatus || 'missing'} after "${rec.delivery}", not sent`);
  if (!rec.inbox) fails.push("the note never reached the other chat's inbox");
  let undelivered = !rec.inbox;
  if (rec.delivery === 'steer' && rec.inbox) {
    if (rec.inbox.status !== 'delivered') { undelivered = true; fails.push(`Steer it now, and the note is still ${rec.inbox.status} there${rec.inbox.offer_when_idle ? ' (offered to a window showing that chat; none took it)' : ''}`); }
    else {
      if (!rec.noteMsg) fails.push("delivered, but the other chat's transcript holds no message with the note");
      const use = norm(rec.reply) ? textUse(rec.noteText, rec.reply).verdict : 'none';
      if (use === 'no' || use === 'none') fails.push(`the other chat's reply does not refer to the note (${textUseCell(rec.noteText, rec.reply)})`);
    }
  }
  if (rec.delivery === 'leave' && rec.inbox && !['waiting', 'delivered', 'dismissed', 'with_next_message', 'steering'].includes(rec.inbox.status)) fails.push(`Leave it there, and the note is ${rec.inbox.status}`);
  if (rec.inbox) {
    const want = outcomeOf(rec.inbox);
    if (!card.present) fails.push(`A shows no line for the sent note (expected "${LINE_WORDS[want] ?? want}")`);
    else if (card.outcome !== want || !norm(card.text).includes(LINE_WORDS[want] ?? '')) fails.push(`A's line says ${card.outcome} "${card.text}", the other chat says ${want}`);
  }
  return { fails, undelivered };
}

/** Round end: a draft never acted on, and every failed proof, as `FAIL:` lines; leave-notes waiting by the
 * person's choice are listed apart (proven, not failed). */
export function noteFails(stateA, recs = []) {
  const lines = [];
  if (stateA && !stateA.ok) lines.push(`FAIL: notes unknown — this chat's notes could not be read: ${stateA.error}`);
  for (const d of stateA?.state?.drafts ?? []) {
    if (d.status !== 'draft') continue;
    const r = recs.find((x) => x.noteId === d.id);
    lines.push(`FAIL: pending note ${d.id} to "${d.target?.name ?? d.to_query}" still a draft${r?.why ? ` (${r.why})` : ''}: ${String(d.text).replace(/\s+/g, ' ').slice(0, 100)}`);
  }
  for (const r of recs) for (const f of r.fails ?? []) lines.push(`FAIL: note ${r.noteId} (${r.action} to "${r.targetName}")${r.undelivered ? ' undelivered' : ''}: ${f}`);
  return [...new Set(lines)];
}

// ---------------------------------------------------------------- IO: sessions.db, read-only

const sql = (q, db) => execFileSync('sqlite3', ['-readonly', '-json', db, q], { encoding: 'utf8' });
const idOk = (id) => /^[\w.-]+$/.test(id ?? '');

/** A chat's `chat_notes.v0`. Unreadable says so; it is never "no notes". */
export function readNotes(sessionId, db = SESSIONS_DB) {
  if (!idOk(sessionId)) return { ok: false, error: `no session id (${sessionId})`, state: { drafts: [], inbox: [] } };
  try {
    const out = execFileSync('sqlite3', ['-readonly', db, `select json_extract(extension_data, '$."chat_notes.v0"') from sessions where id='${sessionId}'`], { encoding: 'utf8' });
    return { ok: true, state: parseNotesState(out) };
  } catch (e) { return { ok: false, error: String(e.message ?? e).split('\n')[0].slice(0, 200), state: { drafts: [], inbox: [] } }; }
}

export function readChatRow(sessionId, db = SESSIONS_DB) {
  if (!idOk(sessionId)) return null;
  try { return JSON.parse(sql(`select id, name, working_dir from sessions where id='${sessionId}'`, db) || '[]')[0] ?? null; } catch { return null; }
}

/** B's messages from rowid `after` on (id, message_id, role, content_json). */
export function readMessages(sessionId, after = 0, db = SESSIONS_DB) {
  if (!idOk(sessionId)) throw new Error(`no session id (${sessionId})`);
  return JSON.parse(sql(`select id, message_id, role, content_json from messages where session_id='${sessionId}' and id > ${Number(after) || 0} order by id`, db) || '[]');
}

export const lastRowId = (sessionId, db = SESSIONS_DB) => {
  try { return JSON.parse(sql(`select coalesce(max(id), 0) as m from messages where session_id='${sessionId}'`, db) || '[{"m":0}]')[0].m; } catch { return 0; }
};

// ---------------------------------------------------------------- DOM

/** The draft tray of the VISIBLE chat: draft cards and sent lines. Reads only. */
export const readNoteTray = (p) => p.evaluate(() => {
  const tx = (e) => (e?.textContent ?? '').replace(/\s+/g, ' ').trim();
  const trays = [...document.querySelectorAll('[data-testid=note-draft-tray]')].filter((t) => t.offsetParent);
  return {
    drafts: trays.flatMap((t) => [...t.querySelectorAll('[data-testid=note-draft-card]')].map((c) => ({
      id: c.dataset.noteId,
      target: tx(c.querySelector('[data-testid=note-target]')),
      live: c.querySelector('[data-testid=note-target-live]')?.dataset.live ?? '',
      text: c.querySelector('[data-testid=note-text]')?.value ?? '',
      picker: !!c.querySelector('[data-testid=note-picker]'),
      candidates: [...c.querySelectorAll('[data-testid=note-candidate]')].map((r) => ({ sessionId: r.dataset.sessionId, text: tx(r) })),
    }))),
    sent: trays.flatMap((t) => [...t.querySelectorAll('[data-testid=note-sent-line]')].map((l) => ({ id: l.dataset.noteId, outcome: l.dataset.outcome, text: tx(l) }))),
  };
});

const SETTLE_POLLS = 5; // ratio: 2-s polls, as needs-you's TRAY_SETTLE_POLLS — bounds a UI refresh, never model work
const WINDOW_POLLS = 15; // ratio: 1-s polls for one UI navigation (a window loading its chat), as r1's OPEN_POLLS
const QUIET_POLLS = 3; // ratio: 2-s polls with no Stop button: the turn has ended (r1's AWAY_IDLE_POLLS)
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));

export async function clickNoteSteps(p, noteId, steps) {
  const card = p.locator(`[data-testid=note-draft-tray]:visible [data-testid=note-draft-card][data-note-id="${noteId}"]`).first();
  const done = [];
  for (const s of steps) {
    if (s.kind === 'click') { await card.getByTestId(s.testid).click(); done.push(s.testid); continue; }
    const filter = card.getByTestId('note-picker-filter');
    if (await filter.count()) await filter.fill(s.words);
    let row = null;
    for (let i = 0; i <= SETTLE_POLLS && !row; i++) {
      const rows = await card.locator('[data-testid=note-candidate]').evaluateAll((es) => es.map((r) => ({ sessionId: r.dataset.sessionId, text: (r.textContent ?? '').replace(/\s+/g, ' ').trim() })));
      row = pickCandidate(rows, s.words, s.exclude);
      if (!row) await sleep(2000);
    }
    if (!row) throw new Error(`no chat in the picker holds every word of "${s.words}"`);
    await card.locator(`[data-testid=note-candidate][data-session-id="${row.sessionId}"]`).click();
    done.push(`pick ${row.sessionId}`);
    // The retarget round-trips goosed; the card redraws with the new target before the send is enabled.
    await card.getByTestId('note-target').waitFor({ state: 'visible', timeout: SETTLE_POLLS * 2000 }).catch(() => {});
  }
  return done.join(' → ');
}

/** B in a new window, the session list's "Open in new window". Returns its CDP page. */
export async function openChatWindow(browser, fromPage, chat) {
  const before = new Set(browser.contexts()[0].pages());
  await fromPage.evaluate(({ dir, id }) => window.electron.createChatWindow({ dir, resumeSessionId: id, viewType: 'pair' }), { dir: chat.working_dir, id: chat.id });
  for (let i = 0; i < WINDOW_POLLS; i++) {
    await sleep(1000);
    const page = browser.contexts()[0].pages().find((pg) => !before.has(pg) && pg.url().includes(`resumeSessionId=${chat.id}`));
    if (page) { await page.waitForLoadState('domcontentloaded').catch(() => {}); return page; }
  }
  throw new Error(`"Open in new window" for ${chat.id} drew no window within ${WINDOW_POLLS} s`);
}

export const closeChatWindow = (page) => page.evaluate(() => window.electron.closeWindow()).catch(() => {});

const inboxNote = (targetId, noteId) => { const r = readNotes(targetId); return r.ok ? r.state.inbox.find((n) => n.id === noteId) ?? null : null; };
const draftNote = (sessionA, noteId) => readNotes(sessionA).state.drafts.find((d) => d.id === noteId) ?? null;

/**
 * Act on one draft in A (the chat on `ctx.page`) and prove the outcome. ctx = { browser, page, sessionA, turn,
 * hangSecs(): number|null (bounds B's turn like r1's hang rule; null = no bound, progress decides), note(line),
 * choice? (an operator's explicit action, deliver-pending --action; else the brief's guidance decides) }.
 * Returns the notes.tsv record.
 */
export async function actOnNote(ctx, draft, guidance) {
  const { page: p } = ctx;
  const choice = ctx.choice ?? chooseNoteAction(draft, guidance);
  const plan = planNoteSteps(draft, choice);
  // The card is pinned by the tool's platform event; give the tray its redraw before calling it missing.
  let before = null;
  for (let i = 0; i <= SETTLE_POLLS && !before; i++) { before = (await readNoteTray(p)).drafts.find((c) => c.id === draft.id); if (!before) await sleep(2000); }
  const rec = {
    turn: ctx.turn, noteId: draft.id, to: draft.to_query, targetId: draft.target?.session_id ?? '', targetName: draft.target?.name ?? '',
    targetLive: before?.live ?? '', text: draft.text, action: choice.action, delivery: plan.delivery ?? '', source: choice.source, match: choice.match,
    clicks: '', actedAt: '', draftStatus: '', inboxStatus: '', deliveredHow: '', deliveredAt: '', windowOpened: 'n', noteMsg: '', markerInB: '',
    reply: '', replyUses: '', card: { present: false }, fails: [], undelivered: false, why: '',
  };
  if (plan.unactionable) { rec.why = plan.unactionable; rec.fails = [plan.unactionable]; rec.undelivered = true; return rec; }
  if (!before) { rec.why = 'the draft card is not in the chat on screen'; rec.fails = [rec.why]; rec.undelivered = true; return rec; }
  try { rec.clicks = await clickNoteSteps(p, draft.id, plan.steps); } catch (e) {
    rec.why = `the card refused the clicks: ${String(e.message ?? e).slice(0, 160)}`; rec.fails = [rec.why]; rec.undelivered = true; return rec;
  }
  rec.actedAt = new Date().toISOString();
  ctx.note?.(`NOTE acted ${draft.id} ${choice.action}${plan.retarget ? ` → ${plan.delivery}` : ''} (${choice.source}${choice.match ? ` "${choice.match}"` : ''}) via ${rec.clicks}`);
  // The target as the store now has it (a pick retargets the draft).
  let d = draftNote(ctx.sessionA, draft.id);
  for (let i = 0; i < SETTLE_POLLS && d?.status === 'draft'; i++) { await sleep(2000); d = draftNote(ctx.sessionA, draft.id); }
  rec.draftStatus = d?.status ?? 'missing';
  rec.targetId = d?.target?.session_id ?? rec.targetId; rec.targetName = d?.target?.name ?? rec.targetName;
  let inbox = null;
  if (plan.delivery !== 'cancel' && rec.targetId) {
    const startRow = lastRowId(rec.targetId);
    for (let i = 0; i <= SETTLE_POLLS && !inbox; i++) { inbox = inboxNote(rec.targetId, draft.id); if (!inbox) await sleep(2000); }
    let bPage = null;
    if (inbox && plan.delivery === 'steer') {
      // B running: it drains between tool calls (or goes back to waiting when that turn ends first).
      const t0 = Date.now();
      while (inbox?.status === 'steering' && !(ctx.hangSecs?.() && (Date.now() - t0) / 1000 > ctx.hangSecs())) { await sleep(2000); inbox = inboxNote(rec.targetId, draft.id); }
      // B idle: offered to the windows showing B. None takes it → a person opens B, which is when it runs.
      for (let i = 0; i < SETTLE_POLLS && inbox?.status === 'waiting'; i++) { await sleep(2000); inbox = inboxNote(rec.targetId, draft.id); }
      if (inbox?.status === 'waiting' && inbox.offer_when_idle) {
        const chat = readChatRow(rec.targetId);
        try {
          bPage = await openChatWindow(ctx.browser, p, chat); rec.windowOpened = 'y';
          ctx.note?.(`NOTE opened ${rec.targetId} in a new window (Open in new window) for ${draft.id}`);
        } catch (e) { rec.windowOpened = `failed: ${String(e.message ?? e).slice(0, 120)}`; }
        for (let i = 0; bPage && i < WINDOW_POLLS && inbox?.status === 'waiting'; i++) { await sleep(1000); inbox = inboxNote(rec.targetId, draft.id); }
      }
      if (inbox?.status === 'delivered') await awaitTurnEnd(ctx, rec.targetId, draft.id, bPage, startRow);
      if (bPage) rec.markerInB = (await bPage.locator(`[data-testid=note-marker][data-message-id="${MESSAGE_ID_PREFIX}${draft.id}"]`).count().catch(() => 0)) ? 'y' : 'n';
      inbox = inboxNote(rec.targetId, draft.id) ?? inbox;
      if (bPage) { await closeChatWindow(bPage); ctx.note?.(`NOTE closed the window of ${rec.targetId}`); }
    }
    if (inbox) {
      rec.inboxStatus = `${inbox.status}${inbox.offer_when_idle ? ' (offered when idle)' : ''}`;
      rec.deliveredHow = inbox.delivered_how ?? ''; rec.deliveredAt = inbox.delivered_at ?? '';
      if (inbox.status === 'delivered') {
        const rows = readMessages(rec.targetId, startRow);
        const m = findNoteMessage(rows, { id: draft.id, text: inbox.text });
        rec.noteMsg = m?.how ?? '';
        rec.reply = m ? replyAfter(rows, m.row.id) : '';
        rec.replyUses = textUseCell(inbox.text, rec.reply);
      }
    } else rec.inboxStatus = 'n (not in the inbox)';
  }
  // A's card: the line for a sent note must say what B's inbox says; a cancelled draft leaves nothing.
  const want = plan.delivery === 'cancel' ? null : outcomeOf(inbox);
  for (let i = 0; i <= SETTLE_POLLS; i++) {
    const tray = await readNoteTray(p);
    const line = tray.sent.find((l) => l.id === draft.id); const still = tray.drafts.some((c) => c.id === draft.id);
    rec.card = line ? { present: true, outcome: line.outcome, text: line.text } : still ? { present: true, outcome: 'draft', text: 'the draft card is still there' } : { present: false };
    if (want === null ? !rec.card.present : rec.card.outcome === want) break;
    await sleep(2000);
  }
  const verdict = judgeNote({ delivery: plan.delivery, draftStatus: rec.draftStatus, inbox, noteMsg: rec.noteMsg, reply: rec.reply, noteText: inbox?.text ?? draft.text, card: rec.card });
  rec.fails = verdict.fails; rec.undelivered = verdict.undelivered;
  return rec;
}

/** B's turn with the note: until B shows no Stop button for QUIET_POLLS polls and has written a reply (B's
 * window), or — no window — until B's rows stop growing for QUIET_POLLS polls after a reply. */
async function awaitTurnEnd(ctx, targetId, noteId, bPage, startRow) {
  const t0 = Date.now(); let quiet = 0; let prevRows = -1;
  while (quiet < QUIET_POLLS) {
    if (ctx.hangSecs?.() && (Date.now() - t0) / 1000 > ctx.hangSecs()) { ctx.note?.(`NOTE ${noteId}: ${targetId}'s turn passed the hang bound; read as it stands`); return; }
    await sleep(2000);
    const rows = readMessages(targetId, startRow);
    const m = findNoteMessage(rows, { id: noteId, text: inboxNote(targetId, noteId)?.text ?? '' });
    const replied = !!(m && replyAfter(rows, m.row.id));
    const busy = bPage ? await bPage.evaluate(() => !!document.querySelector('button[aria-label="Stop"]')).catch(() => false) : rows.length !== prevRows;
    prevRows = rows.length;
    quiet = replied && !busy ? quiet + 1 : 0;
  }
}

// ---------------------------------------------------------------- notes.tsv

export const NOTES_TSV_HEADER = 'turn\tnote\tto\ttarget\ttargetName\ttargetLive\taction\tdelivery\tsource\tmatch\tclicks\tactedAt\tdraftDb\tinboxDb\tdeliveredHow\tdeliveredAt\twindowOpened\tnoteMsgInB\tmarkerInB\treplyUses\tcardLine\tverdict\ttext\treply\n';
const cell = (v) => String(v ?? '').replace(/[\t\r\n]+/g, ' ');
export const noteRow = (r) => [r.turn, r.noteId, r.to, r.targetId, r.targetName, r.targetLive, r.action, r.delivery, r.source, r.match, r.clicks, r.actedAt,
  r.draftStatus, r.inboxStatus, r.deliveredHow, r.deliveredAt, r.windowOpened, r.noteMsg, r.markerInB, r.replyUses,
  r.card?.present ? `${r.card.outcome}: ${r.card.text}` : 'none', r.fails?.length ? `FAIL: ${r.fails.join(' | ')}` : 'ok', String(r.text ?? '').slice(0, 300), String(r.reply ?? '').slice(0, 300)].map(cell).join('\t') + '\n';
