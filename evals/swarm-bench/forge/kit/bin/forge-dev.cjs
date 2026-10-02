#!/usr/bin/env node
'use strict';
// forge-dev: the entrant's offline Forge tools (STARTER.md). The SAME emulator and Custom UI host the
// scorer uses, pointed at the dev site ($FORGE_SITE_URL, seeded differently from the scoring site), with
// dev storage and queues persisted under .forge-dev/ in the workspace. It contains no fixtures, oracle,
// fault schedule or checks.
const crypto = require('crypto');
const fs = require('fs');
const path = require('path');

const USAGE = `forge-dev <command>   (run from the app directory; $FORGE_SITE_URL names the dev site)

  $FORGE_SITE_URL is set for you: http://admin:<control token>@127.0.0.1:<port> — the token is the URL's
  password. Each invoke writes the function's full result to .forge-dev/last-result.json.

  invoke <functionKey> [--module <key>] [--resolver <key>] [--payload <file>] [--as <accountId>] [--sprint <id>] [--config <json>]
        Run a manifest function once in the Forge runtime with the event shape of the module that
        references it. --resolver calls that resolver key with --payload as its payload (resolvers are
        user-led: --as defaults to the dev viewer). For a trigger, consumer or action, --payload is the
        event JSON (a consumer gets it as the AsyncEvent body, an action as its inputs).
  events [--limit N]          deliver the dev site's next N issue updates (default 1) to your trigger(s),
                              then drain the queues
  scheduled <moduleKey>       run a scheduled trigger once, then drain the queues
  serve <moduleKey> [--edit] [--sprint <id>] [--config <json>] [--theme light|dark] [--as <accountId>]
        serve a Custom UI module with the bridge and dashboard host; prints its URL and runs until stopped.
        Start it in the background (e.g. \`node $FORGE_KIT/bin/forge-dev.cjs serve <key> > .forge-dev/serve.log 2>&1 &\`);
        the URL is also written to .forge-dev/serve.json (the latest) and .forge-dev/serve.<pid>.json; several serve
        processes may run side by side. In an edit surface window.__forgeHost.save()
        performs the dashboard's Save. A saved widget config persists like a dashboard's: later
        serve runs of the widget open with it; --config '<json>' overrides it for one run, reset clears it.
  kvs                         dump stored keys and entities
  users                       list the dev site's users (the first line is the default viewer)
  reset                       clear dev storage, queues and saved widget configs, and rewind the dev site's update stream
`;

const argv = process.argv.slice(2);
const cmd = argv[0];
const flag = (name) => argv.includes(`--${name}`);
const opt = (name) => { const i = argv.indexOf(`--${name}`); return i >= 0 ? argv[i + 1] : undefined; };
const BOOLEAN_FLAGS = new Set(['--edit', '--help']);
const positional = [];
for (let i = 1; i < argv.length; i++) {
  if (argv[i].startsWith('--')) { if (!BOOLEAN_FLAGS.has(argv[i])) i++; continue; }
  positional.push(argv[i]);
}

const kitDir = process.env.FORGE_KIT || path.resolve(__dirname, '..');
const lib = (m) => require(path.join(kitDir, 'lib', m));
const appDir = process.cwd();
const stateDir = path.join(appDir, '.forge-dev');
const stateFile = path.join(stateDir, 'state.json');

function siteFromEnv() {
  const raw = process.env.FORGE_SITE_URL;
  if (!raw) throw new Error('FORGE_SITE_URL is not set: the dev site is started by the harness before your session');
  const u = new URL(raw);
  if (!u.password) throw new Error('FORGE_SITE_URL carries no dev-site control token');
  return { url: `${u.protocol}//${u.host}`, adminUrl: `${u.protocol}//${u.host}/__site/${u.password}` };
}
// .forge-dev/state.json is shared by every forge-dev process in the workspace (a backgrounded `serve` and the
// commands run beside it): reads and writes happen under a pid lock file and writes are atomic renames.
const lockFile = path.join(stateDir, 'state.lock');
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
const alive = (pid) => { try { process.kill(pid, 0); return true; } catch (e) { return e.code === 'EPERM'; } };
async function withLock(fn) {
  fs.mkdirSync(stateDir, { recursive: true });
  const mine = `${process.pid}:${crypto.randomBytes(6).toString('hex')}`;
  const holderOf = (file) => { try { return fs.readFileSync(file, 'utf8'); } catch (e) { if (e.code === 'ENOENT') return null; throw e; } };
  for (;;) {
    try { fs.writeFileSync(lockFile, mine, { flag: 'wx' }); break; } catch (e) {
      if (e.code !== 'EEXIST') throw e;
    }
    // The holder released between our create and our read (ENOENT, the race ALT-NOTES 1.7 hit), or the file
    // is mid-write (empty): retry. A dead holder's lock is taken over by an atomic rename, so of several
    // processes that saw it dead exactly one removes it; a lock that turns out to be live is put back.
    const holder = holderOf(lockFile);
    const pid = Number(String(holder ?? '').split(':')[0]);
    if (holder && pid && pid !== process.pid && !alive(pid)) {
      const stolen = `${lockFile}.${mine.replace(':', '-')}.stale`;
      try { fs.renameSync(lockFile, stolen); } catch (e) { if (e.code !== 'ENOENT') throw e; continue; }
      if (holderOf(stolen) !== holder) { try { fs.linkSync(stolen, lockFile); } catch (e) { if (e.code !== 'EEXIST') throw e; } }
      fs.rmSync(stolen, { force: true });
      continue;
    }
    await sleep(25);
  }
  try { return await fn(); } finally { if (holderOf(lockFile) === mine) fs.rmSync(lockFile, { force: true }); }
}
// Absent state is an empty workspace; unreadable or corrupt state is an error, never an empty substitute.
const loadState = () => {
  let text;
  try { text = fs.readFileSync(stateFile, 'utf8'); } catch (e) { if (e.code === 'ENOENT') return {}; throw e; }
  try { return JSON.parse(text); } catch (e) { throw new Error(`${stateFile} is not valid JSON (${e.message}); \`forge-dev reset\` clears it`); }
};
const writeState = (state) => {
  const tmp = `${stateFile}.${process.pid}.tmp`;
  fs.writeFileSync(tmp, JSON.stringify(state));
  fs.renameSync(tmp, stateFile);
};
// Merge this process' queue into the shared one: keep others' events, drop what this process consumed.
async function saveState(emu, consumed = new Set()) {
  await withLock(async () => {
    const prev = loadState();
    const mine = emu.queueState.pending;
    const mineIds = new Set(mine.map((e) => e.eventId));
    const theirs = (prev.queue ?? []).filter((e) => !mineIds.has(e.eventId) && !consumed.has(e.eventId));
    // KVS is persisted by each invocation (aroundInvocation); only the queue and widget configs land here.
    writeState({ ...prev, queue: [...theirs, ...mine], widgetConfigs: { ...(prev.widgetConfigs ?? {}), ...emu.widgetConfigs() } });
  });
}

async function emulator() {
  const site = siteFromEnv();
  const { createEmulator } = lib('emulator.cjs');
  const state = await withLock(async () => loadState());
  let emu = null;
  // Each invocation sees the KVS other forge-dev processes wrote, and leaves its writes for them.
  const aroundInvocation = (run) => withLock(async () => {
    const disk = loadState();
    if (emu && disk.kvs) emu.kvs.load(disk.kvs);
    const r = await run();
    if (emu) writeState({ ...loadState(), kvs: emu.kvs.dump() });
    return r;
  });
  // A separate work dir per process: two concurrent forge-dev commands never rebuild each other's bundle.
  emu = await createEmulator({ appDir, kitDir, site, runtime: 'wrapper', fence: 'dev-auto', workDir: path.join(stateDir, 'work', String(process.pid)),
    devState: state.kvs ? { kvs: state.kvs, queue: state.queue } : null, aroundInvocation });
  if (emu.fence !== 'sandbox') {
    console.log('fence: node --permission (macOS refuses a nested sandbox inside this workspace). The scorer runs every invocation under a deny-default\n'
      + '       sandbox: no file reads outside the bundle, no child processes, network only to the Forge proxy.');
  }
  if (!emu.manifest) { console.log(`manifest.yml: ${emu.manifestError}`); return emu; }
  const built = await emu.build();
  for (const f of built.functions.filter((x) => !x.loaded)) console.log(`function ${f.key} (${f.handler}) does not load: ${f.error}`);
  return emu;
}

const SHORT_CHARS = 4000; // ratio: a terminal screen of JSON; the whole value is always in .forge-dev/last-result.json
const short = (v, n = SHORT_CHARS) => { const s = typeof v === 'string' ? v : JSON.stringify(v, null, 2); return s && s.length > n ? `${s.slice(0, n)}… (${s.length} chars)` : s; };
function printCalls(calls) {
  for (const c of calls) {
    const who = c.service === 'jira' ? c.provider : '';
    const extra = [c.retryAfter ? `Retry-After ${c.retryAfter}` : '', c.needsAuthentication ? 'NeedsAuthenticationError (asUser with no user)' : '', c.limitError ? c.limitError : '', c.missingScope ? `missing ${c.missingScope}` : ''].filter(Boolean).join(' ');
    console.log(`  ${String(c.service).padEnd(6)} ${who.padEnd(4)} ${c.method} ${c.path} -> ${c.status}${extra ? `  ${extra}` : ''}`);
  }
}
function printInvocation(r, label) {
  console.log(`== ${label}: ${r.functionKey ?? '?'} (${r.moduleType ?? '?'} ${r.moduleKey ?? ''})${r.asUser ? ` as ${r.asUser}` : ' as app'} -> ${r.ok ? 'ok' : r.timedOut ? 'TIMED OUT' : 'FAILED'} ${r.ms ?? 0} ms`);
  if (r.ok) {
    // The full result is always written; the terminal shows it whole up to the print budget.
    const full = JSON.stringify(r.result === undefined ? null : r.result, null, 2);
    fs.mkdirSync(stateDir, { recursive: true });
    const file = path.join(stateDir, 'last-result.json');
    fs.writeFileSync(file, full + '\n');
    console.log(`result: ${short(r.result === undefined ? null : r.result)}`);
    if (full.length > SHORT_CHARS) console.log(`(full result, ${full.length} chars: ${path.relative(process.cwd(), file)})`);
  }
  else console.log(`error: ${r.error?.name}: ${r.error?.message}`);
  for (const l of r.logs ?? []) console.log(`  [${l.logLevel ?? 'log'}] ${(l.logArguments ?? [l.raw]).join(' ')}`);
  if (r.calls?.length) { console.log('calls:'); printCalls(r.calls); }
}
function printDeliveries(ds) {
  for (const d of ds) {
    console.log(`-- queue ${d.queueName} event ${d.eventId} attempt ${d.attempt}: ${d.outcome}${d.retryAfter ? ` (redelivered after ${d.retryAfter} s)` : ''}${d.dropped ? ` DROPPED: ${d.dropped}` : ''}`);
    if (d.error) console.log(`   error: ${d.error.name}: ${d.error.message}`);
    for (const l of d.logs ?? []) console.log(`   [${l.logLevel ?? 'log'}] ${(l.logArguments ?? [l.raw]).join(' ')}`);
  }
}

function devExtension(info, moduleType, { sprint, config, edit, widgetId = 'dev-widget' }) {
  if (moduleType === 'dashboards:widget') {
    return { type: 'dashboards:widget', config: config ?? null, context: { dashboardId: 'dev-dashboard', widgetId }, ...(edit ? { entryPoint: 'edit' } : {}) };
  }
  if (moduleType === 'jira:sprintAction') {
    const sp = info.sprints.find((s) => String(s.id) === String(sprint));
    if (!sp) throw new Error(`--sprint ${sprint ?? '(missing)'}: not a sprint on the dev site (find ids with the Agile API, e.g. /rest/agile/1.0/board/{id}/sprint)`);
    const board = info.boards.find((b) => b.id === sp.originBoardId);
    const project = info.projects.find((p) => p.key === board.projectKey);
    return { type: 'jira:sprintAction', project: { id: project.id, key: project.key, type: 'software' }, board: { id: String(board.id), type: board.type },
      sprint: { id: String(sp.id), state: sp.state }, location: `${info.siteUrl}/jira/software/projects/${project.key}/boards/${board.id}` };
  }
  return { type: moduleType };
}

async function main() {
  if (!cmd || cmd === 'help' || flag('help')) { console.log(USAGE); return 0; }
  if (cmd === 'kvs') {
    const { createKvs } = lib('kvs.cjs');
    const st = loadState();
    const kvs = createKvs();
    if (st.kvs) kvs.load(st.kvs);
    console.log(JSON.stringify({ ...kvs.snapshot(), pendingQueueEvents: (st.queue ?? []).length, widgetConfigs: st.widgetConfigs ?? {} }, null, 2));
    return 0;
  }
  if (cmd === 'users') {
    const site = siteFromEnv();
    const info = await (await fetch(`${site.adminUrl}/info`, { method: 'POST', body: '{}', headers: { 'content-type': 'application/json' } })).json();
    for (const u of [info.users.find((x) => x.accountId === info.viewer), ...info.users.filter((x) => x.accountId !== info.viewer)]) console.log(`${u.accountId}  ${u.displayName}${u.accountId === info.viewer ? '  (default viewer)' : ''}`);
    console.log(`app account: ${info.appAccountId}`);
    return 0;
  }
  if (cmd === 'reset') {
    const site = siteFromEnv();
    await fetch(`${site.adminUrl}/reset`, { method: 'POST', body: '{}', headers: { 'content-type': 'application/json' } });
    await withLock(async () => fs.rmSync(stateFile, { force: true }));
    console.log('dev storage and queues cleared; the dev site rewound to its first update');
    return 0;
  }
  const emu = await emulator();
  const info = emu.siteInfo;
  const consumed = new Set();
  let serving = false;
  try {
    if (cmd === 'invoke') {
      const fnKey = positional[0];
      if (!fnKey) throw new Error('invoke <functionKey> ...');
      const payload = opt('payload') ? JSON.parse(fs.readFileSync(opt('payload'), 'utf8')) : undefined;
      const resolverKey = opt('resolver');
      const users = lib('emulator.cjs').functionUsers(emu.manifest)[fnKey] ?? [];
      const use = opt('module') ? users.find((u) => u.key === opt('module')) : users[0];
      if (!use) throw new Error(`function '${fnKey}' is referenced by no module${opt('module') ? ` with key '${opt('module')}'` : ''}`);
      const config = opt('config') ? JSON.parse(opt('config')) : undefined;
      let r;
      if (resolverKey) {
        const asUser = opt('as') ?? info.viewer;
        const context = { accountId: asUser, cloudId: info.cloudId, siteUrl: info.siteUrl, moduleKey: use.key, environmentId: 'emulator-env', environmentType: 'DEVELOPMENT',
          locale: 'en-US', timezone: 'UTC', theme: { colorMode: 'light' }, extension: devExtension(info, use.type, { sprint: opt('sprint'), config }) };
        r = await emu.invokeResolver(use.key, resolverKey, payload ?? {}, context, asUser);
      } else if (use.type === 'scheduledTrigger') {
        r = await emu.invoke(fnKey, { moduleKey: use.key, event: { context: { cloudId: info.cloudId, moduleKey: use.key }, contextToken: 'dev' }, asUser: opt('as') });
      } else if (use.type === 'consumer') {
        if (!payload) throw new Error('a consumer needs --payload <file> (the pushed event body)');
        r = await emu.invoke(fnKey, { moduleKey: use.key, event: { body: payload, queueName: use.module.queue, jobId: 'dev-job', eventId: 'dev-job#0' }, asUser: opt('as') });
      } else if (use.type === 'action') {
        r = await emu.invokeAction(use.key, payload ?? {}, { asUser: opt('as') ?? info.viewer }); // Rovo actions are user-led
      } else {
        if (!payload) throw new Error(`a ${use.type} function needs --payload <file> (its event); for live issue updates use \`events\``);
        r = await emu.invoke(fnKey, { moduleKey: use.key, event: payload, asUser: opt('as') });
      }
      printInvocation(r, 'invoke');
      const pending = emu.queueState.pending.length;
      if (pending) console.log(`${pending} queued event(s) pending: delivered by the next \`events\` or \`scheduled\``);
      await saveState(emu, consumed);
      return r.ok ? 0 : 1;
    }
    if (cmd === 'events') {
      const limit = Number(opt('limit') ?? 1);
      for (let i = 0; i < limit; i++) {
        const d = await emu.deliverNext();
        if (!d) { console.log('no more issue updates on the dev site (`reset` replays them)'); break; }
        const items = d.event.changelog.items.map((it) => `${it.field}: ${it.fromString ?? '∅'} -> ${it.toString ?? '∅'}`).join('; ');
        console.log(`### update ${d.event.issue.key} changelog ${d.changelogId}${d.duplicate ? ' (redelivery)' : ''}: ${items}`);
        if (!d.triggers.length) console.log('   no trigger subscribes to avi:jira:updated:issue');
        for (const t of d.triggers) printInvocation(t, 'trigger');
        const ds = await emu.drainQueues();
        for (const x of ds) consumed.add(x.eventId);
        printDeliveries(ds);
        for (const id of [...new Set(ds.map((x) => x.invocationId).filter(Boolean))]) {
          const calls = emu.log.filter((c) => c.invocationId === id);
          if (calls.length) { console.log(`   calls of ${id}:`); printCalls(calls); }
        }
      }
      await saveState(emu, consumed);
      return 0;
    }
    if (cmd === 'scheduled') {
      const key = positional[0];
      if (!key) throw new Error('scheduled <moduleKey>');
      const r = await emu.runScheduled(key);
      for (const x of r.deliveries) consumed.add(x.eventId);
      printInvocation(r.invocation, `scheduled run`);
      printDeliveries(r.deliveries);
      for (const id of [...new Set(r.deliveries.map((x) => x.invocationId).filter(Boolean))]) {
        const calls = emu.log.filter((c) => c.invocationId === id);
        if (calls.length) { console.log(`   calls of ${id}:`); printCalls(calls); }
      }
      await saveState(emu, consumed);
      return r.invocation.ok ? 0 : 1;
    }
    if (cmd === 'serve') {
      const key = positional[0];
      const found = key && emu.moduleByKey(key);
      if (!found) throw new Error(`serve <moduleKey>: no module '${key}'`);
      const edit = flag('edit');
      // Widget config persists like a dashboard's: a Save in an edit surface is stored in .forge-dev/state.json and
      // later `serve` runs of the widget open with it. --config overrides it for this run; `reset` clears it.
      const stored = (await withLock(async () => loadState())).widgetConfigs?.['dev-widget'];
      let config = stored ?? null;
      if (opt('config') !== undefined) {
        try { config = JSON.parse(opt('config')); } catch (e) { throw new Error(`--config is not valid JSON: ${e.message}`); }
      }
      const extension = devExtension(info, found.type, { sprint: opt('sprint'), config, edit });
      const { url } = await emu.serveDev({ moduleKey: key, entry: edit ? 'edit' : 'view', theme: opt('theme') ?? 'light', asUser: opt('as') ?? info.viewer, extension,
        widgetId: 'dev-widget', layout: { width: Number(opt('width') ?? 800), height: Number(opt('height') ?? 600) } });
      fs.mkdirSync(stateDir, { recursive: true });
      const record = JSON.stringify({ url, pid: process.pid, moduleKey: key, entry: edit ? 'edit' : 'view' });
      fs.writeFileSync(path.join(stateDir, 'serve.json'), record);
      fs.writeFileSync(path.join(stateDir, `serve.${process.pid}.json`), record);
      console.log(url);
      console.log(`serving ${key} (${edit ? 'edit' : 'view'}) as ${opt('as') ?? info.viewer}; stop with: kill ${process.pid}`);
      if (found.type === 'dashboards:widget') {
        const source = opt('config') !== undefined ? '--config' : stored !== undefined ? 'stored by an earlier Save (.forge-dev/state.json; --config overrides, `reset` clears)' : 'none stored';
        console.log(`widget config: ${JSON.stringify(config)} (${source})`);
      }
      serving = true;
      let lastLog = 0;
      let savedConfigs = '{}';
      const tick = setInterval(async () => {
        for (const b of emu.bridgeLog.slice(lastLog)) {
          const detail = b.payload?.functionKey ? ` ${b.payload.functionKey}` : ['navigate', 'open'].includes(b.op) ? ` ${JSON.stringify(b.payload)}${b.url ? ` -> ${b.url}` : ''}` : '';
          console.log(`bridge ${b.op}${detail}${b.error ? ` ERROR ${b.error}` : ''}`);
        }
        lastLog = emu.bridgeLog.length;
        const configs = JSON.stringify(emu.widgetConfigs());
        if (configs !== savedConfigs) {
          savedConfigs = configs;
          await withLock(async () => { const prev = loadState(); writeState({ ...prev, widgetConfigs: { ...(prev.widgetConfigs ?? {}), ...emu.widgetConfigs() } }); });
        }
      }, 1000);
      const stop = async () => {
        clearInterval(tick);
        fs.rmSync(path.join(stateDir, `serve.${process.pid}.json`), { force: true });
        await emu.close();
        process.exit(0);
      };
      process.on('SIGTERM', stop);
      process.on('SIGINT', stop);
      return new Promise(() => {});
    }
    throw new Error(`unknown command '${cmd}'\n${USAGE}`);
  } finally {
    if (!serving) await emu.close();
  }
}

main().then((code) => { process.exitCode = code; }).catch((e) => { console.error(`forge-dev: ${e.message}`); process.exitCode = 2; });
