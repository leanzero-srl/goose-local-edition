// The Forge emulator driver: bundle the app the way `forge deploy` would (one CJS file per
// handler module, @forge/* bundled in), then invoke any manifest `function` by key with the event
// shape of the module that references it. Tier A = runtime-shim.cjs in-process; tier B = the real
// Forge node runtime wrapper (vendor/wrapper.js) in a forked child, exactly as `forge tunnel` runs it.
const path = require('path');
const fs = require('fs');
const crypto = require('crypto');
const { fork } = require('child_process');
const YAML = require('yaml');
const esbuild = require('esbuild');

const CLOUD_ID = '11111111-2222-3333-4444-555555555555';
const CONTEXT_ARI = `ari:cloud:jira::site/${CLOUD_ID}`;

function loadManifest(appDir) {
  return YAML.parse(fs.readFileSync(path.join(appDir, 'manifest.yml'), 'utf8'));
}

// Which module references which function, so the event shape follows the module type.
function functionUsers(manifest) {
  const users = {};
  for (const [type, entries] of Object.entries(manifest.modules ?? {})) {
    if (type === 'function') continue;
    for (const e of entries) {
      const fns = [e.function, e.resolver?.function, e.handler?.function].filter(Boolean);
      for (const f of fns) (users[f] ??= []).push({ type, key: e.key, module: e });
    }
  }
  return users;
}

async function bundle(appDir, outDir, manifest) {
  fs.mkdirSync(outDir, { recursive: true });
  const files = new Set(manifest.modules.function.map((f) => f.handler.split('.')[0]));
  for (const f of files) {
    const entry = ['.ts', '.tsx', '.js', '.jsx', '.mjs'].map((x) => path.join(appDir, 'src', f + x)).find(fs.existsSync);
    if (!entry) throw new Error(`manifest handler file src/${f}.* does not exist`);
    await esbuild.build({ entryPoints: [entry], bundle: true, platform: 'node', format: 'cjs', target: 'node22', outfile: path.join(outDir, `${f}.cjs`), logLevel: 'error', nodePaths: [path.join(__dirname, '..', 'node_modules')] });
  }
  return outDir;
}

function appContext(manifest, moduleKey, functionKey, invocationId) {
  return {
    appId: manifest.app.id.split('/').pop(), environmentId: 'emulator-env', environmentType: 'DEVELOPMENT', appVersion: '1.0.0',
    invocationId, installationId: 'emulator-install', moduleKey, functionKey, contextAri: CONTEXT_ARI,
    installationSummary: { id: 'emulator-install', primaryInstallationContext: CONTEXT_ARI, secondaryInstallationContexts: [] },
  };
}

// Tier A: in-process.
async function invokeA({ bundleDir, manifest, proxyUrl }, functionKey, moduleKey, event, requestContext) {
  const { installShim, runInInvocation } = require('./runtime-shim.cjs');
  if (!global.__forge_fetch__) installShim({ proxyUrl });
  const [file, exportName] = manifest.modules.function.find((f) => f.key === functionKey).handler.split('.');
  const mod = require(path.join(bundleDir, `${file}.cjs`));
  if (typeof mod[exportName] !== 'function') throw new Error(`handler ${file}.${exportName} is not an exported function`);
  return runInInvocation(appContext(manifest, moduleKey, functionKey, crypto.randomUUID()), () => mod[exportName](event, requestContext));
}

// Tier B: the real wrapper. The proxy token must be a JWT whose payload carries `exp`; the wrapper
// only DECODES it (TimedProxyInfo), it never verifies a signature.
function fakeJwt() {
  const b = (o) => Buffer.from(JSON.stringify(o)).toString('base64url');
  return `${b({ alg: 'none', typ: 'JWT' })}.${b({ exp: Math.floor(Date.now() / 1000) + 3600 })}.`;
}

function prepareRealRuntimeDir(bundleDir) {
  const vendor = path.join(__dirname, '..', 'vendor');
  fs.copyFileSync(path.join(vendor, 'loader.js'), path.join(bundleDir, '__forge__.cjs'));
  fs.copyFileSync(path.join(vendor, 'wrapper.js'), path.join(bundleDir, '__forge_wrapper__.cjs'));
}

function invokeB({ bundleDir, manifest, proxyUrl }, functionKey, moduleKey, event) {
  const handler = manifest.modules.function.find((f) => f.key === functionKey).handler;
  const child = fork(path.join(__dirname, 'real-wrapper-runner.cjs'), [path.join(bundleDir, '__forge__.cjs')], {
    stdio: ['ignore', 'pipe', 'inherit', 'ipc'],
    env: { _HANDLER: '__forge__.main', FORGE_EFS_RUNTIME_PATH: bundleDir, FORGE_CUSTOM_WRAPPER_FILE_NAME: '__forge_wrapper__.cjs', LAMBDA_TASK_ROOT: bundleDir },
  });
  const logs = [];
  child.stdout.on('data', (d) => logs.push(...d.toString().trim().split('\n')));
  const lambdaEvent = {
    body: event,
    handler,
    variables: [],
    _meta: {
      proxy: { url: proxyUrl, token: fakeJwt(), host: '127.0.0.1' },
      contextAri: CONTEXT_ARI,
      appContext: appContext(manifest, moduleKey, functionKey, crypto.randomUUID()),
      tracing: { traceId: crypto.randomBytes(8).toString('hex'), spanId: crypto.randomBytes(8).toString('hex') },
      aaid: '5b10ac8d82e05b22cc7d4ef5',
      timeout: 25,
    },
  };
  return new Promise((resolve, reject) => {
    child.on('message', (m) => { child.kill(); resolve({ ...m.result, logs }); });
    child.on('exit', (code) => code && reject(new Error(`real wrapper child exited ${code}`)));
    child.send({ lambdaEvent, deadline: Date.now() + 25_000 });
  });
}

module.exports = { loadManifest, functionUsers, bundle, invokeA, invokeB, prepareRealRuntimeDir, CLOUD_ID, CONTEXT_ARI };
