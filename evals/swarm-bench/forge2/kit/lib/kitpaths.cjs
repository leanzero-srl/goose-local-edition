'use strict';
// Where the kit's materialised pieces live. An assembled kit ($FORGE_KIT, built by
// bench/forge_kit.py ensure()) holds bin/ lib/ openapi/ runtime-pin.json beside app-modules/,
// lint-modules/, wrapper/ and schema/. Running these libs straight from the repo (harness development
// and forge/kit/test) needs FORGE_KIT_MODULES=<the module cache dir that ensure() printed>.
const fs = require('fs');
const path = require('path');

function kitPaths(kitDir = process.env.FORGE_KIT || path.resolve(__dirname, '..')) {
  const own = fs.existsSync(path.join(kitDir, 'app-modules')) ? kitDir : process.env.FORGE_KIT_MODULES;
  if (!own || !fs.existsSync(path.join(own, 'app-modules', 'node_modules'))) {
    throw new Error(`REFUSED: no materialised kit modules beside ${kitDir} and FORGE_KIT_MODULES is ${process.env.FORGE_KIT_MODULES ?? 'unset'}; `
      + 'run `python3 evals/swarm-bench/bench/forge_kit.py ensure` first');
  }
  const p = {
    kitDir,
    modulesRoot: own,
    appModules: path.join(own, 'app-modules', 'node_modules'),
    lintModules: path.join(own, 'lint-modules', 'node_modules'),
    wrapperDir: path.join(own, 'wrapper'),
    schema: path.join(own, 'schema', 'manifest-schema.json'),
    openapiDir: path.join(kitDir, 'openapi'),
    lib: path.join(kitDir, 'lib'),
    pin: JSON.parse(fs.readFileSync(path.join(kitDir, 'runtime-pin.json'), 'utf8')),
  };
  if (!fs.existsSync(p.openapiDir)) p.openapiDir = path.join(__dirname, '..', 'openapi');
  p.require = (name) => require(require.resolve(name, { paths: [p.appModules, p.lintModules] }));
  p.resolve = (name) => require.resolve(name, { paths: [p.appModules, p.lintModules] });
  return p;
}

module.exports = { kitPaths };
