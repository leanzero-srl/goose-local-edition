// What the user SEES, second by second, while a break recovers (the owner, 2026-09-25: "what happens
// during these seconds? … it's a long time to wait 13s without having any clear visual cue").
// usage: node recovery.mjs <dir> <kill-link|relaunch-peer> — starts a long answer in a new chat, breaks
// the Studio once the answer is being written, then samples the chat screen every second for 60 s:
// the readiness bar, the model chip, the tail of the transcript, and main's engine snapshot (the tray).
import { chromium } from '/Users/mihaiperdum/Projects/goose/ui/node_modules/playwright-core/index.mjs';
import { execSync } from 'node:child_process';
import { writeFileSync, appendFileSync } from 'node:fs';
const [dir, mode] = process.argv.slice(2);
const b = await chromium.connectOverCDP('http://127.0.0.1:9333');
const p = b.contexts()[0].pages().find((x) => x.url().includes('index.html'));
await p.goto(p.url().split('#')[0] + '#/'); await p.waitForTimeout(2500);
await p.getByRole('button', { name: /^New session in / }).click(); await p.waitForTimeout(4000);
const input = p.locator('[data-testid=chat-input]:visible').first();
await input.click(); await input.fill('Write a 300-word story about a lighthouse keeper. No tools.');
await p.keyboard.press('Enter');
const t0 = Date.now(); const out = `${dir}/timeline.tsv`;
writeFileSync(out, 't_s\tevent\tbar\tchip\ttranscript_tail\ttray\n');
const snap = async () => p.evaluate(async () => {
  const bar = document.querySelector('[data-testid=composer-readiness]')?.innerText.replace(/\s+/g, ' ') ?? '';
  const chipEl = document.querySelector('[data-testid=model-chip], [data-testid=chat-served-chip]');
  const chip = (chipEl?.innerText ?? '').replace(/\s+/g, ' ');
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
  if (!broke && s.tail.split(' ').length > 40 && (Date.now() - t0) > 8000) {
    broke = Date.now();
    event = mode;
    if (mode === 'kill-link') execSync(`ssh workhorse 'kill $(pgrep -f "Goose Swarm.app/Contents/Resources/bin/tailscaled" | head -1)'`);
    else execSync(`ssh workhorse 'osascript -e "quit app \\"Goose Swarm\\""; sleep 3; open -a "/Applications/Goose Swarm.app"'`);
  }
  appendFileSync(out, [t, event, s.bar, s.chip, s.tail, s.tray].join('\t') + '\n');
  if (broke && i % 2 === 0) await p.screenshot({ path: `${dir}/s-${String(i).padStart(3, '0')}.png` });
  if (broke && (Date.now() - broke) / 1000 > 60) break;
  i++; await p.waitForTimeout(1000);
}
await b.close();
