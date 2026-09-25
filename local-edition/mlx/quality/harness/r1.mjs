// R1 — a long AGENTIC goose session on whatever engine serves chat (the split, for R1), through the product.
// usage: node r1.mjs <dir> [--turns N]
// One new chat; turn after turn of real tool work inside <dir>/work (files, shell, tests), so the context
// climbs from goose's ~49k-token prompt toward compaction. Per turn one TSV row: start, end, seconds, how the
// turn ended (done / notice / stall), the chip, the context counter, the notice text if any.
// A turn with no change on screen for STALL_FACTOR x the running median turn length (never below the first
// turn's own length) is logged STALL with a screenshot and the soak goes on waiting; HANG_FACTOR x ends the soak —
// a hang is the finding, and the driver never cancels, retries or edits the turn itself.
import { chromium } from '/Users/mihaiperdum/Projects/goose/ui/node_modules/playwright-core/index.mjs';
import { mkdirSync, writeFileSync, appendFileSync } from 'node:fs';
const dir = process.argv[2];
const maxTurns = Number(process.argv[process.argv.indexOf('--turns') + 1]) || 200;
const work = `${dir}/work`; mkdirSync(work, { recursive: true });
const STALL_FACTOR = 5; // ratio: of the median turn length measured in this soak
const HANG_FACTOR = 15; // ratio: same; the soak.py hang rule of Step 1b used 10x a running median
const out = `${dir}/turns.tsv`;
writeFileSync(out, 'turn\tstart\tend\tsecs\tended\tchip\tcounter\tnotice\n');
const steps = [
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
const b = await chromium.connectOverCDP('http://127.0.0.1:9333');
const p = b.contexts()[0].pages().find((x) => x.url().includes('index.html'));
await p.goto(p.url().split('#')[0] + '#/'); await p.waitForTimeout(2500);
await p.getByRole('button', { name: /^New session in / }).click(); await p.waitForTimeout(4000);
const screen = () => p.evaluate(() => {
  const main = document.querySelector('main') ?? document.body;
  const chipEl = document.querySelector('[data-testid=model-chip-served]')?.closest('button');
  const counter = [...document.querySelectorAll('button, span, div')].map((e) => e.childElementCount === 0 ? e.textContent.trim() : '').find((t) => /^\d+(\.\d+)?k? \/ \d+k$/.test(t)) ?? '';
  const stop = !!document.querySelector('button[aria-label="Stop"]');
  const notices = [...document.querySelectorAll('.goose-message')].map((m) => m.innerText).filter((t) => /stopped answering|quit goose mid|No node can|Ran into this error|Retry/.test(t));
  return { len: main.innerText.length, chip: (chipEl?.innerText ?? '').replace(/\s+/g, ' '), counter, stop, notice: notices.at(-1)?.replace(/\s+/g, ' ').slice(0, 200) ?? '' };
});
const lengths = []; const median = () => { const s = [...lengths].sort((a, b) => a - b); return s.length ? s[Math.floor(s.length / 2)] : null; };
let noticesSeen = 0;
for (let turn = 0; turn < maxTurns; turn++) {
  const prompt = turn < steps.length ? steps[turn] : `Continue improving the ledger package in ${work}: pick the next most useful feature or fix, implement it with a test, and run the suite. (turn ${turn})`;
  const input = p.locator('[data-testid=chat-input]:visible').first();
  await input.click(); await input.fill(prompt); await p.keyboard.press('Enter');
  const start = Date.now(); let lastChange = Date.now(); let prev = null; let ended = ''; let stallLogged = false;
  await p.waitForTimeout(3000);
  while (true) {
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
  const noticeNow = s.notice && s.notice !== '' ? s.notice : '';
  const isNewNotice = noticeNow && (await p.evaluate(() => [...document.querySelectorAll('.goose-message')].filter((m) => /stopped answering|quit goose mid|No node can|Ran into this error/.test(m.innerText)).length)) > noticesSeen;
  if (isNewNotice) { noticesSeen++; ended = ended === 'done' ? 'notice' : ended; await p.screenshot({ path: `${dir}/notice-${turn}.png` }); }
  if (ended === 'done') lengths.push(secs);
  appendFileSync(out, [turn, new Date(start).toISOString(), new Date(end).toISOString(), secs.toFixed(1), ended, s.chip, s.counter, isNewNotice ? noticeNow : ''].join('\t') + '\n');
  if (turn % 5 === 0) await p.screenshot({ path: `${dir}/turn-${turn}.png` });
  if (ended === 'hang') break;
  await p.waitForTimeout(3000);
}
await b.close(); process.exit(0);
