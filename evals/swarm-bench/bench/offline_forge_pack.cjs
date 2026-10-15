'use strict';

const fs = require('node:fs');
const path = require('node:path');
const crypto = require('node:crypto');
const vm = require('node:vm');

const DATA_MODULES = new Set(['fixtures.cjs', 'rng.cjs', 'limits.cjs', 'world.cjs', 'llm.cjs', 'request-time.cjs']);
const digest = bytes => crypto.createHash('sha256').update(bytes).digest('hex');

function demand(condition, code, detail) {
  if (!condition) throw Object.assign(new Error(`${code}: ${detail}`), { code });
}

function sourceSnapshot(site, expected) {
  const sources = new Map();
  for (const name of DATA_MODULES) {
    const file = path.join(site, name);
    demand(Object.hasOwn(expected, name), 'UNPINNED_FIXTURE_MODULE', name);
    const stat = fs.lstatSync(file);
    demand(stat.isFile() && !stat.isSymbolicLink(), 'NONREGULAR_FIXTURE_MODULE', name);
    const bytes = fs.readFileSync(file);
    demand(digest(bytes) === expected[name], 'FIXTURE_SOURCE_PIN_MISMATCH', name);
    sources.set(name, bytes);
  }
  return sources;
}

function dataPack(sources, seed) {
  const cache = new Map(), imports = [];
  const deny = operation => { throw Object.assign(new Error(`FORBIDDEN_FIXTURE_OPERATION: ${operation}`), { code: 'FORBIDDEN_FIXTURE_OPERATION' }); };
  const filesystem = new Proxy(Object.create(null), { get: (_, name) => () => deny(`fs.${String(name)}`) });
  function load(name) {
    demand(DATA_MODULES.has(name), 'FORBIDDEN_FIXTURE_IMPORT', name);
    if (cache.has(name)) return cache.get(name).exports;
    const module = { exports: {} };
    cache.set(name, module);
    function restrictedRequire(request) {
      imports.push({ from: name, request });
      if (request === 'fs') return filesystem;
      if (request === 'crypto') return Object.freeze({ createHash: crypto.createHash });
      demand(request.startsWith('./') && DATA_MODULES.has(request.slice(2)), 'FORBIDDEN_FIXTURE_IMPORT', request);
      return load(request.slice(2));
    }
    const context = vm.createContext(Object.assign(Object.create(null), { module, exports: module.exports, require: restrictedRequire }));
    const script = new vm.Script(`(function(module,exports,require){\n${sources.get(name).toString('utf8')}\n})`, { filename: name });
    script.runInContext(context)(module, module.exports, restrictedRequire);
    return module.exports;
  }
  const fixture = load('fixtures.cjs');
  demand(typeof fixture.worldPack === 'function', 'MISSING_WORLD_PACK_EXPORT', 'fixtures.cjs');
  const pack = fixture.worldPack(seed, { scoring: true });
  return { json: JSON.stringify(pack) + '\n', imports };
}

function reconstruct({ site, expectedSiteHashes, seed, output }) {
  demand(/^[0-9a-f]{16}$/.test(seed), 'INVALID_ORIGINAL_SEED', String(seed));
  demand(!fs.existsSync(output), 'OUTPUT_ALREADY_EXISTS', output);
  const before = sourceSnapshot(site, expectedSiteHashes);
  const first = dataPack(before, seed), second = dataPack(before, seed);
  demand(first.json === second.json, 'NONDETERMINISTIC_FIXTURE_DATA', seed);
  const after = sourceSnapshot(site, expectedSiteHashes);
  for (const name of DATA_MODULES) demand(before.get(name).equals(after.get(name)), 'FIXTURE_SOURCE_CHANGED', name);
  const packBytes = Buffer.from(first.json);
  fs.writeFileSync(output, packBytes, { flag: 'wx', mode: 0o600 });
  const proof = {
    schema: 'forge-offline-pure-fixture-data-v1', seed, output,
    bytes: packBytes.length, sha256: digest(packBytes),
    sourcePins: Object.fromEntries([...before].map(([name, bytes]) => [name, digest(bytes)])),
    originalSourceVerifiedBeforeAndAfter: true, twoIndependentDataOnlyReconstructionsByteEqual: true,
    imports: first.imports, candidateImports: 0, probeImports: 0, childProcesses: 0, networkCalls: 0,
    moduleFilesystemOperationsAllowed: false,
    scope: 'Only exact pinned frozen fixture-data modules execute; no candidate app, browser, probe, scorer or model executes.',
  };
  fs.writeFileSync(output + '.proof.json', JSON.stringify(proof, null, 2) + '\n', { flag: 'wx', mode: 0o600 });
  return proof;
}

if (require.main === module) {
  const args = process.argv.slice(2), value = name => {
    const index = args.indexOf(name);
    demand(index >= 0 && index + 1 < args.length, 'MISSING_ARGUMENT', name);
    return args[index + 1];
  };
  try {
    const expected = JSON.parse(fs.readFileSync(value('--canonical'), 'utf8')).evaluation_inputs.site;
    console.log(JSON.stringify(reconstruct({ site: value('--site'), expectedSiteHashes: expected, seed: value('--seed'), output: value('--out') })));
  } catch (error) {
    console.error(JSON.stringify({ status: 'PURE_FIXTURE_DATA_REFUSED', code: error.code || 'FIXTURE_DATA_FAILURE', message: error.message }));
    process.exitCode = 1;
  }
}

module.exports = { reconstruct, dataPack, DATA_MODULES };
