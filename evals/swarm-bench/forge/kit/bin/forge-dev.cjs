#!/usr/bin/env node
'use strict';
// forge-dev: the entrant's offline Forge tools (STARTER.md). The SAME emulator and Custom UI host the
// scorer uses, pointed at the dev site ($FORGE_SITE_URL, seeded differently from the scoring site), with
// dev storage and queues persisted under .forge-dev/ in the workspace. It contains no fixtures, oracle,
// fault schedule or checks.
const fs = require('fs');
const path = require('path');

const USAGE = `forge-dev <command>   (run from the app directory; $FORGE_SITE_URL names the dev site)

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
        the URL is also written to .forge-dev/serve.json. In an edit surface window.__forgeHost.save()
        performs the dashboard's Save.
  kvs                         dump stored keys and entities
  users                       list the dev site's users (the first line is the default viewer)
  reset                       clear dev storage and queues and rewind the dev site's update stream
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
const loadState = () => (fs.existsSync(stateFile) ? JSON.parse(fs.readFileSync(stateFile, 'utf8')) : {});
function saveState(emu, extra = {}) {
  fs.mkdirSync(stateDir, { recursive: true });
  const prev = loadState();
  fs.writeFileSync(stateFile, JSON.stringify({ ...prev, ...emu.dumpState(), widgetConfigs: { ...(prev.widgetConfigs ?? {}), ...emu.widgetConfigs() }, ...extra }));
}

async function emulator() {
  const site = siteFromEnv();
  const { createEmulator } = lib('emulator.cjs');
  const state = loadState();
  const emu = await createEmulator({ appDir, kitDir, site, runtime: 'wrapper', fence: 'dev-auto', workDir: path.join(stateDir, 'work'),
    devState: state.kvs ? { kvs: state.kvs, queue: state.queue } : null });
  if (emu.fence !== 'sandbox') {
    console.log('fence: node --permission (macOS refuses a nested sandbox inside this workspace). The scorer runs every invocation under a deny-default\n'
      + '       sandbox: no file reads outside the bundle, no child processes, network only to the Forge proxy.');
  }
  if (!emu.manifest) { console.log(`manifest.yml: ${emu.manifestError}`); return emu; }
  const built = await emu.build();
  for (const f of built.functions.filter((x) => !x.loaded)) console.log(`function ${f.key} (${f.handler}) does not load: ${f.error}`);
  return emu;
}

const short = (v, n = 4000) => { const s = typeof v === 'string' ? v : JSON.stringify(v, null, 2); return s && s.length > n ? `${s.slice(0, n)}… (${s.length} chars)` : s; };
function printCalls(calls) {
  for (const c of calls) {
    const who = c.service === 'jira' ? c.provider : '';
    const extra = [c.retryAfter ? `Retry-After ${c.retryAfter}` : '', c.needsAuthentication ? 'NeedsAuthenticationError (asUser with no user)' : '', c.limitError ? c.limitError : '', c.missingScope ? `missing ${c.missingScope}` : ''].filter(Boolean).join(' ');
    console.log(`  ${String(c.service).padEnd(6)} ${who.padEnd(4)} ${c.method} ${c.path} -> ${c.status}${extra ? `  ${extra}` : ''}`);
  }
}
function printInvocation(r, label) {
  console.log(`== ${label}: ${r.functionKey ?? '?'} (${r.moduleType ?? '?'} ${r.moduleKey ?? ''})${r.asUser ? ` as ${r.asUser}` : ' as app'} -> ${r.ok ? 'ok' : r.timedOut ? 'TIMED OUT' : 'FAILED'} ${r.ms ?? 0} ms`);
  if (r.ok) console.log(`result: ${short(r.result === undefined ? null : r.result)}`);
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
    fs.rmSync(stateDir, { recursive: true, force: true });
    console.log('dev storage and queues cleared; the dev site rewound to its first update');
    return 0;
  }
  const emu = await emulator();
  const info = emu.siteInfo;
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
        r = await emu.invokeAction(use.key, payload ?? {}, { asUser: opt('as') });
      } else {
        if (!payload) throw new Error(`a ${use.type} function needs --payload <file> (its event); for live issue updates use \`events\``);
        r = await emu.invoke(fnKey, { moduleKey: use.key, event: payload, asUser: opt('as') });
      }
      printInvocation(r, 'invoke');
      const pending = emu.queueState.pending.length;
      if (pending) console.log(`${pending} queued event(s) pending: delivered by the next \`events\` or \`scheduled\``);
      saveState(emu);
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
        printDeliveries(ds);
        for (const id of [...new Set(ds.map((x) => x.invocationId).filter(Boolean))]) {
          const calls = emu.log.filter((c) => c.invocationId === id);
          if (calls.length) { console.log(`   calls of ${id}:`); printCalls(calls); }
        }
      }
      saveState(emu);
      return 0;
    }
    if (cmd === 'scheduled') {
      const key = positional[0];
      if (!key) throw new Error('scheduled <moduleKey>');
      const r = await emu.runScheduled(key);
      printInvocation(r.invocation, `scheduled run`);
      printDeliveries(r.deliveries);
      for (const id of [...new Set(r.deliveries.map((x) => x.invocationId).filter(Boolean))]) {
        const calls = emu.log.filter((c) => c.invocationId === id);
        if (calls.length) { console.log(`   calls of ${id}:`); printCalls(calls); }
      }
      saveState(emu);
      return r.invocation.ok ? 0 : 1;
    }
    if (cmd === 'serve') {
      const key = positional[0];
      const found = key && emu.moduleByKey(key);
      if (!found) throw new Error(`serve <moduleKey>: no module '${key}'`);
      const edit = flag('edit');
      const stored = loadState().widgetConfigs?.['dev-widget'];
      const config = opt('config') ? JSON.parse(opt('config')) : stored ?? null;
      const extension = devExtension(info, found.type, { sprint: opt('sprint'), config, edit });
      const { url } = await emu.serveDev({ moduleKey: key, entry: edit ? 'edit' : 'view', theme: opt('theme') ?? 'light', asUser: opt('as') ?? info.viewer, extension,
        widgetId: 'dev-widget', layout: { width: Number(opt('width') ?? 800), height: Number(opt('height') ?? 600) } });
      fs.mkdirSync(stateDir, { recursive: true });
      fs.writeFileSync(path.join(stateDir, 'serve.json'), JSON.stringify({ url, pid: process.pid, moduleKey: key, entry: edit ? 'edit' : 'view' }));
      console.log(url);
      console.log(`serving ${key} (${edit ? 'edit' : 'view'}) as ${opt('as') ?? info.viewer}; stop with: kill ${process.pid}`);
      let lastLog = 0;
      const tick = setInterval(() => {
        saveState(emu);
        for (const b of emu.bridgeLog.slice(lastLog)) console.log(`bridge ${b.op}${b.payload?.functionKey ? ` ${b.payload.functionKey}` : ''}${b.error ? ` ERROR ${b.error}` : ''}`);
        lastLog = emu.bridgeLog.length;
      }, 1000);
      const stop = async () => { clearInterval(tick); saveState(emu); fs.rmSync(path.join(stateDir, 'serve.json'), { force: true }); await emu.close(); process.exit(0); };
      process.on('SIGTERM', stop);
      process.on('SIGINT', stop);
      return new Promise(() => {});
    }
    throw new Error(`unknown command '${cmd}'\n${USAGE}`);
  } finally {
    if (cmd !== 'serve') await emu.close();
  }
}

main().then((code) => { process.exitCode = code; }).catch((e) => { console.error(`forge-dev: ${e.message}`); process.exitCode = 2; });
