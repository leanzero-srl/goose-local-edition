// What the user SEES, second by second, while a break recovers (the owner, 2026-09-25: "what happens
// during these seconds? … it's a long time to wait 13s without having any clear visual cue").
// usage: node recovery.mjs <dir> <kill-link|relaunch-peer> — starts a long answer in a new chat, breaks
// the Studio once the answer is being written, then samples the chat screen every second for 60 s:
// the readiness bar, the model chip, the tail of the transcript, and main's engine snapshot (the tray).
import { chromium } from '/Users/mihaiperdum/Projects/goose/ui/node_modules/playwright-core/index.mjs';
import { spawn } from 'node:child_process';
import { writeFileSync, appendFileSync } from 'node:fs';
const [dir, mode] = process.argv.slice(2);
const b = await chromium.connectOverCDP('http://127.0.0.1:9333');
const p = b.contexts()[0].pages().find((x) => x.url().includes('index.html'));
await p.goto(p.url().split('#')[0] + '#/'); await p.waitForTimeout(2500);
await p.getByRole('button', { name: /^New session in / }).click(); await p.waitForTimeout(4000);
const input = p.locator('[data-testid=chat-input]:visible').first();
// A fresh subject every run: with the same prompt, recall surfaced the earlier runs and the model answered
// "you asked this four times" in 6 s — the break then hit no answer in flight (3.0.37 relaunch run).
const subjects = ['a clockmaker in Prague', 'a ferry pilot in Lofoten', 'a beekeeper in Crete', 'a night-shift baker in Lyon', 'a glass blower in Murano', 'a tram driver in Lisbon', 'a cartographer in Tromsø', 'a violin maker in Cremona'];
const subject = subjects[Math.floor(Math.random() * subjects.length)];
await input.click(); await input.fill(`Write a 400-word story about ${subject}, run ${Date.now()}. No tools, no preamble.`);
await p.keyboard.press('Enter');
const t0 = Date.now(); const out = `${dir}/timeline.tsv`;
writeFileSync(out, 't_s\tevent\tbar\tchip\ttranscript_tail\ttray\n');
const snap = async () => p.evaluate(async () => {
  const bar = document.querySelector('[data-testid=composer-readiness]')?.innerText.replace(/\s+/g, ' ') ?? '';
  const chipEl = document.querySelector('[data-testid=model-chip-served]')?.closest('button');
  // Visible text only: the dot's aria-label was never on screen (round 3, Q-56) — a harness that reads
  // hidden words reports cues no user saw. The label is kept apart, marked hidden.
  const dot = chipEl?.querySelector('[aria-label]')?.getAttribute('aria-label') ?? '';
  const chip = `${(chipEl?.innerText ?? '').replace(/\s+/g, ' ')}${dot ? ' {hidden:' + dot + '}' : ''}`;
  const main = document.querySelector('main') ?? document.body;
  const txt = main.innerText.replace(/\s+/g, ' ');
  let tray = '';
  try { const s = await window.electron.mlxEngineActivity(); tray = `${s.engine}/${s.mode}${s.statusDetail ? ' ' + s.statusDetail.slice(0, 80) : ''}`; } catch { tray = 'n/a'; }
  return { bar, chip, tail: txt.slice(-220), tray };
});
let broke = null; let i = 0;
while ((Date.now() - t0) / 1000 < 240) {
  const s = await snap(); const t = ((Date.now() - t0) / 1000).toFixed(1);
  let event = '';
  // Break once the answer is being written: the chip's phase word says Writing (the transcript-length test
  // missed on 3.0.37 — a 220-char tail is under 40 words), and never before 8 s so the turn is under way.
  if (!broke && /Writing/.test(s.chip) && (Date.now() - t0) > 8000) {
    broke = Date.now();
    event = mode;
    // Fire and keep sampling: execSync froze the sampler ~4.5 s through the relaunch (osascript quit + sleep 3),
    // so the first seconds after the break — the ones this harness exists to see — had no rows (3.0.37/3.0.38).
    const cmd = mode === 'kill-link'
      ? `ssh workhorse 'kill $(pgrep -f "Goose Swarm.app/Contents/Resources/bin/tailscaled" | head -1)'`
      : `ssh workhorse 'osascript -e "quit app \\"Goose Swarm\\""; sleep 3; open -a "/Applications/Goose Swarm.app"'`;
    spawn('/bin/sh', ['-c', cmd], { stdio: 'ignore' });
  }
  appendFileSync(out, [t, event, s.bar, s.chip, s.tail, s.tray].join('\t') + '\n');
  if (broke && i % 2 === 0) await p.screenshot({ path: `${dir}/s-${String(i).padStart(3, '0')}.png` });
  if (broke && (Date.now() - broke) / 1000 > 60) break;
  i++; await p.waitForTimeout(1000);
}
await b.close();
