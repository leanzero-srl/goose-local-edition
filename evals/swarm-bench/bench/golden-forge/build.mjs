// Builds each Custom UI resource into static/<name>/build/: index.html, index.js, index.css.
// No inline script or style, relative asset paths, production React (no eval) — the default Forge CSP.
import { build } from 'esbuild';
import { copyFileSync, mkdirSync, rmSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';

const root = dirname(fileURLToPath(import.meta.url));
for (const name of ['widget', 'widget-edit', 'sprint']) {
  const out = join(root, 'static', name, 'build');
  rmSync(out, { recursive: true, force: true });
  mkdirSync(out, { recursive: true });
  await build({
    entryPoints: { index: join(root, 'static', name, 'src', 'index.jsx') },
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
  copyFileSync(join(root, 'static', 'shared', 'index.html'), join(out, 'index.html'));
}
console.log('built static/{widget,widget-edit,sprint}/build');
