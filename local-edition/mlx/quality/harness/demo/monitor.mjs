// read-only sampler: engine activity every 3s, memory both Macs every 30s -> DIR/monitor.jsonl
import { page, DIR } from './lib.mjs';
import { appendFileSync } from 'node:fs';
import { execSync } from 'node:child_process';
const { p } = await page();
const out = `${DIR}/monitor.jsonl`;
let last = '', k = 0;
while (true) {
  const t = new Date().toISOString();
  try {
    const a = await p.evaluate(async () => { const a = await window.electron.mlxEngineActivity(); return { engine: a.engine, mode: a.mode, detail: a.statusDetail, model: a.measured?.answer?.way?.placementId, run: a.stats?.numRunning, wait: a.stats?.numWaiting, reqs: (a.stats?.requests ?? []).map((r) => `${r.id}:${r.status}:${r.completionTokens}`).join(' ') }; });
    const key = JSON.stringify({ ...a, reqs: a.reqs.replace(/:\d+(\s|$)/g, '$1') });
    if (key !== last || k % 10 === 0) { appendFileSync(out, JSON.stringify({ t, ...a }) + '\n'); last = key; }
  } catch (e) { appendFileSync(out, JSON.stringify({ t, err: String(e).slice(0, 200) }) + '\n'); }
  if (k % 10 === 0) {
    try {
      const mb = execSync("top -l 1 -n 0 | grep PhysMem").toString().trim();
      const st = execSync("ssh -o ConnectTimeout=5 workhorse 'top -l 1 -n 0 | grep PhysMem'").toString().trim();
      appendFileSync(out, JSON.stringify({ t, mem: { macbook: mb, studio: st } }) + '\n');
    } catch (e) { appendFileSync(out, JSON.stringify({ t, memErr: String(e).slice(0, 200) }) + '\n'); }
  }
  k++; await new Promise((r) => setTimeout(r, 3000));
}
