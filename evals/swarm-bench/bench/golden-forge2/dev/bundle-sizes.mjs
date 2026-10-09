// Prints what each Custom UI entry's bundle is made of (bytes per package), for the R9 boot budget.
// Usage: node dev/bundle-sizes.mjs <entry.jsx>...
import { build } from 'esbuild';
import { resolve } from 'node:path';

for (const entry of process.argv.slice(2)) {
  const r = await build({
    entryPoints: { index: resolve(entry) },
    bundle: true,
    write: false,
    outdir: '/nonexistent',
    format: 'iife',
    platform: 'browser',
    target: ['es2020'],
    jsx: 'automatic',
    minify: true,
    legalComments: 'none',
    define: { 'process.env.NODE_ENV': '"production"' },
    metafile: true,
    logLevel: 'error',
  });
  const out = Object.values(r.metafile.outputs).find((o) => o.entryPoint);
  const by = {};
  for (const [f, v] of Object.entries(out.inputs)) {
    const m = f.match(/node_modules\/((?:@[^/]+\/)?[^/]+)/);
    const k = m ? m[1] : 'app';
    by[k] = (by[k] ?? 0) + v.bytesInOutput;
  }
  const total = r.outputFiles.reduce((n, f) => n + f.contents.length, 0);
  console.log(`${entry}: ${total} bytes`);
  for (const [k, v] of Object.entries(by).sort((a, b) => b[1] - a[1])) console.log(`  ${k} ${v}`);
}
