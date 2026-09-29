// R1 — a long AGENTIC goose session on whatever engine serves chat (the split, for R1), through the product.
// usage: node r1.mjs <dir> [--turns N] [--brief <brief.json>]   (a goose-task-author brief; {WORK} → <dir>/work)
// One new chat, opened IN <dir>/work (Projects + → New session here; sessions.db working_dir checked before turn 1,
// recorded in <dir>/round.json — Q-390); turn after turn of real tool work inside <dir>/work (files, shell, tests), so the context
// climbs from goose's ~49k-token prompt toward compaction. Per turn one TSV row: start, end, seconds, how the
// turn ended (done / notice / stall), the chip, the context counter, the notice text if any.
// A turn with no change on screen for STALL_FACTOR x the running median turn length (never below the first
// turn's own length) is logged STALL with a screenshot and the soak goes on waiting; HANG_FACTOR x ends the soak —
// a hang is the finding, and the driver never cancels, retries or edits the turn itself.
// NEEDS-YOU (Q-376): every card goose raises in this chat is answered like a person would, through the card's
// own controls, from the brief's `needsYou` guidance (needsyou.mjs) — mid-turn (the answer must show Queued,
// Q-341) and at each turn end (the answer turn runs before the next brief turn). One needsyou.tsv row per card:
// answer + source, queued, delivered, cleared, siblings still open (Q-344), the model's next words. A card
// still there after its answer turn, or a sibling closed by a card answer, is a LIVE finding that ends the run
// at the turn boundary. The OUTCOME is recorded too (owner 2026-09-29): replyUses = does goose's answer-turn
// reply quote or use the answer (needsyou.mjs textUse, text overlap — a reader still reads the reply column).
// NOTES (Q-358, notes.mjs): every note goose drafts to ANOTHER chat (`send_note`) is acted on at the turn
// boundary through its own card — Steer it now / Leave it there / Not this chat / Cancel, from the brief's
// `notes.actions` guidance, default Leave — and its outcome PROVEN from sessions.db (read-only): in the other
// chat's inbox; for Steer, delivered, the note's message in that chat's transcript and a reply that refers to it
// (a Steer to a chat no window shows opens it in a NEW window — the session list's "Open in new window" — and
// closes it after; r1's own window never leaves its chat); A's line saying what the other chat says. One
// notes.tsv row per note; a failed proof is a LIVE finding (it does not stop the run).
// ROUND END (owner 2026-09-29: nothing stays pending): every card still open and every note not delivered as
// acted is a `FAIL: …` line in round.json `fails`, events.log and r1's stdout (runwatch prints them).
import { chromium } from '/Users/mihaiperdum/Projects/goose/ui/node_modules/playwright-core/index.mjs';
import { mainPage } from './mainpage.mjs';
import { mkdirSync, writeFileSync, appendFileSync, readFileSync, readdirSync, statSync, existsSync } from 'node:fs';
import { liveCheck } from './livecheck.mjs';
import { loadGuidance, chooseAnswer, planClick, isAnswerMessage, readDbItems, sessionIdOf, readTray, readChat, answerCard, norm, textUseCell, openCardFails } from './needsyou.mjs';
import { loadNoteGuidance, readNotes, actOnNote, noteFails, noteRow, NOTES_TSV_HEADER, readMessages, lastRowId } from './notes.mjs';
import { workDirOf, projectRowTestId, readWorkingDir, checkWorkingDir } from './workdir.mjs';
const dir = process.argv[2];
const turnsArg = process.argv.indexOf('--turns'); const maxTurnsArg = turnsArg > 0 ? Number(process.argv[turnsArg + 1]) : 0;
const work = workDirOf(dir); mkdirSync(work, { recursive: true });
const STALL_FACTOR = 5; // ratio: of the median turn length measured in this soak
const HANG_FACTOR = 15; // ratio: same; the soak.py hang rule of Step 1b used 10x a running median
const AWAY_IDLE_POLLS = 3; // three 2-s polls with nothing served: a gap between calls or the turn end, never mid-stream
const LIVE_EVERY = 30; // ratio: 2-s polls per live check (~a minute, livecheck's cadence since Q-147); the needs-you look rides it
const TRAY_SETTLE_POLLS = 5; // ratio: 2 x the tray's 5-s session-activity read (ACTIVITY_POLL_MS) in 2-s polls — bounds a UI refresh, never model work
const out = `${dir}/turns.tsv`;
writeFileSync(out, 'turn\tstart\tend\tsecs\tended\ttools\trecalled\tchip\tcounter\tnotice\n');
// What THIS turn added: the messages after the send, never the whole page (the smoke run matched an older
// session's notice). A failed turn is any goose notice or the engine's empty-response line.
const FAIL = /empty response|stopped answering|quit goose mid|No node can|Ran into this error|split across your Macs stopped|stopped making progress/;
const added = (n0) => p.evaluate((n0) => { const ms = [...document.querySelectorAll('.goose-message')].filter((m) => m.offsetParent).slice(n0).map((m) => m.innerText); const recalled = ms.join('\n').split('\n').filter((l) => /^recalled:/.test(l.trim())).join(' | '); const tools = [...document.querySelectorAll('.goose-message')].filter((m) => m.offsetParent).slice(n0).reduce((k, m) => k + m.querySelectorAll('[class*=tool i], details').length, 0); return { text: ms.join(' ').replace(/\s+/g, ' '), tools, recalled }; }, n0);
const briefArg = process.argv.indexOf('--brief');
const brief = briefArg > 0 ? JSON.parse(readFileSync(process.argv[briefArg + 1], 'utf8')) : null;
const guidance = loadGuidance(brief);
const noteGuidance = loadNoteGuidance(brief);
const builtin = [
  `Work only inside ${work}. Create a Python package "ledger" with ledger/__init__.py and ledger/core.py holding a class Ledger that records (date, account, amount, memo) entries in memory. Show me the files.`,
  `Add pytest tests in ${work}/tests/test_core.py for adding entries and for the balance of one account. Run them with python3 -m pytest -q from ${work} and show the output.`,
  `Add CSV import and export to Ledger (ledger/io.py), with tests. Run the whole test suite.`,
  `Add a monthly summary: total per account per month. Tests, then run the suite.`,
  `Read every file under ${work}/ledger back to me and point out anything inconsistent between modules.`,
  `Fix what you found. Run the suite.`,
  `Add a command-line interface ledger/cli.py (argparse): add, list, balance, summary, import, export. Test it through subprocess. Run the suite.`,
  `Generate a 2,000-row CSV of fake entries with a script in ${work}/tools/gen.py, import it through the CLI, and print the summary for the last three months.`,
  `Profile the summary on the 2,000 rows (python3 -m cProfile) and show the top 15 lines. Improve the slowest part without changing behaviour; run the suite.`,
  `Add currency support: every entry carries a currency; balances are per currency; CSV gains a column with a default of EUR for old files. Tests; run the suite.`,
  `Write ${work}/README.md documenting the CLI with an example session you actually run.`,
  `Review the whole package for error handling: bad dates, bad amounts, a missing file. Add tests for each and make them pass.`,
  `Add a JSON export next to CSV, with a round-trip test. Run the suite.`,
  `Add an 'undo last entry' command persisted in a small journal file. Tests; run the suite.`,
  `Read the test files back to me and list which behaviours are still untested. Then add the two most important missing tests and run the suite.`,
  `Refactor core.py so that storage is a separate class (memory or JSON file) chosen at startup. Keep every test passing.`,
  `Add budgets: a monthly limit per account, and the summary flags accounts over budget. Tests; run the suite.`,
  `Show me git-style diffs of what changed in core.py since the start (reconstruct from what you remember of the first version) and explain each change.`,
  `Run the full suite, then the CLI end to end on the 2,000-row file: import, summary, budget report, export JSON. Show all outputs.`,
  `Summarise this whole session: the package layout, every command, and the test count. Then continue: add a 'search' command filtering by memo text with a test.`,
];
const steps = brief ? brief.turns.map((t) => t.say.replaceAll('{WORK}', work)) : builtin;
// Every provider call goose makes is logged under an in-flight name (llm_request.<pid>.<uuid>.jsonl) and renamed
// into the rotating llm_request.<n>.jsonl when it ends, however it ends (Q-343) — so a numbered file IS a finished
// call. Its LAST line says how it ended: usage (input, output, cache_read), an error, or neither (`no-usage`: a
// cancelled stream — the Stop, a cut turn — or an answer that never sent usage). Every finished call is counted,
// one with no usage line too: the old reader skipped those forever, and #3p turn 8 made tool calls and logged none. Read on every poll, the view away included (ten slots rotate out within one many-tool
// turn), and once more when the turn ends.
const LOGS = `${process.env.HOME}/.local/state/goose/logs`;
const seenCalls = new Map();
const pollCalls = () => { const got = []; for (const f of readdirSync(LOGS)) { if (!/^llm_request\.\d+\.jsonl$/.test(f)) continue; let st; try { st = statSync(`${LOGS}/${f}`); } catch { continue; } /* goose rotates these between readdir and stat (E2E #4b died on ENOENT) */ const key = `${st.ino}:${st.mtimeMs}`; /* inode, not name: goose renames .0→.1→… on rotation and E2E #3i re-counted every old call under each new name */ if (seenCalls.has(key)) continue; let last; try { last = JSON.parse(readFileSync(`${LOGS}/${f}`, 'utf8').trimEnd().split('\n').at(-1)); } catch (e) { if (e.code === 'ENOENT') continue; last = null; } seenCalls.set(key, 1); got.push({ ...(last?.usage ?? {}), ended: !last ? 'unreadable' : last.usage ? 'usage' : last.error ? 'error' : 'no-usage' }); } return got; };
const logCalls = (turn) => { for (const u of pollCalls()) appendFileSync(`${dir}/calls.tsv`, [turn, u.input_tokens ?? '', u.output_tokens ?? '', u.cache_read_input_tokens ?? '', u.ended].join('\t') + '\n'); };
pollCalls();
writeFileSync(`${dir}/calls.tsv`, 'turn\tinput\toutput\tcache_read\tended\n');
const b = await chromium.connectOverCDP('http://127.0.0.1:9333');
const p = await mainPage(b);
await p.goto(p.url().split('#')[0] + '#/'); await p.waitForTimeout(2500);
const screen = () => p.evaluate(() => {
  const main = document.querySelector('main') ?? document.body;
  const chipEl = document.querySelector('[data-testid=model-chip-served]')?.closest('button');
  const counter = [...document.querySelectorAll('button, span, div')].map((e) => e.childElementCount === 0 ? e.textContent.trim() : '').find((t) => /^\d+(\.\d+)?k? \/ \d+k$/.test(t)) ?? '';
  const stop = !!document.querySelector('button[aria-label="Stop"]');
  return { len: main.innerText.length, chip: (chipEl?.innerText ?? '').replace(/\s+/g, ' '), counter, stop };
});
let chatUrl = ''; let title = ''; const liveSeen = new Set();
const lengths = []; const median = () => { const s = [...lengths].sort((a, b) => a - b); return s.length ? s[Math.floor(s.length / 2)] : null; };
const maxTurns = maxTurnsArg || (brief ? steps.length : 200);
const iso = () => new Date().toISOString();
const note = (line) => appendFileSync(`${dir}/events.log`, `${iso()} ${line}\n`);

// Q-390: the chat is opened in <dir>/work the way a person does it — Projects "+" registers the folder, its row's
// "New session here" starts the chat (workdir.mjs says why, and which step stands in for the native chooser) —
// and sessions.db must then say that folder, or the run stops before turn 1. <dir>/round.json keeps the proof.
const OPEN_POLLS = 15; // ratio: 1-s polls for one UI navigation (the sidebar redraw, the new chat's route) — never model work
const roundFile = `${dir}/round.json`;
const writeRound = (fields) => { let cur = {}; try { cur = JSON.parse(readFileSync(roundFile, 'utf8')); } catch {} writeFileSync(roundFile, JSON.stringify({ ...cur, ...fields }, null, 2) + '\n'); };
async function openChatInWork() {
  const fail = async (why) => {
    note(`WORKDIR_FAIL ${why}`); writeRound({ work, workingDirVerified: false, workingDirError: why });
    await p.screenshot({ path: `${dir}/workdir-fail.png` }).catch(() => {});
    console.error(`r1 STOPPED before turn 1 (Q-390): ${why}`); await b.close(); process.exit(2);
  };
  const before = sessionIdOf(p.url());
  const reg = await p.evaluate(async (dir) => {
    const was = await window.electron.listProjects();
    if (was.some((x) => x.path === dir)) return { added: false, listed: true };
    const projects = await window.electron.addProject(dir);
    const added = projects.filter((x) => !was.some((w) => w.path === x.path));
    // What chooseAndAddProject does with the chooser's answer (AppEvents.PROJECTS_CHANGED = 'projects-changed').
    window.dispatchEvent(new CustomEvent('projects-changed', { detail: { projects, added } }));
    return { added: added.length > 0, listed: projects.some((x) => x.path === dir) };
  }, work);
  if (!reg.listed) await fail(`the project registry refused ${work} (addProject: absolute, an existing directory, not a symlink)`);
  const fold = p.getByTestId('projects-fold');
  if (!(await fold.count())) await fail('no Projects section in the sidebar to start the chat from');
  if ((await fold.getAttribute('aria-expanded')) === 'false') { await fold.click(); note('WORKDIR expanded the folded Projects section'); }
  const row = p.getByTestId(projectRowTestId(work));
  if (!(await row.waitFor({ state: 'attached', timeout: OPEN_POLLS * 1000 }).then(() => true, () => false))) await fail(`the sidebar drew no project row for ${work}`);
  // The row's actions are `invisible group-hover:visible` (Layout/tree.tsx rowActionClass): hover the folder's own
  // toggle (the row div also holds its sessions, so its centre may sit over a session), then the + shows.
  await row.locator('button[aria-expanded]').first().hover();
  await row.locator('button[aria-label^="New session here"]').first().click();
  let sessionId = '';
  for (let i = 0; i < OPEN_POLLS && !sessionId; i++) { await p.waitForTimeout(1000); const s = sessionIdOf(p.url()); if (s && s !== before) sessionId = s; }
  if (!sessionId) await fail(`"New session here" on ${work} opened no chat (view ${p.url().split('#')[1]})`);
  const stored = readWorkingDir(sessionId); const v = checkWorkingDir(stored, work);
  writeRound({ work, sessionId, workingDir: stored.ok ? stored.workingDir : null, workingDirVerified: v.ok, workingDirSource: 'sessions.db', openedVia: 'Projects + (registry, the chooser\'s answer) → New session here', projectAddedByR1: reg.added, openedAt: iso() });
  note(`WORKDIR ${v.ok ? 'OK' : 'WRONG'} session ${sessionId} ${v.says}`);
  if (!v.ok) await fail(`chat ${sessionId}: ${v.says}`);
  await p.waitForTimeout(3000);
  return sessionId;
}
writeFileSync(roundFile, JSON.stringify({ dir, work, startedAt: iso() }, null, 2) + '\n'); // a rerun in the same dir starts a fresh proof
const openedSession = await openChatInWork();

// One turn, from a send already made until the chat stops working: the done / notice / stall / hang rules, the
// live check and the needs-you look every LIVE_EVERY polls, one turns.tsv row. `label` is the brief turn's
// number, or `<n>a<k>` for the k-th answer turn after brief turn n (Q-376). n0 = assistant messages before it.
// The user rows goose stored after `after` — does one carry this prompt's words?
function sentLanded(after, prompt) {
  const head = prompt.slice(0, 80);
  return readMessages(openedSession, after).some((row) => {
    if (row.role !== 'user') return false;
    try { return JSON.parse(row.content_json).some((c) => c.type === 'text' && c.text.slice(0, 80) === head); } catch { return false; }
  });
}

async function runTurn(label, n0, start) {
  let lastChange = Date.now(); let prev = null; let ended = ''; let stallLogged = false;
  await p.waitForTimeout(3000);
  let polls = 0; let away = false; let idleAway = 0;
  while (true) {
    if (!chatUrl && (Date.now() - start) > 8000) chatUrl = p.url();
    // Every ~minute: does every surface agree that this session is live? (livecheck.mjs, Q-147)
    if (polls++ % LIVE_EVERY === 0) {
      // Re-read every time: goose retitles a chat after its first answer, and a title read once at the start
      // ('New Session') made every later check report the live session as not listed (E2E #5b).
      title = await p.evaluate(() => document.querySelector('[data-testid=session-title-trigger]')?.innerText.trim() ?? '').catch(() => title);
      const lc = await liveCheck(p, { title }).catch((e) => ({ findings: [{ kind: 'PROBE_ERROR', says: String(e) }] }));
      appendFileSync(`${dir}/live.jsonl`, JSON.stringify({ turn: label, ...lc }) + '\n');
      for (const f of lc.findings) if (!liveSeen.has(f.kind)) { liveSeen.add(f.kind); await p.screenshot({ path: `${dir}/live-${label}-${f.kind}.png` }); note(`LIVE ${f.kind} ${JSON.stringify(f).slice(0, 300)}`); }
      // Q-376: a card open while the turn runs is answered now and must show Queued (Q-341).
      await nyTick(label, { mayStart: false }).catch((e) => note(`NEEDS_YOU_PROBE_ERROR ${String(e).slice(0, 300)}`));
    }
    // Someone else moved the view: nothing on screen is this turn's, so no done/stall verdict is taken from it.
    if (chatUrl && p.url() !== chatUrl) {
      if (!away) { away = true; idleAway = 0; note(`VIEW_AWAY ${p.url().split('#')[1]}`); }
      // Q-186: with the view away, the turn's end is invisible — #3i waited until a human navigated back.
      // When the engine serves nothing for a few polls in a row, goose is between calls or done: go back
      // to the chat, where the boundary can be read (a person on another page is only moved while idle).
      logCalls(label);
      const busy = await p.evaluate(async () => (await window.electron.mlxEngineActivity())?.stats?.numRunning ?? 0).catch(() => 1);
      idleAway = busy ? 0 : idleAway + 1;
      if (idleAway >= AWAY_IDLE_POLLS) {
        await p.goto(chatUrl); await p.waitForTimeout(4000);
        note(`VIEW_RETURNED engine idle ${idleAway} polls`);
      }
      lastChange = Date.now(); await p.waitForTimeout(2000); continue;
    }
    away = false;
    logCalls(label);
    const s = await screen();
    if (!prev || s.len !== prev.len || s.chip !== prev.chip) lastChange = Date.now();
    prev = s;
    const quiet = (Date.now() - lastChange) / 1000; const m = median() ?? (Date.now() - start) / 1000;
    if (!s.stop && (Date.now() - start) > 5000) { ended = 'done'; break; }
    if (!stallLogged && lengths.length && quiet > STALL_FACTOR * m) { stallLogged = true; await p.screenshot({ path: `${dir}/stall-${label}.png` }); note(`STALL turn ${label} quiet ${quiet.toFixed(0)}s median ${m.toFixed(0)}s chip ${s.chip}`); }
    if (lengths.length && quiet > HANG_FACTOR * m) { ended = 'hang'; await p.screenshot({ path: `${dir}/hang-${label}.png` }); break; }
    await p.waitForTimeout(2000);
  }
  const end = Date.now(); const secs = (end - start) / 1000; const s = await screen();
  logCalls(label);
  const a = await added(n0); const failed = a.text.match(FAIL);
  if (failed && ended === 'done') { ended = 'notice'; await p.screenshot({ path: `${dir}/notice-${label}.png` }); }
  if (ended === 'done') lengths.push(secs);
  appendFileSync(out, [label, new Date(start).toISOString(), new Date(end).toISOString(), secs.toFixed(1), ended, a.tools, a.recalled, s.chip, s.counter, failed ? a.text.slice(Math.max(0, failed.index - 60), failed.index + 140) : ''].join('\t') + '\n');
  return { ended, secs, text: a.text };
}

// ---------------------------------------------------------------- needs-you (Q-376)
// State per card r1 has met: `recs`, one per item id. A rec is `done` once its row is written (answered and its
// answer turn checked, or left unanswered with the reason).
const nyOut = `${dir}/needsyou.tsv`;
writeFileSync(nyOut, 'turn\titem\tquestion\toptions\tanswer\tsource\tmatch\tclick\tansweredAt\tqueued\tdeliveredAt\tdeliveredDb\tanswerFirst\tanswerTurn\tcleared\tdbStatus\tsiblings\tsiblingsOpen\treplyUses\treply\n');
const recs = []; let nyStop = ''; const nyNotShown = new Set();
const cell = (v) => String(v ?? '').replace(/[\t\r\n]+/g, ' ');
const nyLog = (turn, event, rec, findings = []) => appendFileSync(`${dir}/live.jsonl`, JSON.stringify({
  turn, at: iso(), kind: 'needs_you', event, item: rec?.itemId, question: rec?.question, options: rec?.options, optionsFrom: rec?.optionsFrom, answer: rec?.answer,
  source: rec?.source, click: rec?.click, answeredAt: rec?.answeredAt, queued: rec?.queued, deliveredAt: rec?.deliveredAt,
  answerFirst: rec?.answerFirst, cleared: rec?.cleared, siblings: rec?.siblingState, reply: rec?.reply, findings,
}) + '\n');
// A FINDING is a live.jsonl finding AND an events.log `LIVE` line (runwatch.sh wakes on those); `stops` ends the
// run at the next turn boundary with the reason, the way a STOP file does.
async function nyFinding(turn, rec, kind, says, truth, stops) {
  const f = { kind, surface: 'needs-you card', item: rec?.itemId ?? '', says, truth };
  nyLog(turn, 'finding', rec, [f]);
  note(`LIVE ${kind} ${JSON.stringify(f).slice(0, 300)}`);
  await p.screenshot({ path: `${dir}/needsyou-${turn}-${kind}.png` }).catch(() => {});
  if (stops && !nyStop) nyStop = `${kind} ${f.item}: ${says}`;
}
const nyRow = (rec) => {
  const sib = Object.entries(rec.siblingState);
  appendFileSync(nyOut, [rec.turn, rec.itemId, rec.question, rec.options.join(' | '), rec.answer, rec.source, rec.match, rec.click, rec.answeredAt, rec.queued,
    rec.deliveredAt, rec.deliveredDb, rec.answerFirst, rec.answerTurn, rec.cleared, rec.dbStatus, sib.map(([k, v]) => `${k}:${v}`).join(', '),
    sib.length ? (sib.every(([, v]) => v.startsWith('y')) ? 'y' : 'n') : '-', rec.replyUses, rec.reply].map(cell).join('\t') + '\n');
};
const onScreen = (tray, id) => tray.cards.some((c) => c.id === id);

function nyMarkDelivered(chat, db) {
  for (const r of recs) {
    if (!r.answeredAt || r.deliveredAt) continue;
    const i = chat.users.slice(r.usersAtAnswer).findIndex((t) => isAnswerMessage(t, r));
    // "The answer turn runs first" (Q-341): the first message the chat gained after the answer is the answer.
    if (i >= 0) { r.deliveredAt = iso(); r.answerFirst = i === 0 ? 'y' : `n (${i} message(s) before it)`; r.assistAtDelivery = chat.assistant; }
  }
  for (const r of recs) if (r.deliveredAt && !r.deliveredDb) r.deliveredDb = db.ok ? db.items.find((it) => it.id === r.itemId)?.answer_delivered_at ?? '' : '';
}

// Q-344: the questions that were open beside this one when it was answered must still be open (or answered by
// r1 itself) once goosed has read the answer — never superseded or dismissed by it.
async function nyCheckSiblings(turn, r, db, tray) {
  for (const sid of r.siblings) {
    const mine = recs.some((x) => x.itemId === sid && x.answeredAt);
    const status = db.ok ? (db.items.find((it) => it.id === sid)?.status ?? 'missing') : `store unreadable: ${db.error}`;
    const ok = db.ok ? status === 'open' || (mine && status === 'answered') : onScreen(tray, sid) || mine;
    r.siblingState[sid] = `${ok ? 'y' : 'n'} ${status}${onScreen(tray, sid) ? '' : ' (no card)'}`;
    if (!ok) await nyFinding(turn, r, 'NEEDS_YOU_SIBLING_CLOSED', `answering "${r.question.slice(0, 80)}" left sibling ${sid} ${status}`, 'Q-344: a card answer closes only its own question', true);
  }
  r.siblingsChecked = true;
}

// One look at the chat's cards: mark delivered answers, check siblings, answer what is open. Never navigates:
// with the view away from r1's chat it does nothing. mayStart = an answer may be sent at once (the turn has
// ended) — then at most one card is answered and true is returned, so the caller runs that answer turn.
// Mid-turn (mayStart false) every open card is answered and must queue.
async function nyTick(turn, { mayStart }) {
  if (!chatUrl || p.url() !== chatUrl) return false;
  const db = readDbItems(sessionIdOf(chatUrl));
  const tray = await readTray(p); const chat = await readChat(p);
  nyMarkDelivered(chat, db);
  for (const r of recs) if (r.deliveredAt && !r.siblingsChecked && (chat.assistant > r.assistAtDelivery || !tray.busy)) await nyCheckSiblings(turn, r, db, tray);
  for (const e of tray.elicitations) {
    if (recs.some((r) => r.itemId === e.id)) continue;
    const rec = nyRec(turn, { id: e.id, question: e.message, options: [] }, chat, 'dom');
    rec.source = 'unanswerable: an MCP elicitation form, not an ask_user card'; rec.done = true;
    nyRow(rec); nyLog(turn, 'unanswerable', rec); note(`NEEDS_YOU unanswerable ${e.id}: elicitation`);
  }
  const idle = !tray.busy;
  if (idle && !mayStart) return false;
  // An answer already given and not yet delivered goes first (the tray sends it when the turn ends).
  if (idle && recs.some((r) => r.answeredAt && !r.deliveredAt && !r.done)) return false;
  for (const c of tray.cards) {
    if (recs.some((r) => r.itemId === c.id)) continue;
    // A sibling of an answer goosed has not read yet waits: answering it first would hide a supersede (Q-344).
    if (recs.some((r) => r.answeredAt && !r.siblingsChecked && r.siblings.includes(c.id))) continue;
    const it = db.ok ? db.items.find((x) => x.id === c.id) : null;
    const rec = nyRec(turn, { id: c.id, question: it?.question ?? c.question, options: it ? it.options ?? [] : c.chips }, chat, it ? 'db' : `dom (${db.ok ? 'not in the store' : db.error})`);
    const answeredBefore = recs.filter((r) => r !== rec && r.answeredAt && norm(r.question) === norm(rec.question)).length;
    if (answeredBefore) await nyFinding(turn, rec, 'NEEDS_YOU_REASKED', `asked again after ${answeredBefore} answer(s): ${rec.question.slice(0, 120)}`, 'an answered question does not come back', false);
    const choice = chooseAnswer({ question: rec.question, recommended: c.recommended || it?.recommended_answer || '', options: rec.options }, guidance);
    rec.source = choice.source; rec.match = choice.match;
    if (choice.answer === null || answeredBefore >= 2) {
      rec.source = choice.answer === null ? `unanswerable: ${choice.reason}` : `unanswered: re-asked after ${answeredBefore} answers`;
      rec.done = true; nyRow(rec); nyLog(turn, 'unanswerable', rec); note(`NEEDS_YOU ${rec.source} ${c.id}: ${rec.question.slice(0, 120)}`);
      continue;
    }
    const plan = planClick({ chips: c.chips, recommended: c.recommended }, choice.answer);
    // What the card SENDS: the recommended button carries its full words (answer + reason), not the guidance's.
    rec.answer = plan.text;
    rec.click = plan.kind === 'option' ? `option ${plan.index}` : plan.kind;
    rec.siblings = tray.cards.filter((x) => x.id !== c.id).map((x) => x.id);
    for (const sid of rec.siblings) rec.siblingState[sid] = '?';
    let card;
    try { card = await answerCard(p, c.id, plan); } catch (e) {
      rec.source += ` (answer failed: ${String(e.message ?? e).slice(0, 160)})`; rec.done = true; nyRow(rec);
      await nyFinding(turn, rec, 'NEEDS_YOU_ANSWER_FAILED', String(e.message ?? e).slice(0, 200), 'the card takes an answer through its own controls', false);
      continue;
    }
    rec.answeredAt = iso();
    if (!idle) {
      // Q-341: mid-turn the answer waits on the card as Queued and goes when the turn ends.
      const queued = await card.getByTestId('needs-you-queued').waitFor({ state: 'attached' }).then(() => true, () => false);
      const still = (await screen()).stop;
      rec.queued = queued ? 'y' : still ? 'n' : 'n (the turn ended at the click)';
      if (!queued && still) await nyFinding(turn, rec, 'NEEDS_YOU_NOT_QUEUED', `answered mid-turn, no Queued row on the card`, 'Q-341: an answer given while the turn runs is queued', false);
    } else {
      rec.queued = await card.getByTestId('needs-you-queued').count().then((n) => (n ? 'y (the chat was not idle at the click)' : 'n'), () => 'n');
    }
    nyLog(turn, 'answered', rec);
    note(`NEEDS_YOU answered ${c.id} ${rec.source} via ${rec.click} queued=${rec.queued}: ${rec.answer.slice(0, 100)}`);
    if (idle) return true;
  }
  return false;
}

function nyRec(turn, { id, question, options }, chat, optionsFrom) {
  const rec = { turn, itemId: id, question, options, optionsFrom, answer: '', source: '', match: '', click: '', answeredAt: '', queued: '', deliveredAt: '', deliveredDb: '',
    answerFirst: '', answerTurn: '', cleared: '', dbStatus: '', siblings: [], siblingState: {}, siblingsChecked: false, replyUses: '', reply: '',
    usersAtAnswer: chat.users.length, assistAtDelivery: chat.assistant, done: false };
  recs.push(rec);
  return rec;
}

// The store lists an open question the tray has not drawn yet: give the tray its refresh (it re-reads every 5 s).
async function nySettle(turn) {
  for (let i = 0; ; i++) {
    const db = readDbItems(sessionIdOf(chatUrl));
    if (!db.ok) { note(`NEEDS_YOU store unreadable: ${db.error}`); return; }
    const tray = await readTray(p);
    const missing = db.items.filter((it) => it.status === 'open' && !onScreen(tray, it.id) && !recs.some((r) => r.itemId === it.id && r.done));
    if (!missing.length) return;
    if (i >= TRAY_SETTLE_POLLS) {
      for (const it of missing) if (!nyNotShown.has(it.id)) { nyNotShown.add(it.id); await nyFinding(turn, { itemId: it.id, question: it.question }, 'NEEDS_YOU_NOT_SHOWN', `open in the store, no card in the chat after ${i} polls: ${it.question.slice(0, 120)}`, 'every open question of the chat has a card', false); }
      return;
    }
    await p.waitForTimeout(2000);
  }
}

// An answer given, not yet in the chat: wait for its message (the tray sends it at once when idle, or when the
// running turn ends). True = an answer turn has started. Not there after the tray's refresh while the chat
// is idle = the answer went nowhere (Q-341's hazard): a finding that ends the run.
async function nyAwaitDelivery(turn) {
  const t0 = Date.now();
  for (let i = 0; ; i++) {
    const db = readDbItems(sessionIdOf(chatUrl));
    nyMarkDelivered(await readChat(p), db);
    if (recs.some((r) => r.deliveredAt && !r.done)) return true;
    const pending = recs.filter((r) => r.answeredAt && !r.deliveredAt && !r.done);
    if (!pending.length) return false;
    // A queued answer waits for a running turn (one r1 did not send — a loop tick); that wait is bounded
    // like any turn, by the hang ratio of this run's median.
    const m = median();
    if (i >= TRAY_SETTLE_POLLS && (!(await screen()).stop || (m && (Date.now() - t0) / 1000 > HANG_FACTOR * m))) {
      const tray = await readTray(p);
      for (const r of pending) {
        r.dbStatus = db.ok ? db.items.find((it) => it.id === r.itemId)?.status ?? 'missing' : db.error; r.cleared = onScreen(tray, r.itemId) ? 'n' : 'y'; r.done = true; nyRow(r);
        await nyFinding(turn, r, 'NEEDS_YOU_NOT_DELIVERED', `answered at ${r.answeredAt}, no answer message in the chat; store ${r.dbStatus}${tray.unsent.length ? `; notice: ${tray.unsent[0].slice(0, 120)}` : ''}`, 'an answer reaches the model as the next chat message', true);
      }
      return false;
    }
    await p.waitForTimeout(2000);
  }
}

// After the answer turn: each answered card is gone from the chat and Answered in the store, siblings checked,
// the model's first words recorded so a reader can see it used the answer.
async function nyFinalize(label, batch, res) {
  let tray; let db;
  for (let i = 0; ; i++) {
    db = readDbItems(sessionIdOf(chatUrl)); tray = await readTray(p);
    if (!batch.some((r) => onScreen(tray, r.itemId)) || i >= TRAY_SETTLE_POLLS) break;
    await p.waitForTimeout(2000);
  }
  for (const r of batch) {
    if (!r.siblingsChecked) await nyCheckSiblings(label, r, db, tray);
    r.dbStatus = db.ok ? db.items.find((it) => it.id === r.itemId)?.status ?? 'missing' : `store unreadable: ${db.error}`;
    const shown = onScreen(tray, r.itemId);
    r.cleared = !shown && (r.dbStatus === 'answered' || !db.ok) ? 'y' : `n (${shown ? 'card still in the chat' : 'card gone'}; store ${r.dbStatus})`;
    // The outcome: the whole answer-turn reply against what the card sent (the column keeps 300 chars to read).
    r.answerTurn = label; r.replyUses = textUseCell(r.answer, res.text); r.reply = res.text.slice(0, 300); r.done = true;
    nyRow(r); nyLog(label, 'final', r);
    if (r.cleared !== 'y') await nyFinding(label, r, 'NEEDS_YOU_NOT_CLEARED', `after its answer turn ${label}: ${r.cleared}`, 'an answered question leaves the chat and reads Answered', true);
  }
}

// At a turn boundary: answer the chat's open cards and run every answer turn before the next brief turn.
// True = an answer turn ended in a notice or a hang (the run stops, as for a brief turn).
async function nyTurnEnd(turn) {
  for (let k = 1; !nyStop && !existsSync(`${dir}/STOP`); ) {
    if (!chatUrl || p.url() !== chatUrl) { if (recs.some((r) => !r.done)) note(`NEEDS_YOU skipped at the end of turn ${turn}: the view is not r1's chat`); return false; }
    await nySettle(String(turn));
    await nyTick(String(turn), { mayStart: true });
    if (!recs.some((r) => r.answeredAt && !r.done)) return false;
    if (!(await nyAwaitDelivery(String(turn)))) return false;
    const batch = recs.filter((r) => r.deliveredAt && !r.done);
    const label = `${turn}a${k++}`;
    const res = await runTurn(label, Math.min(...batch.map((r) => r.assistAtDelivery)), Date.parse(batch[0].deliveredAt));
    await nyFinalize(label, batch, res);
    if (res.ended === 'hang' || res.ended === 'notice') return true;
    await p.waitForTimeout(3000);
  }
  return false;
}

// ---------------------------------------------------------------- notes to another chat (Q-358, notes.mjs)
// At a turn boundary (A idle, the chat on screen r1's own): every draft still a draft in the store is acted on
// through its card and its outcome proven. A draft is met once; its row says what was proven.
const notesOut = `${dir}/notes.tsv`;
writeFileSync(notesOut, NOTES_TSV_HEADER);
const noteRecs = [];
async function notesTurnEnd(turn) {
  if (!chatUrl || p.url() !== chatUrl) { note(`NOTES skipped at the end of turn ${turn}: the view is not r1's chat`); return; }
  const a = readNotes(openedSession);
  if (!a.ok) { note(`NOTES store unreadable at the end of turn ${turn}: ${a.error}`); return; }
  for (const d of a.state.drafts) {
    if (d.status !== 'draft' || noteRecs.some((r) => r.noteId === d.id) || existsSync(`${dir}/STOP`)) continue;
    const m = median();
    const rec = await actOnNote({ browser: b, page: p, sessionA: openedSession, turn: String(turn), hangSecs: () => (m ? HANG_FACTOR * m : null), note }, d, noteGuidance);
    noteRecs.push(rec);
    appendFileSync(notesOut, noteRow(rec));
    appendFileSync(`${dir}/live.jsonl`, JSON.stringify({ turn: String(turn), at: iso(), kind: 'note', ...rec, reply: rec.reply.slice(0, 600) }) + '\n');
    note(`NOTE ${d.id} ${rec.action} → ${rec.inboxStatus || rec.draftStatus} ${rec.fails.length ? `FAIL ${rec.fails.join(' | ')}` : 'proven'}`);
    if (rec.fails.length) {
      const f = { kind: rec.undelivered ? 'NOTE_NOT_DELIVERED' : 'NOTE_OUTCOME_WRONG', surface: 'note to another chat', item: d.id, says: rec.fails.join(' | ').slice(0, 240), truth: 'a note acted on reaches the other chat as clicked, and both chats say so' };
      note(`LIVE ${f.kind} ${JSON.stringify(f).slice(0, 300)}`);
      await p.screenshot({ path: `${dir}/note-${turn}-${f.kind}.png` }).catch(() => {});
    }
  }
}

for (let turn = 0; turn < maxTurns; turn++) {
  // A STOP file ends the run at this boundary, before the next turn is sent — killing r1 from outside raced
  // its 3-s gap and sent #3i's turn 4 into an install that stopped the split (Q-219).
  if (existsSync(`${dir}/STOP`)) { note(`STOPPED by ${dir}/STOP before turn ${turn}`); break; }
  const prompt = turn < steps.length ? steps[turn] : `Continue improving the ledger package in ${work}: pick the next most useful feature or fix, implement it with a test, and run the suite. (turn ${turn})`;
  const n0 = await p.evaluate(() => [...document.querySelectorAll('.goose-message')].filter((m) => m.offsetParent).length);
  // The owner shares this app while it runs (Q-147: he was on Providers mid-turn). Mid-turn the driver never
  // yanks his view; to TYPE it must be in its own chat, so it returns there only at a turn boundary.
  if (chatUrl && p.url() !== chatUrl) { note(`RETURN to own chat from ${p.url().split('#')[1]}`); await p.goto(chatUrl); await p.waitForTimeout(3000); }
  const input = p.locator('[data-testid=chat-input]:visible').first();
  const row0 = lastRowId(openedSession);
  await input.click(); await input.fill(prompt); await p.keyboard.press('Enter');
  let r = await runTurn(String(turn), n0, Date.now());
  // Q-492: a turn is DONE only when its words reached the chat. #3x's turn 1 (the memory turn) was pressed,
  // never stored, and counted done in 5 s with 0 tools — the Stop button's absence proves nothing about a send.
  if (r.ended === 'done' && !sentLanded(row0, prompt)) {
    const box = await input.evaluate((el) => ({ value: (el.value ?? el.innerText ?? '').slice(0, 120), disabled: !!el.disabled || el.getAttribute('aria-disabled') === 'true' })).catch((e) => ({ error: String(e).slice(0, 120) }));
    const queued = await p.evaluate(() => [...document.querySelectorAll('[data-testid*=queue], [data-testid*=queued]')].map((e) => e.innerText.slice(0, 80)).join(' | ')).catch(() => '');
    await p.screenshot({ path: `${dir}/send-lost-${turn}.png` }).catch(() => {});
    note(`LIVE SEND_LOST turn ${turn}: pressed Enter, no user row after ${row0}; composer ${JSON.stringify(box)} queued "${queued}"`);
    // A person who sees their message vanish sends it again; one resend, recorded as its own turn row.
    const n1 = await p.evaluate(() => [...document.querySelectorAll('.goose-message')].filter((m) => m.offsetParent).length);
    const row1 = lastRowId(openedSession);
    await input.click(); await input.fill(prompt); await p.keyboard.press('Enter');
    r = await runTurn(`${turn}r`, n1, Date.now());
    if (!sentLanded(row1, prompt)) { note(`STOPPED: turn ${turn} did not reach the chat twice (SEND_LOST)`); break; }
  }
  // Q-390: the turn went into the chat opened (and verified) in <dir>/work, not some other chat.
  if (turn === 0 && sessionIdOf(chatUrl || p.url()) !== openedSession) { note(`WORKDIR_FAIL turn 0 ran in ${sessionIdOf(chatUrl || p.url()) || chatUrl}, not the chat opened in ${work} (${openedSession})`); writeRound({ workingDirVerified: false, workingDirError: `turn 0 ran in ${sessionIdOf(chatUrl || p.url()) || chatUrl}` }); console.error(`r1 STOPPED after turn 0 (Q-390): the turn did not run in ${openedSession}`); break; }
  if (turn % 5 === 0) await p.screenshot({ path: `${dir}/turn-${turn}.png` });
  // STOP RULE (skill): a turn that ends in a notice ends the round — E2E #3e sent 29 more turns into a stopped
  // split, 5 s each, and recorded them as turns.
  if (r.ended === 'hang' || r.ended === 'notice') break;
  await p.waitForTimeout(3000);
  // Q-376: the cards this turn raised are answered, and their answer turns run, before the next brief turn.
  const answerTurnFailed = await nyTurnEnd(turn);
  // STOP RULE (Q-376): a card still there after its answer turn, a sibling closed by a card answer, or an
  // answer that never reached the chat ends the run here, with the reason.
  if (nyStop) { note(`STOPPED by needs-you finding after turn ${turn}: ${nyStop}`); break; }
  if (answerTurnFailed) break;
  // Q-358: the notes this turn (or its answer turns) drafted are acted on before the next brief turn.
  await notesTurnEnd(turn);
}
// A card met but not settled when the run ended (a stop, a hang) still gets its row, saying so.
for (const r of recs) if (!r.done) { r.cleared = r.cleared || 'unchecked: the run ended first'; r.done = true; nyRow(r); nyLog(r.turn, 'unsettled', r); }
// Q-390: goose can move a chat's folder itself (its "set as this chat's folder?" card) — the folder at the end is
// recorded beside the one it started in, so a rubric reads where the chat actually was.
{ const end = readWorkingDir(openedSession); const v = checkWorkingDir(end, work); writeRound({ workingDirAtEnd: end.ok ? end.workingDir : `unreadable: ${end.error}`, workingDirAtEndIsWork: v.ok }); if (!v.ok) note(`WORKDIR_MOVED at the end: ${v.says}`); }
// ROUND END (owner 2026-09-29: nothing stays pending): every open card and every note not delivered as acted.
{
  const tray = p.url() === chatUrl ? await readTray(p).catch(() => ({ cards: [] })) : { cards: [] };
  const fails = [...openCardFails(readDbItems(openedSession), tray), ...noteFails(readNotes(openedSession), noteRecs)];
  const notesWaiting = noteRecs.filter((r) => !r.fails.length && r.delivery === 'leave').map((r) => `${r.noteId} waits in "${r.targetName}" (${r.inboxStatus}) — Leave it there, proven`);
  writeRound({ endedAt: iso(), fails, notesWaiting, notesActed: noteRecs.length });
  for (const f of fails) { note(f); console.log(f); }
  if (!fails.length) console.log(`round end: no open card, no undelivered note (${noteRecs.length} note(s) acted on)`);
}
await b.close(); process.exit(0);
