// R5 switch races, driven through the installed app's Run it card (CDP 9333).
// usage: node r5.mjs <evidence-dir>   — each step: act, wait for the tile to settle, census both Macs.
// J4 (design DESIGN-NODES-AND-STRATEGIES.md §10.4, the node loader's thrash and starvation probe):
//   J4=1 NODE_A=<node id> NODE_B=<node id> [ROUNDS=10] node r5.mjs <evidence-dir>
//   Two chats on two nodes with DIFFERENT ways, each asked for several tool steps per reply; chat B is
//   sent while chat A's reply is still running, every round. Checks, per round and in total: swaps
//   (new Ready rows in mlx-load-measurements.jsonl) equal reply alternations and never the model-call
//   count; no swap lands mid-reply; no reply starves; never two engines on one Mac (census); the load
//   lock and the swap claim name one holder at every sample; a turn stopped while it waits in the
//   loader's queue leaves no swap behind. New chats pick their node through GOOSE_PROVIDER/GOOSE_MODEL
//   in config.yaml (what nodes/write's forNewChats writes); the file's original bytes are restored.
import { chromium } from '/Users/mihaiperdum/Projects/goose/ui/node_modules/playwright-core/index.mjs';
import { mainPage } from './mainpage.mjs';
import { execSync } from 'node:child_process';
import { readFileSync, writeFileSync, appendFileSync, readdirSync, statSync, existsSync } from 'node:fs';
const dir = process.argv[2];
const census = (label) =>
  execSync(`CENSUS_OUT=${dir}/census.jsonl /Users/mihaiperdum/Projects/goose/local-edition/mlx/quality/harness/census.sh ${label}`, { encoding: 'utf8' }).trim();
const b = await chromium.connectOverCDP('http://127.0.0.1:9333');
const p = await mainPage(b);
p.setDefaultTimeout(15000);
const log = (...a) => console.log(new Date().toISOString().slice(11, 19), ...a);
async function openEngine() {
  await p.goto(p.url().split('#')[0] + '#/leanzero-swarm'); await p.waitForTimeout(2500);
  await p.getByText('LeanZero MLX', { exact: true }).first().click(); await p.waitForTimeout(4000);
}
const WAYS = { here: /Run on this Mac/, studio: /Run on Work.s Mac Studio/, both: /Run across both Macs/ };
async function buttonIndex(label, way) {
  return p.evaluate(({ label, src }) => {
    const re = new RegExp(src);
    const bs = [...document.querySelectorAll('button')].filter((b) => b.innerText.trim() === label);
    return bs.findIndex((b) => { let n = b; for (let k = 0; k < 8 && n; k++) { n = n.parentElement; if (!n) break;
      const heads = n.innerText.match(/Run on this Mac|Run on Work.s Mac Studio|Run across both Macs/g) || [];
      if (heads.length === 1) return re.test(heads[0]); if (heads.length > 1) return false; } return false; });
  }, { label, src: WAYS[way].source });
}
// A disabled button while another switch runs IS the UI refusing the race: recorded, never an error.
async function click(label, way) {
  const i = await buttonIndex(label, way);
  if (i < 0) { log(`NO ${label} button on ${way}`); return false; }
  const btn = p.getByRole('button', { name: new RegExp(`^${label}$`) }).nth(i);
  if (await btn.isDisabled().catch(() => true)) { log(`REFUSED ${label} on ${way}: disabled while busy`); return false; }
  await btn.click({ timeout: 5000 }).catch((e) => log(`click ${label} on ${way} failed: ${e.message.split('\n')[0]}`));
  log(`clicked ${label} on ${way}`); return true;
}
async function tile() {
  return p.evaluate(() => { const s = document.body.innerText; const a = s.indexOf('Sampling'); const r = s.indexOf('RUN IT');
    return (s.slice(a + 9, a + 170) + ' || ' + s.slice(r, r + 60)).replace(/\n+/g, ' | '); });
}
async function settle(tag, maxS = 420) {
  const t0 = Date.now(); let last = '', same = 0;
  while ((Date.now() - t0) / 1000 < maxS) {
    await p.waitForTimeout(3000);
    const t = await tile();
    // Only the headline decides "settled": the tile's counters (uptime, requests) change every poll.
    const head = t.split(' | ').slice(0, 4).join(' | ');
    same = head === last ? same + 1 : 0; last = head;
    if (same >= 3 && !/Mounting|Loading|Starting|Stopping|Checking|Restoring|Building/.test(t)) break;
  }
  log(`${tag} settled in ${Math.round((Date.now() - t0) / 1000)}s: ${last.slice(0, 220)}`);
  await p.screenshot({ path: `${dir}/${tag}.png` });
  log(census(tag));
}
if (process.env.J4) { await j4(); await b.close(); process.exit(0); }
await openEngine();
await settle('r5-0-start', 60);
// a) Run across both Macs, then — while it is still starting — Run on the Studio.
if (!process.env.SKIP_A && await click('Run', 'both')) { await p.waitForTimeout(4000); await click('Run', 'studio'); await settle('r5-a-both-then-studio'); }
// b) Run on this Mac, double-clicked.
{ const i = await buttonIndex('Run', 'here'); if (i >= 0) { const btn = p.getByRole('button', { name: /^Run$/ }).nth(i); await btn.dblclick(); log('double-clicked Run on here'); await settle('r5-b-here-double'); } }
// c) back to the Studio, then Run on this Mac while the Studio is still mounting.
if (await click('Run', 'studio')) { await p.waitForTimeout(3000); await click('Run', 'here'); await settle('r5-c-studio-then-here'); }
// d) end where the owner left it: the Studio.
if (await click('Run', 'studio')) await settle('r5-d-end-studio');
await b.close();

// ---------------------------------------------------------------------------------------------
// J4 — two chats in tool loops on two ways: swaps per REPLY alternation, never per model call.
// ---------------------------------------------------------------------------------------------
async function j4() {
  const HOME = process.env.HOME;
  const CONFIG = `${HOME}/.config/goose/config.yaml`;
  const LOADS = `${HOME}/.local/share/goose/mlx-load-measurements.jsonl`;
  const HOLDERS = `${HOME}/.local/state/goose/mlx-holders`;
  const LOCK = `${HOME}/.local/state/goose/mlx-load.lock`;
  const LOGS = `${HOME}/.local/state/goose/logs`;
  const A = process.env.NODE_A, B = process.env.NODE_B, ROUNDS = Number(process.env.ROUNDS || 10);
  if (!A || !B || A === B) throw new Error('J4 needs NODE_A and NODE_B: two nodes whose ways differ');
  const tsv = `${dir}/j4.tsv`;
  writeFileSync(tsv, 'round\tchat\tstart\tend\tsecs\tcalls\tswaps_during\tended\n');
  const events = (line) => appendFileSync(`${dir}/j4-events.log`, `${new Date().toISOString()} ${line}\n`);
  const loadRows = () => (existsSync(LOADS) ? readFileSync(LOADS, 'utf8').trim().split('\n').filter(Boolean).map((l) => { try { return JSON.parse(l); } catch { return { unreadable: l }; } }) : []);
  const readyRows = () => loadRows().filter((r) => r.outcome?.kind === 'ready');
  // Every provider call lands in a rotating llm_request.<n>.jsonl (r1.mjs's reader, by inode).
  const seen = new Map();
  const calls = () => { let n = 0; for (const f of readdirSync(LOGS)) { if (!/^llm_request\.\d+\.jsonl$/.test(f)) continue; let st; try { st = statSync(`${LOGS}/${f}`); } catch { continue; } const k = `${st.ino}:${st.mtimeMs}`; if (seen.has(k)) continue; seen.set(k, 1); n++; } return n; };
  calls();
  // One holder per shared record at every sample: the load lock (key=value) and the swap claim.
  const holderOf = (path) => { try { const t = readFileSync(path, 'utf8').trim(); return t ? (t.match(/^pid=(\d+)/m)?.[1] ?? 'unreadable') : null; } catch { return null; } };
  const sample = (tag) => {
    const lock = holderOf(LOCK), claim = holderOf(`${HOLDERS}/swap.claim`);
    const replies = existsSync(HOLDERS) ? readdirSync(HOLDERS).filter((f) => f.endsWith('.json')).flatMap((f) => { try { return (JSON.parse(readFileSync(`${HOLDERS}/${f}`, 'utf8')).replies ?? []).map((r) => `${r.session.slice(0, 8)}:${r.way?.kind ?? '-'}${r.waiting ? ':waiting' : ''}`); } catch { return [`${f}:unreadable`]; } }) : [];
    appendFileSync(`${dir}/j4-samples.jsonl`, JSON.stringify({ t: new Date().toISOString(), tag, lock, claim, replies }) + '\n');
    if (lock === 'unreadable' || claim === 'unreadable') events(`FINDING ${tag}: a holder record is unreadable (lock ${lock}, claim ${claim})`);
  };
  const original = readFileSync(CONFIG);
  const setModel = (id) => {
    const text = original.toString('utf8').split('\n').filter((l) => !/^GOOSE_(PROVIDER|MODEL):/.test(l));
    writeFileSync(CONFIG, [...text.filter((l, i) => i < text.length - 1 || l !== ''), 'GOOSE_PROVIDER: swarm', `GOOSE_MODEL: node:${id}`, ''].join('\n'));
  };
  const chats = {};
  try {
    for (const [name, id] of [['A', A], ['B', B]]) {
      setModel(id);
      await p.goto(p.url().split('#')[0] + '#/'); await p.waitForTimeout(2500);
      await p.getByRole('button', { name: /^New session in / }).click(); await p.waitForTimeout(4000);
      chats[name] = p.url();
      events(`chat ${name} on node:${id} at ${chats[name]}`);
    }
  } finally {
    writeFileSync(CONFIG, original);
  }
  const work = `${dir}/work`; execSync(`mkdir -p ${work}/a ${work}/b`);
  const prompt = (who, round) => `Work only inside ${work}/${who}. Step by step, one tool call each: list the directory, create notes_${round}.txt with three lines about round ${round}, read it back, append a fourth line, count its lines with wc -l, and tell me the count.`;
  const send = async (who, round) => {
    await p.goto(chats[who]); await p.waitForTimeout(2500);
    const input = p.locator('[data-testid=chat-input]:visible').first();
    await input.click(); await input.fill(prompt(who.toLowerCase(), round)); await p.keyboard.press('Enter');
    await p.waitForTimeout(3000);
  };
  const running = async (who) => { if (p.url() !== chats[who]) { await p.goto(chats[who]); await p.waitForTimeout(2000); } return p.evaluate(() => !!document.querySelector('button[aria-label="Stop"]')); };
  const waitDone = async (who) => { const t0 = Date.now(); while (await running(who)) { sample(`wait-${who}`); await p.waitForTimeout(2000); } return (Date.now() - t0) / 1000; };
  let replies = 0, alternations = 0, lastChat = null; const ready0 = readyRows().length; let callsTotal = 0;
  for (let round = 0; round < ROUNDS; round++) {
    if (existsSync(`${dir}/STOP`)) { events(`STOPPED before round ${round}`); break; }
    const before = readyRows().length; const t0 = Date.now();
    await send('A', round);
    // Chat B while chat A's reply is still in its tool loop: B's demand queues behind A's reply.
    const cancelProbe = round === Math.floor(ROUNDS / 2);
    await send('B', round);
    if (cancelProbe) {
      const stop = p.locator('button[aria-label="Stop"]').first();
      if (await stop.isVisible().catch(() => false)) { await stop.click(); events(`round ${round}: stopped chat B while it waited`); }
    }
    const aSecs = await waitDone('A');
    const bSecs = cancelProbe ? 0 : await waitDone('B');
    const got = calls(); callsTotal += got;
    const swaps = readyRows().length - before;
    const turnReplies = cancelProbe ? 1 : 2;
    for (const who of cancelProbe ? ['A'] : ['A', 'B']) { if (lastChat && lastChat !== who) alternations++; lastChat = who; }
    replies += turnReplies;
    appendFileSync(tsv, [round, 'A+B', new Date(t0).toISOString(), new Date().toISOString(), ((Date.now() - t0) / 1000).toFixed(1), got, swaps, `a ${aSecs.toFixed(0)}s b ${bSecs.toFixed(0)}s`].join('\t') + '\n');
    if (swaps > turnReplies) events(`FINDING round ${round}: ${swaps} swaps for ${turnReplies} replies (${got} model calls) — a swap per call or mid-reply`);
    if (cancelProbe && swaps > 1) events(`FINDING round ${round}: the stopped chat B still swapped (${swaps} swaps)`);
    const c = census(`j4-round-${round}`); log(c);
    for (const line of c.split('\n')) { const n = Number(line.match(/: (\d+) engine procs/)?.[1] ?? 0); if (n > 3) events(`FINDING round ${round}: ${line} — more engine processes than one way holds`); }
  }
  const swapsTotal = readyRows().length - ready0;
  const summary = `J4: ${replies} replies, ${alternations} alternations, ${swapsTotal} swaps, ${callsTotal} model calls`;
  events(summary); log(summary);
  if (swapsTotal > alternations + 1) events(`FINDING: swaps (${swapsTotal}) exceed reply alternations (${alternations})`);
  if (callsTotal > 0 && swapsTotal >= callsTotal) events(`FINDING: swaps (${swapsTotal}) track model calls (${callsTotal}), not replies`);
}
