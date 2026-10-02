import { build } from 'esbuild';
import { mkdirSync, readFileSync, writeFileSync, rmSync } from 'node:fs';

const surfaces = { 'widget-view': 'view.css', 'widget-edit': 'edit.css', 'sprint-ledger': 'ledger.css' };
const base = readFileSync('static/shared/base.css', 'utf8');

for (const [name, css] of Object.entries(surfaces)) {
  const out = `static/${name}/build`;
  rmSync(out, { recursive: true, force: true });
  mkdirSync(out, { recursive: true });
  await build({
    entryPoints: [`static/${name}/src/main.ts`],
    outfile: `${out}/main.js`,
    bundle: true,
    format: 'iife',
    target: 'es2020',
    minify: true,
    legalComments: 'none',
    define: { 'process.env.NODE_ENV': '"production"' },
  });
  writeFileSync(`${out}/app.css`, base + '\n' + readFileSync(`static/${name}/src/${css}`, 'utf8'));
  writeFileSync(
    `${out}/index.html`,
    `<!doctype html>
<html lang="en">
  <head>
    <meta charset="utf-8" />
    <meta name="viewport" content="width=device-width, initial-scale=1" />
    <title>Scope ledger</title>
    <link rel="stylesheet" href="./app.css" />
  </head>
  <body>
    <div id="root"></div>
    <script src="./main.js"></script>
  </body>
</html>
`,
  );
}
console.log('built', Object.keys(surfaces).join(', '));
