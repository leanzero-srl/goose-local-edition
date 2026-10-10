'use strict';
// Bundling and invocation (DESIGN.md §6.1).
//  - bundle: esbuild per handler file under src/, platform node, format cjs, target node22. Handler
//    `<module>.<fn>` follows @forge/cli-shared ConfigFile.getAppHandlers (exactly two dot parts, so
//    `src/index.fn` is REJECTED there: "handlerFormat") and @forge/bundler's entry rule
//    path.resolve('src', module) with webpack's extensions ['.ts','.tsx','.js','.jsx','.json','.cjs'].
//    Bare imports from app code resolve ONLY from the pristine kit modules, never the workdir's.
//  - invoke: one fresh Node process per invocation running Atlassian's wrapper (runtime 'wrapper',
//    sha-verified against runtime-pin.json) or the in-repo shim (runtime 'shim': unpublishable).
//    The process runs under a deny-default sandbox-exec profile (fence 'sandbox') with the virtual-time
//    agent preloaded (clock.cjs): its clock starts at `vStart`, the module's limit is enforced on VIRTUAL
//    elapsed time inside the process, and the final clock comes back on fd 4. The module's limit in REAL
//    seconds stays as a guard for a process that never yields (CPU-bound or hung): it is killed by pid (gate 4:
//    never a process group) and reported as `realTimeout`, distinct from a virtual-time kill.
const fs = require('fs');
const path = require('path');
const crypto = require('crypto');
const { spawn, spawnSync } = require('child_process');
const { builtinModules } = require('module');

const ENTRY_EXTENSIONS = ['.ts', '.tsx', '.js', '.jsx', '.json', '.cjs'];
const sha256 = (buf) => crypto.createHash('sha256').update(buf).digest('hex');

function parseHandler(handler) {
  const parts = String(handler).split('.');
  if (parts.length !== 2 || !parts[0] || !parts[1]) {
    return { error: `Handler "${handler}" must be <file>.<exportedFunction> (exactly one dot, as @forge/cli-shared requires)` };
  }
  return { module: parts[0], fn: parts[1] };
}

function verifyWrapper(paths) {
  const pin = paths.pin;
  const w = path.join(paths.wrapperDir, 'wrapper.js');
  const l = path.join(paths.wrapperDir, 'loader.js');
  if (!fs.existsSync(w) || !fs.existsSync(l)) throw new Error(`REFUSED: the pinned runtime wrapper is not materialised under ${paths.wrapperDir}; run forge_kit.py ensure`);
  const ws = sha256(fs.readFileSync(w));
  const ls = sha256(fs.readFileSync(l));
  if (ws !== pin.sha256 || ls !== pin.loader_sha256) throw new Error(`REFUSED: wrapper sha256 ${ws} / loader ${ls} do not match runtime-pin.json (${pin.sha256} / ${pin.loader_sha256})`);
  return { wrapperSha256: ws, loaderSha256: ls, url: pin.url };
}

function kitModulesPlugin(paths) {
  const builtins = new Set([...builtinModules, ...builtinModules.map((m) => `node:${m}`)]);
  return {
    name: 'forge-kit-modules',
    setup(build) {
      build.onResolve({ filter: /^[^./]/ }, async (args) => {
        if (args.pluginData?.kit || builtins.has(args.path) || args.path.startsWith('node:')) return undefined;
        if (args.importer && fs.realpathSync(args.importer).startsWith(fs.realpathSync(paths.appModules))) return undefined;
        const r = await build.resolve(args.path, { kind: args.kind, resolveDir: path.dirname(paths.appModules), pluginData: { kit: true } });
        if (r.errors.length) return { errors: [{ text: `Could not resolve "${args.path}": it is not one of the installed packages` }] };
        return r;
      });
    },
  };
}

async function bundle({ appDir, outDir, manifest, paths }) {
  const esbuild = paths.require('esbuild');
  fs.mkdirSync(outDir, { recursive: true });
  const files = {};
  for (const f of manifest?.modules?.function ?? []) {
    const h = parseHandler(f.handler);
    if (h.error || files[h.module]) continue;
    const entry = `./src/${h.module}`;
    const out = path.join(outDir, `${h.module}.cjs`);
    try {
      const r = await esbuild.build({
        absWorkingDir: appDir, entryPoints: [entry], bundle: true, platform: 'node', format: 'cjs', target: 'node22',
        outfile: out, resolveExtensions: ENTRY_EXTENSIONS, metafile: true, logLevel: 'silent', write: true,
        plugins: [kitModulesPlugin(paths)],
      });
      files[h.module] = { bundled: true, out, metafile: r.metafile, error: null };
    } catch (e) {
      const msg = (e.errors ?? []).map((x) => `${x.location ? `${x.location.file}:${x.location.line}: ` : ''}${x.text}`).join('\n') || String(e.message);
      files[h.module] = { bundled: false, out: null, metafile: null, error: msg };
    }
  }
  return files;
}

function prepareBundleDir(outDir, paths, runtime) {
  fs.copyFileSync(path.join(__dirname, 'runner.cjs'), path.join(outDir, '__forge_runner__.cjs'));
  fs.copyFileSync(path.join(__dirname, 'clock.cjs'), path.join(outDir, '__forge_clock__.cjs'));
  if (runtime === 'wrapper') {
    fs.copyFileSync(path.join(paths.wrapperDir, 'loader.js'), path.join(outDir, '__forge__.cjs'));
    fs.copyFileSync(path.join(paths.wrapperDir, 'wrapper.js'), path.join(outDir, '__forge_wrapper__.cjs'));
  } else {
    fs.copyFileSync(path.join(__dirname, 'shim.cjs'), path.join(outDir, '__forge_shim__.cjs'));
  }
}

// Deny-default profile for ONE invocation process (measured 2026-10-02 on Node 22.22.0: a read outside
// the grant -> EPERM, child_process -> EPERM, the proxy port -> 200, another 127.0.0.1 port -> EPERM,
// https://example.com -> ENOTFOUND). The literal "/" read is what Node's startup needs.
function sandboxProfile({ bundleDir, proxyPort, node = process.execPath }) {
  const q = (p) => JSON.stringify(fs.realpathSync(p));
  const nodeRoot = path.dirname(path.dirname(fs.realpathSync(node)));
  return [
    '(version 1)', '(deny default)',
    `(allow process-exec (literal ${q(node)}))`,
    `(allow file-read* (subpath "/usr/lib") (subpath "/System") (subpath ${q(nodeRoot)}) (subpath ${q(bundleDir)}) (literal "/dev/urandom") (literal "/dev/null") (literal "/"))`,
    '(allow file-read-metadata)', '(allow sysctl-read)', '(allow mach-lookup)', '(allow ipc-posix-shm)', '(allow system-socket)',
    '(allow signal (target self))',
    `(allow network-outbound (remote ip "localhost:${proxyPort}"))`,
  ].join('\n');
}

let fenceProbe = null;
// 'sandbox' when sandbox-exec can apply a profile here. Inside the entrant's workspace sandbox it cannot
// (measured: "sandbox_apply: Operation not permitted", exit 71 — macOS refuses nested profiles).
function sandboxAvailable() {
  if (fenceProbe !== null) return fenceProbe;
  if (process.platform !== 'darwin' || !fs.existsSync('/usr/bin/sandbox-exec')) return (fenceProbe = false);
  const r = spawnSync('/usr/bin/sandbox-exec', ['-p', '(version 1)(allow default)', '/usr/bin/true'], { encoding: 'utf8' });
  return (fenceProbe = r.status === 0);
}

function fakeJwt(claims) {
  const b = (o) => Buffer.from(JSON.stringify(o)).toString('base64url');
  return `${b({ alg: 'none', typ: 'JWT' })}.${b(claims)}.`;
}

// -> {result, logs, stderr, ms, timedOut, killedAtLimit, realTimeout, vEnd, crash}
//    vEnd: the invocation's virtual clock when it ended (null when the process died without reporting it).
function runInvocation({ bundleDir, lambdaEvent, timeoutSec, vStart, runtime, fence, proxyPort, node = process.execPath, extra = {} }) {
  if (!Number.isFinite(vStart)) throw new Error(`REFUSED: runInvocation needs the invocation's virtual start time (vStart), got ${vStart}`);
  bundleDir = fs.realpathSync(bundleDir);
  const runner = path.join(bundleDir, '__forge_runner__.cjs');
  const agent = ['--require', path.join(bundleDir, '__forge_clock__.cjs')];
  let cmd;
  let args;
  if (fence === 'sandbox') {
    cmd = '/usr/bin/sandbox-exec';
    args = ['-p', sandboxProfile({ bundleDir, proxyPort, node }), node, ...agent, runner];
  } else if (fence === 'node-permission') {
    cmd = node;
    // Dev only (nested sandbox-exec is refused inside the workspace sandbox): Node's permission model
    // keeps file reads to the bundle and forbids child processes; it does not fence the network.
    args = ['--permission', `--allow-fs-read=${bundleDir}`, ...agent, runner];
  } else {
    throw new Error(`REFUSED: unknown fence ${fence}`);
  }
  const deadline = vStart + timeoutSec * 1000;
  const env = { PATH: '/usr/bin:/bin', TZ: 'UTC', LANG: 'C', _HANDLER: '__forge__.main', LAMBDA_TASK_ROOT: bundleDir,
    FORGE_EFS_RUNTIME_PATH: bundleDir, FORGE_CUSTOM_WRAPPER_FILE_NAME: '__forge_wrapper__.cjs',
    FORGE_VCLOCK_START: String(vStart), FORGE_VCLOCK_DEADLINE: String(deadline) };
  const started = Date.now();
  return new Promise((resolve) => {
    const child = spawn(cmd, args, { cwd: bundleDir, env, stdio: ['pipe', 'pipe', 'pipe', 'pipe', 'pipe'] });
    let out = '';
    let err = '';
    let res = '';
    let vclock = '';
    let realTimeout = false;
    child.stdout.on('data', (d) => (out += d));
    child.stderr.on('data', (d) => (err += d));
    child.stdio[3].on('data', (d) => (res += d));
    child.stdio[4].on('data', (d) => (vclock += d));
    const guard = setTimeout(() => { realTimeout = true; process.kill(child.pid, 'SIGKILL'); }, timeoutSec * 1000);
    child.on('close', () => {
      clearTimeout(guard);
      const reports = vclock.split('\n').filter(Boolean).map((line) => { try { return JSON.parse(line); } catch { return null; } }).filter(Boolean);
      const last = reports[reports.length - 1] ?? null;
      const killedAtLimit = reports.some((r) => r.killed === 'limit');
      const timedOut = killedAtLimit || realTimeout;
      let parsed = null;
      try { parsed = res ? JSON.parse(res) : null; } catch { parsed = { crash: { message: `unparseable runner output: ${res.slice(0, 200)}` } }; }
      const logs = out.split('\n').filter(Boolean).map((line) => { try { return JSON.parse(line); } catch { return { raw: line }; } });
      const why = killedAtLimit ? `killed at the ${timeoutSec} s limit (virtual time)` : realTimeout ? `killed after ${timeoutSec} s of real time without finishing (CPU-bound or hung)` : null;
      resolve({ result: timedOut ? null : parsed?.result ?? null,
        crash: timedOut ? { message: why } : parsed?.crash ?? (parsed ? null : { message: `invocation process ended without a result: ${err.slice(-400)}` }),
        logs, stderr: err, ms: Date.now() - started, timedOut, killedAtLimit, realTimeout,
        vEnd: killedAtLimit || realTimeout ? deadline : Number.isFinite(last?.vnow) ? last.vnow : null });
    });
    // clockOffsetMs 0: the agent owns `Date`; the runner's own clock shift must stay off.
    child.stdin.end(JSON.stringify({ mode: runtime, lambdaEvent, deadline, clockOffsetMs: 0, ...extra }));
  });
}

module.exports = { bundle, prepareBundleDir, verifyWrapper, sandboxProfile, sandboxAvailable, runInvocation, parseHandler, fakeJwt, ENTRY_EXTENSIONS };
