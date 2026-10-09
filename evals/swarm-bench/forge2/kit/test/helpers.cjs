'use strict';
// Shared test plumbing: the materialised kit (bench/forge_kit.py ensure), Playwright, a scratch dir.
const fs = require('fs');
const os = require('os');
const path = require('path');
const { execFileSync, execSync } = require('child_process');
const { createRequire } = require('module');

const REPO = path.resolve(__dirname, '..', '..', '..');          // evals/swarm-bench
const FORGE = path.join(REPO, 'forge');
let kit = null;

function ensureKit() {
  if (kit) return kit;
  const out = execFileSync('python3', [path.join(REPO, 'bench', 'forge_kit.py'), 'ensure'], { encoding: 'utf8' });
  kit = JSON.parse(out);
  process.env.FORGE_KIT_MODULES = kit.modules_dir;
  return kit;
}

function playwright() {
  const tries = [
    () => (process.env.BENCH_BROWSER_MODULE ? require(process.env.BENCH_BROWSER_MODULE) : null),
    () => require('playwright'),
    () => createRequire(path.join(execSync('npm root -g', { encoding: 'utf8' }).trim(), 'x.js'))('playwright'),
    () => createRequire(path.join(path.dirname(process.execPath), '..', 'lib', 'node_modules', 'x.js'))('playwright'),
  ];
  for (const t of tries) { try { const p = t(); if (p) return p; } catch { /* next */ } }
  throw new Error('playwright is not resolvable from this node');
}

function scratch(name) {
  return fs.mkdtempSync(path.join(os.tmpdir(), `forge-test-${name}-`));
}

function copyDir(src, dst, skip = new Set(['node_modules'])) {
  fs.mkdirSync(dst, { recursive: true });
  for (const e of fs.readdirSync(src, { withFileTypes: true })) {
    if (skip.has(e.name)) continue;
    const s = path.join(src, e.name);
    const d = path.join(dst, e.name);
    if (e.isDirectory()) copyDir(s, d, skip); else if (e.isFile()) fs.copyFileSync(s, d);
  }
}

module.exports = { REPO, FORGE, ensureKit, playwright, scratch, copyDir };
