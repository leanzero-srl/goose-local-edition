// Proof 2: the REAL `forge lint` engine, offline and without credentials.
// `forge lint` the command is login-gated twice: command.js:309 checkAuthentication() ->
// credential-store.js:144 (FORGE_EMAIL+FORGE_API_TOKEN are verified over the network by
// userRepository.getUser), and its default mode 'both' adds ServerSideLinter, which zips and
// uploads the app. The client-side half is pure: @forge/lint's lint() with linter.mode
// 'client-side' runs the same 16 linters the CLI runs locally (lint.js:102-119), and its only
// remote input is statsigService.getDeprecatedRuntimes() (abstract-manifest-linter.js:49).
// Usage: node lint-offline.cjs <appDir>   -> prints the CLI's own report, JSON summary, exit 1 on errors.
const path = require('path');
const fs = require('fs');
const cliMods = path.join(__dirname, '..', 'node_modules', '@forge', 'cli', 'node_modules');
const { lint, reportLintResults, problemCount } = require(path.join(cliMods, '@forge', 'lint'));
const YAML = require(path.join(__dirname, '..', 'node_modules', 'yaml'));

(async () => {
  const appDir = path.resolve(process.argv[2]);
  process.chdir(appDir); // the manifest linters and tsconfig lookup read from cwd, as under the CLI
  const manifest = YAML.parse(fs.readFileSync('manifest.yml', 'utf8'));
  const walk = (d) => fs.readdirSync(d, { withFileTypes: true }).flatMap((e) => (e.name === 'node_modules' ? [] : e.isDirectory() ? walk(path.join(d, e.name)) : [path.join(d, e.name)]));
  const files = fs.existsSync('src') ? walk('src') : [];
  const logger = { info: (m) => console.log(m), warn: (m) => console.log(m), error: (m) => console.log(m), debug: () => {}, trace: () => {} };
  const statsig = { getDeprecatedRuntimes: async () => [] };
  const results = await lint(files, manifest, 'development', logger, statsig, { linter: { mode: 'client-side' } });
  reportLintResults(logger, results);
  const counts = problemCount(results);
  const flat = results.flatMap((r) => [...r.errors.map((e) => ({ sev: 'error', file: r.file, ...e })), ...r.warnings.map((w) => ({ sev: 'warning', file: r.file, ...w }))]);
  console.log('LINT_JSON ' + JSON.stringify({ counts, problems: flat.map(({ sev, file, line, column, message }) => ({ sev, file, line, column, message })) }));
  process.exit(counts.errors ? 1 : 0);
})().catch((e) => { console.error('LINT_CRASH', e.message); process.exit(2); });
