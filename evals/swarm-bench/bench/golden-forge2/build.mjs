// Builds each Custom UI resource into static/<name>/build/: index.html, index.js, index.css, and for the
// widget and the sprint action app.js. index.js is the boot script (bridge + plain DOM: one invoke, the
// first data paint), which then loads app.js (React). No inline script or style, relative asset paths,
// production React (no eval) — the default Forge CSP.
import { build } from 'esbuild';
import { copyFileSync, mkdirSync, rmSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';

const root = dirname(fileURLToPath(import.meta.url));
const ENTRIES = {
  widget: { index: 'boot.js', app: 'index.jsx' },
  'widget-edit': { index: 'index.jsx' },
  sprint: { index: 'boot.js', app: 'index.jsx' },
};
for (const [name, entries] of Object.entries(ENTRIES)) {
  const out = join(root, 'static', name, 'build');
  rmSync(out, { recursive: true, force: true });
  mkdirSync(out, { recursive: true });
  await build({
    entryPoints: Object.fromEntries(Object.entries(entries).map(([out, file]) => [out, join(root, 'static', name, 'src', file)])),
    bundle: true,
    outdir: out,
    format: 'iife',
    platform: 'browser',
    target: ['es2020'],
    jsx: 'automatic',
    minify: true,
    legalComments: 'none',
    define: { 'process.env.NODE_ENV': '"production"' },
    logLevel: 'warning',
  });
  copyFileSync(join(root, 'static', name, 'index.html'), join(out, 'index.html'));
}
console.log('built static/{widget,widget-edit,sprint}/build');
