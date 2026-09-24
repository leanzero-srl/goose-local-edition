// Hermetic end-to-end proof of the Link launch reconnect, against REAL processes:
// a throwaway Headscale on loopback, a fake auth worker that mints real preauth keys on
// it, the real `goose serve` binary, and the real goose-owned userspace tailscaled.
//
// Everything lives under one temp root: goosed runs with HOME=<root>/h, so its identity,
// intent and mesh state are <root>/h/.leanzero/…, its tailscaled socket is inside that
// dir, and nothing reads or writes the owner's ~/.leanzero. The owner's PERSONAL
// Tailscale is only ever READ (`tailscale status --json` before and after), and the run
// fails if any identity field moved.
//
// Phases (each asserted, receipts printed):
//   1. a signed-in node that was never connected: the launch does NOT connect it;
//   2. the user's Connect → Connected, intent `connected` on disk;
//   3. goosed stopped the way the desktop stops it (SIGTERM → teardown), relaunched,
//      an ACP connection opened (what the desktop does at launch) — reconnected with no
//      user action;
//   3b. a SECOND goosed on the same account/home while the first holds the mesh (the
//      desktop runs one goosed per window): its launch leaves the mesh to the first —
//      `skipped`, no join key minted, no daemon spawned, the first's daemon untouched
//      when the second exits;
//   4. goosed SIGKILLed (a crash: its tailscaled survives in its own process group) —
//      the relaunch reports a NAMED reconnect failure naming the orphan's pid, before
//      any join key is minted; the orphan is stopped per-pid and Retry (Connect) brings
//      the mesh back;
//   5. the user's Disconnect → relaunch → stays disconnected, no daemon, no join key.
//
// Run (see the crate README section / the commit for the exact command):
//   GOOSE_BIN=target/debug/goose HEADSCALE_BIN=/path/headscale \
//   LEANZERO_TAILSCALED=/opt/homebrew/bin/tailscaled \
//   LEANZERO_TAILSCALE_CLI=/opt/homebrew/bin/tailscale \
//   node crates/leanzero-link/tests/e2e/relaunch_reconnect.mjs

import { execFileSync, spawn } from 'node:child_process';
import crypto from 'node:crypto';
import fs from 'node:fs';
import http from 'node:http';
import net from 'node:net';
import os from 'node:os';
import path from 'node:path';

const need = (name) => {
  const value = process.env[name];
  if (!value) throw new Error(`${name} must be set`);
  if (!fs.existsSync(value)) throw new Error(`${name}=${value} does not exist`);
  return path.resolve(value);
};
const GOOSE = need('GOOSE_BIN');
const HEADSCALE = need('HEADSCALE_BIN');
const TAILSCALED = need('LEANZERO_TAILSCALED');
const TAILSCALE = need('LEANZERO_TAILSCALE_CLI');
// The owner's personal daemon answers on the CLI's DEFAULT socket; it is only read.
const PERSONAL_CLI = process.env.PERSONAL_TAILSCALE_CLI ?? '/opt/homebrew/bin/tailscale';

// Harness waits bound the TEST, never product behaviour.
const WAIT_MS = 120_000;
const POLL_MS = 500;

const root = fs.mkdtempSync(path.join(os.tmpdir(), 'lzrc.'));
const home = path.join(root, 'h');
const leanzero = path.join(home, '.leanzero');
const stateDir = path.join(leanzero, 'tailscale');
const hsDir = path.join(root, 'hs');
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
const log = (...a) => console.log(`[e2e ${new Date().toISOString().slice(11, 19)}]`, ...a);
const receipts = [];
const receipt = (text) => {
  receipts.push(text);
  log(`RECEIPT ${text}`);
};
function assert(cond, message) {
  if (!cond) throw new Error(`ASSERTION FAILED: ${message}`);
}

async function freePort() {
  return await new Promise((resolve, reject) => {
    const srv = net.createServer();
    srv.once('error', reject);
    srv.listen(0, '127.0.0.1', () => {
      const { port } = srv.address();
      srv.close(() => resolve(port));
    });
  });
}

async function waitFor(what, probe) {
  const deadline = Date.now() + WAIT_MS;
  let last;
  while (Date.now() < deadline) {
    try {
      last = await probe();
      if (last) return last;
    } catch (e) {
      last = e;
    }
    await sleep(POLL_MS);
  }
  throw new Error(`timed out waiting for ${what}; last: ${last instanceof Error ? last.message : JSON.stringify(last)}`);
}

// ── the owner's personal Tailscale: identity fields only (traffic may move) ─────────
function personalIdentity() {
  let raw;
  try {
    raw = execFileSync(PERSONAL_CLI, ['status', '--json'], { encoding: 'utf8', timeout: 15_000 });
  } catch (e) {
    return { unreadable: String(e.stderr ?? e.message).trim() };
  }
  const s = JSON.parse(raw);
  const peers = Object.values(s.Peer ?? {})
    .map((p) => `${p.ID}|${p.PublicKey}|${p.HostName}|${(p.TailscaleIPs ?? []).join(',')}`)
    .sort();
  return {
    backendState: s.BackendState,
    self: s.Self && {
      id: s.Self.ID,
      publicKey: s.Self.PublicKey,
      hostName: s.Self.HostName,
      dnsName: s.Self.DNSName,
      ips: s.Self.TailscaleIPs,
      userId: s.Self.UserID,
    },
    tailnet: s.CurrentTailnet?.Name,
    magicDnsSuffix: s.MagicDNSSuffix,
    peers,
  };
}

// ── throwaway Headscale ────────────────────────────────────────────────────────────
const hsPort = await freePort();
const hsCfg = path.join(hsDir, 'config.yaml');
const loginServer = `http://127.0.0.1:${hsPort}`;
function headscale(...args) {
  return execFileSync(HEADSCALE, ['-c', hsCfg, ...args], { encoding: 'utf8', timeout: 30_000 });
}

// ── fake auth worker: real preauth keys from the throwaway Headscale ─────────────────
const ACCOUNT_EMAIL = 'e2e@leanzero.test';
const ACCOUNT_TOKEN = `e2e-token-${crypto.randomBytes(8).toString('hex')}`;
const NODE_SECRET = crypto.randomBytes(32).toString('hex');
let hsUserId;
let joinKeysMinted = 0;
const worker = http.createServer((req, res) => {
  const send = (status, body) => {
    res.writeHead(status, { 'content-type': 'application/json' });
    res.end(JSON.stringify(body));
  };
  if (req.method === 'GET' && req.url === '/v1/health') {
    return send(200, { ok: true, version: 'e2e', capabilities: { mail: false, audience: false, mesh: true } });
  }
  if (req.method === 'POST' && req.url === '/v1/mesh/join-key') {
    if (req.headers.authorization !== `Bearer ${ACCOUNT_TOKEN}`) {
      return send(401, { error: 'invalid token', reason: 'bad_signature' });
    }
    const key = JSON.parse(
      headscale('preauthkeys', 'create', '-u', String(hsUserId), '--ephemeral', '-e', '1h', '-o', 'json')
    );
    joinKeysMinted += 1;
    return send(200, {
      authKey: key.key,
      loginServer,
      nodeSecret: NODE_SECRET,
      expirySeconds: 3600,
    });
  }
  send(404, { error: 'not found' });
});

// ── goosed ──────────────────────────────────────────────────────────────────────────
const SERVE_SECRET = crypto.randomBytes(16).toString('hex');
let workerUrl;
let controlPort;
let launch = 0;
const liveGoosed = new Set();

// goose serve writes its log under $HOME/.local/state/goose/logs; every launch's file.
function goosedLogText() {
  const dir = path.join(home, '.local', 'state', 'goose', 'logs');
  if (!fs.existsSync(dir)) return '';
  return fs
    .readdirSync(dir, { recursive: true })
    .filter((f) => String(f).endsWith('.log'))
    .map((f) => fs.readFileSync(path.join(dir, String(f)), 'utf8'))
    .join('\n');
}
const countInLog = (re) => (goosedLogText().match(new RegExp(re, 'g')) ?? []).length;

async function startGoosed() {
  launch += 1;
  const port = await freePort();
  const out = fs.openSync(path.join(root, `goosed-${launch}.log`), 'w');
  const env = {
    PATH: process.env.PATH,
    HOME: home,
    TMPDIR: process.env.TMPDIR ?? os.tmpdir(),
    GOOSE_SERVER__SECRET_KEY: SERVE_SECRET,
    GOOSE_DISABLE_KEYRING: '1',
    LEANZERO_LINK_WORKER_URL: workerUrl,
    LEANZERO_TAILSCALED: TAILSCALED,
    LEANZERO_TAILSCALE_CLI: TAILSCALE,
    LEANZERO_LINK_CONTROL_PORT: String(controlPort),
    RUST_LOG: 'info,leanzero_link=info',
  };
  const child = spawn(GOOSE, ['serve', '--host', '127.0.0.1', '--port', String(port)], {
    env,
    stdio: ['ignore', out, out],
  });
  await waitFor(`goosed #${launch} to listen on ${port}`, () =>
    new Promise((resolve) => {
      const s = net.connect(port, '127.0.0.1');
      s.once('connect', () => {
        s.destroy();
        resolve(true);
      });
      s.once('error', () => resolve(false));
    })
  );
  log(`goosed #${launch} pid ${child.pid} on :${port}`);
  const g = { child, port };
  liveGoosed.add(g);
  child.once('exit', () => liveGoosed.delete(g));
  return g;
}

async function stopGoosed(g, signal) {
  if (g.child.exitCode != null || g.child.signalCode != null) return;
  const exited = new Promise((r) => g.child.once('exit', (code, sig) => r({ code, sig })));
  g.child.kill(signal);
  const result = await exited;
  log(`goosed pid ${g.child.pid} exited after ${signal}: ${JSON.stringify(result)}`);
}

// ── a minimal ACP client over the /acp WebSocket (what the desktop opens at launch) ───
class Acp {
  constructor(port) {
    this.url = `ws://127.0.0.1:${port}/acp?token=${SERVE_SECRET}`;
    this.nextId = 1;
    this.pending = new Map();
  }
  async open() {
    this.ws = new WebSocket(this.url);
    await new Promise((resolve, reject) => {
      this.ws.addEventListener('open', resolve, { once: true });
      this.ws.addEventListener('error', (e) => reject(new Error(`ws error: ${e.message ?? e.type}`)), { once: true });
    });
    this.ws.addEventListener('message', (event) => {
      const msg = JSON.parse(typeof event.data === 'string' ? event.data : event.data.toString());
      if (msg.id != null && this.pending.has(msg.id) && (msg.result !== undefined || msg.error)) {
        const { resolve, reject } = this.pending.get(msg.id);
        this.pending.delete(msg.id);
        msg.error ? reject(Object.assign(new Error(msg.error.message), { data: msg.error.data })) : resolve(msg.result);
      }
    });
    await this.call('initialize', { protocolVersion: 1, clientCapabilities: {} });
    return this;
  }
  call(method, params = {}) {
    const id = this.nextId++;
    this.ws.send(JSON.stringify({ jsonrpc: '2.0', id, method, params }));
    return new Promise((resolve, reject) => this.pending.set(id, { resolve, reject }));
  }
  link(op, params = {}) {
    return this.call(`_goose/unstable/leanzeroLink/${op}`, params);
  }
  close() {
    this.ws?.close();
  }
}

// ── our daemon, found by its unique statedir argv; stopped per-pid only ────────────────
function ourTailscaledPids() {
  try {
    return execFileSync('pgrep', ['-f', `statedir=${stateDir}`], { encoding: 'utf8' })
      .split('\n')
      .map((s) => s.trim())
      .filter(Boolean)
      .map(Number);
  } catch {
    return [];
  }
}
function killOurTailscaled(pid) {
  const args = execFileSync('ps', ['-o', 'args=', '-p', String(pid)], { encoding: 'utf8' });
  assert(
    args.startsWith(TAILSCALED) && args.includes(`--statedir=${stateDir}`),
    `pid ${pid} is not our tailscaled: ${args}`
  );
  process.kill(pid, 'SIGTERM');
}
function onlineNodes() {
  return JSON.parse(headscale('nodes', 'list', '-o', 'json') || '[]').filter((n) => n.online);
}
const intentOnDisk = () => {
  const p = path.join(leanzero, 'link-intent.json');
  return fs.existsSync(p) ? JSON.parse(fs.readFileSync(p, 'utf8')) : null;
};

const children = [];
let personalBefore;
let ok = false;
try {
  personalBefore = personalIdentity();
  log('personal tailscale identity BEFORE:', JSON.stringify(personalBefore));

  fs.mkdirSync(hsDir, { recursive: true });
  const [metrics, grpc] = [await freePort(), await freePort()];
  fs.writeFileSync(
    hsCfg,
    `server_url: ${loginServer}
listen_addr: 127.0.0.1:${hsPort}
metrics_listen_addr: 127.0.0.1:${metrics}
grpc_listen_addr: 127.0.0.1:${grpc}
grpc_allow_insecure: false
unix_socket: ${hsDir}/hs.sock
unix_socket_permission: "0770"
noise:
  private_key_path: ${hsDir}/noise_private.key
prefixes:
  v4: 100.64.0.0/10
  v6: fd7a:115c:a1e0::/48
  allocation: sequential
derp:
  server:
    enabled: false
  urls:
    - https://controlplane.tailscale.com/derpmap/default
  auto_update_enabled: false
  paths: []
disable_check_updates: true
node:
  ephemeral:
    inactivity_timeout: 30m
database:
  type: sqlite
  sqlite:
    path: ${hsDir}/db.sqlite
dns:
  magic_dns: false
  override_local_dns: false
  base_domain: leanzero.test
  nameservers:
    global: []
log:
  level: info
policy:
  mode: database
`
  );
  const hs = spawn(HEADSCALE, ['-c', hsCfg, 'serve'], {
    stdio: ['ignore', fs.openSync(path.join(root, 'headscale.log'), 'w'), fs.openSync(path.join(root, 'headscale.log'), 'a')],
  });
  children.push(hs);
  await waitFor('headscale /health', async () => (await fetch(`${loginServer}/health`)).ok);
  hsUserId = JSON.parse(headscale('users', 'create', 'acct-e2e', '-o', 'json')).id;
  log(`headscale pid ${hs.pid} at ${loginServer}, user id ${hsUserId}`);

  await new Promise((r) => worker.listen(0, '127.0.0.1', r));
  workerUrl = `http://127.0.0.1:${worker.address().port}`;
  controlPort = await freePort();

  // A signed-in account (what a verified email code leaves), never connected here.
  fs.mkdirSync(leanzero, { recursive: true, mode: 0o700 });
  fs.writeFileSync(
    path.join(leanzero, 'identity.json'),
    JSON.stringify({ email: ACCOUNT_EMAIL, token: ACCOUNT_TOKEN, updated_at: new Date().toISOString() }),
    { mode: 0o600 }
  );

  // ── 1. never connected: the launch does not connect ──────────────────────────────
  let g = await startGoosed();
  let acp = await new Acp(g.port).open();
  let st = await waitFor('the launch reconnect outcome (never connected)', async () => {
    const s = await acp.link('status');
    return s.reconnect?.state !== 'idle' ? s : null;
  });
  assert(st.reconnect.state === 'skipped', `expected skipped, got ${JSON.stringify(st.reconnect)}`);
  assert(st.auth.state === 'loggedIn', `auth ${st.auth.state}`);
  assert(ourTailscaledPids().length === 0, 'no daemon for a never-connected node');
  assert(joinKeysMinted === 0, 'no join key minted');
  receipt(`1 never-connected launch: reconnect=${JSON.stringify(st.reconnect)} intent=${JSON.stringify(st.intent)} daemons=0 joinKeys=0`);

  // ── 2. the user's Connect ──────────────────────────────────────────────────────────
  st = await acp.link('connect');
  assert(st.auth.state === 'connected', `connect → ${JSON.stringify(st.auth)} lastError=${st.lastError}`);
  const firstIp = st.auth.meshIp;
  const intent2 = intentOnDisk();
  assert(intent2?.intent === 'connected' && intent2?.cause === 'userConnect', `intent ${JSON.stringify(intent2)}`);
  const pids2 = ourTailscaledPids();
  assert(pids2.length === 1, `one daemon, got ${pids2}`);
  const nodes2 = await waitFor('the node online on headscale', () => (onlineNodes().length === 1 ? onlineNodes() : null));
  receipt(`2 user connect: connected meshIp=${firstIp} intent=${JSON.stringify(intent2)} tailscaled pid=${pids2[0]} headscale online=${nodes2.map((n) => n.given_name ?? n.name).join(',')} joinKeys=${joinKeysMinted}`);
  acp.close();

  // ── 3. quit the way the desktop quits (SIGTERM → teardown), relaunch, no click ────────
  await stopGoosed(g, 'SIGTERM');
  await waitFor('our daemon stopped by the teardown', () => ourTailscaledPids().length === 0);
  const keysBefore3 = joinKeysMinted;
  const reconnectLinesBefore3 = countInLog('leanzero_link_reconnect: reconnected with no user action');
  g = await startGoosed();
  acp = await new Acp(g.port).open(); // the desktop's launch connection — no Link call made yet
  st = await waitFor('the launch reconnect to finish', async () => {
    const s = await acp.link('status');
    return ['reconnected', 'failed'].includes(s.reconnect?.state) ? s : null;
  });
  assert(st.reconnect.state === 'reconnected', `expected reconnected, got ${JSON.stringify(st.reconnect)} lastError=${st.lastError}`);
  assert(st.auth.state === 'connected', `auth ${JSON.stringify(st.auth)}`);
  assert(joinKeysMinted === keysBefore3 + 1, 'one fresh join key for the reconnect');
  const pids3 = ourTailscaledPids();
  assert(pids3.length === 1 && pids3[0] !== pids2[0], `a fresh daemon, got ${pids3}`);
  assert(intentOnDisk()?.cause === 'userConnect', 'the reconnect never rewrites the intent');
  assert(
    countInLog('leanzero_link_reconnect: reconnected with no user action') === reconnectLinesBefore3 + 1,
    'goosed log carries the reconnect line'
  );
  const nodes3 = await waitFor('an online node after the reconnect', () => (onlineNodes().length >= 1 ? onlineNodes() : null));
  receipt(`3 SIGTERM + relaunch, no user action: reconnect=${JSON.stringify(st.reconnect)} auth=${st.auth.state} meshIp=${st.auth.meshIp} tailscaled pid=${pids3[0]} (was ${pids2[0]}) headscale online=${nodes3.length} joinKeys=${joinKeysMinted}`);

  // ── 3b. a second window's goosed leaves the mesh to the first ──────────────────────
  const keysBefore3b = joinKeysMinted;
  const second = await startGoosed();
  const acp2 = await new Acp(second.port).open();
  const st2 = await waitFor('the second goosed launch outcome', async () => {
    const s = await acp2.link('status');
    return s.reconnect?.state !== 'idle' ? s : null;
  });
  assert(st2.reconnect.state === 'skipped', `second goosed: ${JSON.stringify(st2.reconnect)}`);
  assert(st2.reconnect.reason.includes(String(pids3[0])), `names the holder: ${st2.reconnect.reason}`);
  assert(joinKeysMinted === keysBefore3b, 'no join key minted by the second goosed');
  acp2.close();
  await stopGoosed(second, 'SIGTERM');
  const still = ourTailscaledPids();
  assert(still.length === 1 && still[0] === pids3[0], `the first goosed's daemon survives the second's exit: ${still}`);
  assert((await acp.link('status')).auth.state === 'connected', 'the first goosed is still connected');
  receipt(`3b second goosed on the same home: reconnect=${JSON.stringify(st2.reconnect)} joinKeys unchanged=${joinKeysMinted}; after its SIGTERM the first's tailscaled pid ${still[0]} still serves`);
  acp.close();

  // ── 4. a goosed crash (SIGKILL): the orphan daemon is refused by name; Retry recovers ──
  await stopGoosed(g, 'SIGKILL');
  const orphans = ourTailscaledPids();
  assert(orphans.length === 1, `the crashed goosed leaves its daemon, got ${orphans}`);
  const keysBefore4 = joinKeysMinted;
  g = await startGoosed();
  acp = await new Acp(g.port).open();
  st = await waitFor('the launch reconnect outcome after a crash', async () => {
    const s = await acp.link('status');
    return ['reconnected', 'failed'].includes(s.reconnect?.state) ? s : null;
  });
  assert(st.reconnect.state === 'failed', `an orphan on the socket must fail by name, got ${JSON.stringify(st.reconnect)}`);
  assert(st.reconnect.reason.includes(`kill ${orphans[0]}`), `the reason names the pid to stop: ${st.reconnect.reason}`);
  assert(joinKeysMinted === keysBefore4, 'the orphan is classified before any join key is minted');
  assert(st.auth.state === 'loggedIn', `auth ${st.auth.state}`);
  receipt(`4a SIGKILL + relaunch: reconnect failed, named: "${st.reconnect.reason.slice(0, 220)}"`);
  killOurTailscaled(orphans[0]);
  await waitFor('the orphan gone', () => ourTailscaledPids().length === 0);
  st = await acp.link('connect'); // Retry
  assert(st.auth.state === 'connected', `retry → ${JSON.stringify(st.auth)} ${st.lastError}`);
  assert(st.reconnect.state === 'idle', 'the Retry supersedes the launch report');
  receipt(`4b orphan pid ${orphans[0]} stopped per-pid; Retry → connected meshIp=${st.auth.meshIp}`);

  // ── 5. the user's Disconnect stays off across a relaunch ───────────────────────────
  st = await acp.link('disconnect');
  assert(st.auth.state === 'loggedIn', `disconnect → ${JSON.stringify(st.auth)}`);
  await waitFor('the daemon stopped by the disconnect', () => ourTailscaledPids().length === 0);
  const intent5 = intentOnDisk();
  assert(intent5?.intent === 'disconnected' && intent5?.cause === 'userDisconnect', `intent ${JSON.stringify(intent5)}`);
  assert(fs.existsSync(path.join(leanzero, 'identity.json')), 'disconnect keeps the account');
  acp.close();
  await stopGoosed(g, 'SIGTERM');
  const keysBefore5 = joinKeysMinted;
  g = await startGoosed();
  acp = await new Acp(g.port).open();
  st = await waitFor('the launch reconnect outcome after a disconnect', async () => {
    const s = await acp.link('status');
    return s.reconnect?.state !== 'idle' ? s : null;
  });
  assert(st.reconnect.state === 'skipped', `a disconnected node stays off, got ${JSON.stringify(st.reconnect)}`);
  // Give a (wrong) background connect every chance to show itself before asserting absence.
  await sleep(5_000);
  const after5 = await acp.link('status');
  assert(after5.auth.state === 'loggedIn', `still off: ${after5.auth.state}`);
  assert(ourTailscaledPids().length === 0, 'no daemon started');
  assert(joinKeysMinted === keysBefore5, 'no join key requested');
  receipt(`5 disconnect + relaunch: reconnect=${JSON.stringify(st.reconnect)} auth=${after5.auth.state} daemons=0 joinKeys unchanged=${joinKeysMinted}`);
  acp.close();
  await stopGoosed(g, 'SIGTERM');
  ok = true;
} catch (error) {
  console.error(error);
} finally {
  for (const g of [...liveGoosed]) await stopGoosed(g, 'SIGTERM');
  for (const pid of ourTailscaledPids()) killOurTailscaled(pid);
  for (const child of children) if (child.exitCode == null) child.kill('SIGTERM');
  worker.close();
  const personalAfter = personalIdentity();
  log('personal tailscale identity AFTER: ', JSON.stringify(personalAfter));
  const same = JSON.stringify(personalBefore) === JSON.stringify(personalAfter);
  receipt(`personal tailscale identity fields ${same ? 'UNCHANGED' : 'CHANGED'} (backend=${personalAfter.backendState}, self=${personalAfter.self?.hostName}, peers=${personalAfter.peers?.length})`);
  if (!same) ok = false;
  log(`artifacts under ${root}`);
  console.log(`\n${ok ? 'PASS' : 'FAIL'} — ${receipts.length} receipts`);
  process.exitCode = ok ? 0 : 1;
}
