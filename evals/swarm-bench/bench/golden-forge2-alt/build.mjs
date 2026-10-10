// Builds each Custom UI resource into static/<name>/build/: index.html, index.js, index.css.
// No inline script or style, relative asset paths, production code (no eval) — the default Forge CSP.
// The widget and the sprint action are plain DOM code (§17 boot budget: 150 KB of JS and CSS before first paint);
// the widget's edit surface keeps v1's React.
import { build } from 'esbuild';
import { copyFileSync, existsSync, mkdirSync, rmSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';

const root = dirname(fileURLToPath(import.meta.url));
for (const name of ['widget', 'widget-edit', 'sprint']) {
  const out = join(root, 'static', name, 'build');
  const src = join(root, 'static', name, 'src');
  const entry = existsSync(join(src, 'index.js')) ? join(src, 'index.js') : join(src, 'index.jsx');
  rmSync(out, { recursive: true, force: true });
  mkdirSync(out, { recursive: true });
  await build({
    entryPoints: { index: entry },
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
