#!/usr/bin/env node
'use strict';
// `npm run lint`: Forge's own client-side linter, offline. `forge lint` itself is login-gated twice
// (command.js checkAuthentication, and its default mode 'both' uploads the app to the server-side
// linter); its client-side half is @forge/lint 6.3.0's lint() with linter.mode 'client-side' — the same
// 16 linters the CLI runs locally. The PermissionLinter maps calls to scopes with the kit's pinned Jira /
// Confluence / Bitbucket OpenAPI files (USE_LOCAL_SWAGGER) instead of fetching them.
//
//   node $FORGE_KIT/bin/lint.cjs [--json] [appDir]       (cwd defaults to the app)
//
// --json prints one line: {counts, problems:[{sev,file,line,column,message,linter}], stageReached,
// stagesTotal, stages, statsig}. `stageReached` is the first manifest validation stage that failed in
// @forge/manifest's FullValidationProcessor order (later stages are unproven, never clean), or 'complete'.
// Exit 0 when there are no errors, 1 with errors, 2 when the linter itself crashed.
const fs = require('fs');
const os = require('os');
const path = require('path');

const args = process.argv.slice(2);
const json = args.includes('--json');
const appDir = path.resolve(args.find((a) => !a.startsWith('--')) ?? process.cwd());
const kitDir = process.env.FORGE_KIT || path.resolve(__dirname, '..');

// @forge/cli-shared constructs a `conf` store under the home directory; keep it inside the temp dir so the
// linter writes nothing outside the workspace sandbox's grants.
const home = path.join(os.tmpdir(), 'forge-lint-home');
fs.mkdirSync(home, { recursive: true });
process.env.HOME = home;
const openapi = (f) => path.join(kitDir, 'openapi', f);
Object.assign(process.env, {
  USE_LOCAL_SWAGGER: 'true', LOCAL_JIRA_SWAGGER: openapi('jira.json'), LOCAL_JSM_SWAGGER: openapi('jsm.json'),
  LOCAL_JSW_SWAGGER: openapi('jsw.json'), LOCAL_CONF_SWAGGER: openapi('conf.json'), LOCAL_CONF_V2_SWAGGER: openapi('confv2.json'),
  LOCAL_BB_SWAGGER: openapi('bb.json'),
});

const { kitPaths } = require(path.join(kitDir, 'lib', 'kitpaths.cjs'));
const paths = kitPaths(kitDir);
const req = (name) => require(require.resolve(name, { paths: [paths.lintModules] }));

const SOURCE_EXT = /\.(jsx?|tsx?)$/;
function walk(dir) {
  if (!fs.existsSync(dir)) return [];
  return fs.readdirSync(dir, { withFileTypes: true }).flatMap((e) => {
    if (e.name === 'node_modules' || e.name.startsWith('.')) return [];
    const p = path.join(dir, e.name);
    return e.isDirectory() ? walk(p) : SOURCE_EXT.test(e.name) ? [p] : [];
  });
}

async function manifestStages() {
  const { ProcessorBuilder, ValidationTypes } = req('@forge/manifest');
  const processor = ProcessorBuilder.instance().withValidation(ValidationTypes.FULL).withOptions({ deprecatedRuntimes: [] }).build();
  const stages = [];
  let manifestObject;
  for (const v of processor.validators ?? []) {
    const r = await v.validate(manifestObject);
    manifestObject = r.manifestObject || manifestObject;
    stages.push({ name: v.constructor.name, success: r.success, errors: (r.errors ?? []).filter((e) => e.level === 'error').length });
  }
  const first = stages.find((s) => !s.success || s.errors);
  return { stages, stageReached: first ? first.name : 'complete', stagesTotal: stages.length };
}

(async () => {
  process.chdir(appDir);
  const { lint, problemCount } = req('@forge/lint');
  const YAML = req('yaml');
  let manifest;
  try {
    manifest = YAML.parse(fs.readFileSync('manifest.yml', 'utf8'));
  } catch (e) {
    manifest = undefined;
  }
  const files = walk('src').map((f) => path.relative(appDir, f)).sort();
  const lines = [];
  const logger = { info: (m) => lines.push(String(m)), warn: (m) => lines.push(String(m)), error: (m) => lines.push(String(m)), debug() {}, trace() {} };
  const statsig = { getDeprecatedRuntimes: async () => [] };
  const results = await lint(files, manifest, 'development', logger, statsig, { linter: { mode: 'client-side' } });
  const counts = problemCount(results);
  const problems = results.flatMap((r) => [
    ...r.errors.map((e) => ({ sev: 'error', file: r.file, line: e.line, column: e.column, message: e.message, reference: e.reference ?? null })),
    ...r.warnings.map((w) => ({ sev: 'warning', file: r.file, line: w.line, column: w.column, message: w.message, reference: w.reference ?? null })),
  ]).sort((a, b) => `${a.file}\u0000${String(a.line).padStart(6, '0')}\u0000${a.message}`.localeCompare(`${b.file}\u0000${String(b.line).padStart(6, '0')}\u0000${b.message}`));
  const stages = await manifestStages();
  if (json) {
    process.stdout.write('LINT_JSON ' + JSON.stringify({ counts, problems, ...stages,
      statsig: 'offline stub: getDeprecatedRuntimes() -> [] (the schema runtime enum still applies)' }) + '\n');
  } else {
    const { reportLintResults } = req('@forge/lint');
    const out = [];
    reportLintResults({ info: (m) => out.push(String(m)), warn: (m) => out.push(String(m)), error: (m) => out.push(String(m)), debug() {} }, results);
    process.stdout.write(out.join('\n') + '\n');
    if (stages.stageReached !== 'complete') process.stdout.write(`(manifest validation stopped at ${stages.stageReached}: later checks run once it passes)\n`);
  }
  process.exitCode = counts.errors ? 1 : 0;
})().catch((e) => {
  process.stderr.write(`LINT_CRASH ${e?.stack ?? e}\n`);
  process.exit(2);
});
