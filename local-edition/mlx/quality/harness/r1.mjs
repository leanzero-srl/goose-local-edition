// R1 — a long AGENTIC goose session on whatever engine serves chat (the split, for R1), through the product.
// usage: node r1.mjs <dir> [--turns N] [--brief <brief.json>]   (a goose-task-author brief; {WORK} → <dir>/work)
// One new chat; turn after turn of real tool work inside <dir>/work (files, shell, tests), so the context
// climbs from goose's ~49k-token prompt toward compaction. Per turn one TSV row: start, end, seconds, how the
// turn ended (done / notice / stall), the chip, the context counter, the notice text if any.
// A turn with no change on screen for STALL_FACTOR x the running median turn length (never below the first
// turn's own length) is logged STALL with a screenshot and the soak goes on waiting; HANG_FACTOR x ends the soak —
// a hang is the finding, and the driver never cancels, retries or edits the turn itself.
import { chromium } from '/Users/mihaiperdum/Projects/goose/ui/node_modules/playwright-core/index.mjs';
import { mainPage } from './mainpage.mjs';
import { mkdirSync, writeFileSync, appendFileSync, readFileSync, readdirSync, statSync, existsSync } from 'node:fs';
import { liveCheck } from './livecheck.mjs';
const dir = process.argv[2];
const turnsArg = process.argv.indexOf('--turns'); const maxTurnsArg = turnsArg > 0 ? Number(process.argv[turnsArg + 1]) : 0;
const work = `${dir}/work`; mkdirSync(work, { recursive: true });
const STALL_FACTOR = 5; // ratio: of the median turn length measured in this soak
const HANG_FACTOR = 15; // ratio: same; the soak.py hang rule of Step 1b used 10x a running median
const AWAY_IDLE_POLLS = 3; // three 2-s polls with nothing served: a gap between calls or the turn end, never mid-stream
const out = `${dir}/turns.tsv`;
writeFileSync(out, 'turn\tstart\tend\tsecs\tended\ttools\trecalled\tchip\tcounter\tnotice\n');
// What THIS turn added: the messages after the send, never the whole page (the smoke run matched an older
// session's notice). A failed turn is any goose notice or the engine's empty-response line.
const FAIL = /empty response|stopped answering|quit goose mid|No node can|Ran into this error|split across your Macs stopped|stopped making progress/;
const added = (n0) => p.evaluate((n0) => { const ms = [...document.querySelectorAll('.goose-message')].filter((m) => m.offsetParent).slice(n0).map((m) => m.innerText); const recalled = ms.join('\n').split('\n').filter((l) => /^recalled:/.test(l.trim())).join(' | '); const tools = [...document.querySelectorAll('.goose-message')].filter((m) => m.offsetParent).slice(n0).reduce((k, m) => k + m.querySelectorAll('[class*=tool i], details').length, 0); return { text: ms.join(' ').replace(/\s+/g, ' '), tools, recalled }; }, n0);
const briefArg = process.argv.indexOf('--brief');
const brief = briefArg > 0 ? JSON.parse(readFileSync(process.argv[briefArg + 1], 'utf8')) : null;
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
// Every provider call goose makes lands in a rotating llm_request.<n>.jsonl whose LAST line carries usage
// (input, output, cache_read). Read on every poll so no call of a many-tool turn is missed.
const LOGS = `${process.env.HOME}/.local/state/goose/logs`;
const seenCalls = new Map();
const pollCalls = () => { const got = []; for (const f of readdirSync(LOGS)) { if (!/^llm_request\.\d+\.jsonl$/.test(f)) continue; let st; try { st = statSync(`${LOGS}/${f}`); } catch { continue; } /* goose rotates these between readdir and stat (E2E #4b died on ENOENT) */ const key = `${st.ino}:${st.mtimeMs}`; /* inode, not name: goose renames .0→.1→… on rotation and E2E #3i re-counted every old call under each new name */ if (seenCalls.has(key)) continue; try { const L = readFileSync(`${LOGS}/${f}`, 'utf8').trimEnd().split('\n'); const u = JSON.parse(L.at(-1)).usage; if (!u) continue; seenCalls.set(key, 1); got.push(u); } catch {} } return got; };
pollCalls();
writeFileSync(`${dir}/calls.tsv`, 'turn\tinput\toutput\tcache_read\n');
const b = await chromium.connectOverCDP('http://127.0.0.1:9333');
const p = await mainPage(b);
await p.goto(p.url().split('#')[0] + '#/'); await p.waitForTimeout(2500);
await p.getByRole('button', { name: /^New session in / }).click(); await p.waitForTimeout(4000);
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
for (let turn = 0; turn < maxTurns; turn++) {
  // A STOP file ends the run at this boundary, before the next turn is sent — killing r1 from outside raced
  // its 3-s gap and sent #3i's turn 4 into an install that stopped the split (Q-219).
  if (existsSync(`${dir}/STOP`)) { appendFileSync(`${dir}/events.log`, `${new Date().toISOString()} STOPPED by ${dir}/STOP before turn ${turn}\n`); break; }
  const prompt = turn < steps.length ? steps[turn] : `Continue improving the ledger package in ${work}: pick the next most useful feature or fix, implement it with a test, and run the suite. (turn ${turn})`;
  const n0 = await p.evaluate(() => [...document.querySelectorAll('.goose-message')].filter((m) => m.offsetParent).length);
  // The owner shares this app while it runs (Q-147: he was on Providers mid-turn). Mid-turn the driver never
  // yanks his view; to TYPE it must be in its own chat, so it returns there only at a turn boundary.
  if (chatUrl && p.url() !== chatUrl) { appendFileSync(`${dir}/events.log`, `${new Date().toISOString()} RETURN to own chat from ${p.url().split('#')[1]}\n`); await p.goto(chatUrl); await p.waitForTimeout(3000); }
  const input = p.locator('[data-testid=chat-input]:visible').first();
  await input.click(); await input.fill(prompt); await p.keyboard.press('Enter');
  const start = Date.now(); let lastChange = Date.now(); let prev = null; let ended = ''; let stallLogged = false;
  await p.waitForTimeout(3000);
  let polls = 0; let away = false; let idleAway = 0;
  while (true) {
    if (!chatUrl && (Date.now() - start) > 8000) chatUrl = p.url();
    // Every ~minute: does every surface agree that this session is live? (livecheck.mjs, Q-147)
    if (polls++ % 30 === 0) {
      // Re-read every time: goose retitles a chat after its first answer, and a title read once at the start
      // ('New Session') made every later check report the live session as not listed (E2E #5b).
      title = await p.evaluate(() => document.querySelector('[data-testid=session-title-trigger]')?.innerText.trim() ?? '').catch(() => title);
      const lc = await liveCheck(p, { title }).catch((e) => ({ findings: [{ kind: 'PROBE_ERROR', says: String(e) }] }));
      appendFileSync(`${dir}/live.jsonl`, JSON.stringify({ turn, ...lc }) + '\n');
      for (const f of lc.findings) if (!liveSeen.has(f.kind)) { liveSeen.add(f.kind); await p.screenshot({ path: `${dir}/live-${turn}-${f.kind}.png` }); appendFileSync(`${dir}/events.log`, `${new Date().toISOString()} LIVE ${f.kind} ${JSON.stringify(f).slice(0, 300)}\n`); }
    }
    // Someone else moved the view: nothing on screen is this turn's, so no done/stall verdict is taken from it.
    if (chatUrl && p.url() !== chatUrl) {
      if (!away) { away = true; idleAway = 0; appendFileSync(`${dir}/events.log`, `${new Date().toISOString()} VIEW_AWAY ${p.url().split('#')[1]}\n`); }
      // Q-186: with the view away, the turn's end is invisible — #3i waited until a human navigated back.
      // When the engine serves nothing for a few polls in a row, goose is between calls or done: go back
      // to the chat, where the boundary can be read (a person on another page is only moved while idle).
      const busy = await p.evaluate(async () => (await window.electron.mlxEngineActivity())?.stats?.numRunning ?? 0).catch(() => 1);
      idleAway = busy ? 0 : idleAway + 1;
      if (idleAway >= AWAY_IDLE_POLLS) {
        await p.goto(chatUrl); await p.waitForTimeout(4000);
        appendFileSync(`${dir}/events.log`, `${new Date().toISOString()} VIEW_RETURNED engine idle ${idleAway} polls\n`);
      }
      lastChange = Date.now(); await p.waitForTimeout(2000); continue;
    }
    away = false;
    for (const u of pollCalls()) appendFileSync(`${dir}/calls.tsv`, [turn, u.input_tokens, u.output_tokens, u.cache_read_input_tokens ?? ''].join('\t') + '\n');
    const s = await screen();
    if (!prev || s.len !== prev.len || s.chip !== prev.chip) lastChange = Date.now();
    prev = s;
    const quiet = (Date.now() - lastChange) / 1000; const m = median() ?? (Date.now() - start) / 1000;
    if (!s.stop && (Date.now() - start) > 5000) { ended = 'done'; break; }
    if (!stallLogged && lengths.length && quiet > STALL_FACTOR * m) { stallLogged = true; await p.screenshot({ path: `${dir}/stall-${turn}.png` }); appendFileSync(`${dir}/events.log`, `${new Date().toISOString()} STALL turn ${turn} quiet ${quiet.toFixed(0)}s median ${m.toFixed(0)}s chip ${s.chip}\n`); }
    if (lengths.length && quiet > HANG_FACTOR * m) { ended = 'hang'; await p.screenshot({ path: `${dir}/hang-${turn}.png` }); break; }
    await p.waitForTimeout(2000);
  }
  const end = Date.now(); const secs = (end - start) / 1000; const s = await screen();
  const a = await added(n0); const failed = a.text.match(FAIL);
  if (failed && ended === 'done') { ended = 'notice'; await p.screenshot({ path: `${dir}/notice-${turn}.png` }); }
  if (ended === 'done') lengths.push(secs);
  appendFileSync(out, [turn, new Date(start).toISOString(), new Date(end).toISOString(), secs.toFixed(1), ended, a.tools, a.recalled, s.chip, s.counter, failed ? a.text.slice(Math.max(0, failed.index - 60), failed.index + 140) : ''].join('\t') + '\n');
  if (turn % 5 === 0) await p.screenshot({ path: `${dir}/turn-${turn}.png` });
  // STOP RULE (skill): a turn that ends in a notice ends the round — E2E #3e sent 29 more turns into a stopped
  // split, 5 s each, and recorded them as turns.
  if (ended === 'hang' || ended === 'notice') break;
  await p.waitForTimeout(3000);
}
await b.close(); process.exit(0);
