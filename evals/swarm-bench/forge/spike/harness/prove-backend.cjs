// Proof 1: invoke every manifest function of app/ (resolver x2, scheduledTrigger, webtrigger,
// queue consumer fed by what the trigger pushed) against the mock Jira, under tier A and tier B,
// and print what landed in Jira and KVS.
const path = require('path');
const { startMockJira, calls, issues } = require('./mock-jira.cjs');
const { createForgeProxy } = require('./forge-proxy.cjs');
const E = require('./emulator.cjs');

const tier = process.argv[2] ?? 'A';
const appDir = path.join(__dirname, '..', 'app');
const SCRATCH = process.env.SPIKE_SCRATCH ?? path.join(__dirname, '..', '.out');

(async () => {
  const manifest = E.loadManifest(appDir);
  const users = E.functionUsers(manifest);
  await startMockJira(18990 + (tier === 'B' ? 1 : 0));
  const proxy = createForgeProxy({ jiraUrl: `http://127.0.0.1:${18990 + (tier === 'B' ? 1 : 0)}`, manifest });
  const proxyUrl = await proxy.listen();
  const bundleDir = await E.bundle(appDir, path.join(SCRATCH, `bundle-${tier}`), manifest);
  if (tier === 'B') E.prepareRealRuntimeDir(bundleDir);
  const ctx = { bundleDir, manifest, proxyUrl };
  const invoke = async (fn, event, req) => {
    const moduleKey = users[fn]?.[0]?.key;
    if (tier === 'A') return E.invokeA(ctx, fn, moduleKey, event, req);
    const r = await E.invokeB(ctx, fn, moduleKey, event);
    if (r.logs.length) console.log(`  [${fn} logs]`, r.logs.join(' | ').slice(0, 300));
    if (!r.success) throw new Error(`${fn}: ${JSON.stringify(r.error)}`);
    return r.body;
  };
  const extension = { type: 'jira:issuePanel', issue: { key: 'SPK-1', id: '10001', type: 'Bug' }, project: { key: 'SPK', id: '10000', type: 'software' } };
  const uiCall = (functionKey, payload) => ({ call: { functionKey, payload }, context: { cloudId: E.CLOUD_ID, localId: 'local-1', moduleKey: 'spike-issue-panel', extension, accountId: '5b10ac8d82e05b22cc7d4ef5' } });
  const reqCtx = { principal: { accountId: '5b10ac8d82e05b22cc7d4ef5' }, installContext: E.CONTEXT_ARI };

  const out = {};
  out.summarise1 = await invoke('resolver', uiCall('summarise', {}), reqCtx);
  out.summarise2 = await invoke('resolver', uiCall('summarise', { issueKey: 'SPK-1' }), reqCtx);
  out.comment = await invoke('resolver', uiCall('comment', { issueKey: 'SPK-2', text: 'hello from the emulator' }), reqCtx);
  out.egressProbe = await invoke('resolver', uiCall('egressProbe', {}), reqCtx);
  out.hourly = await invoke('hourly', { context: { cloudId: E.CLOUD_ID, moduleKey: 'spike-hourly' }, contextToken: 'x' }, reqCtx);
  out.hook = await invoke('hook', { method: 'POST', path: '/x1/spike-hook', headers: { 'content-type': ['application/json'] }, queryParameters: {}, body: JSON.stringify({ id: 'evt-7', ok: true }), context: { cloudId: E.CLOUD_ID, moduleKey: 'spike-hook' } }, reqCtx);
  out.queued = proxy.queue.length;
  for (const q of proxy.queue.splice(0)) out[`consume:${q.body.key}`] = await invoke('consume', { body: q.body, queueName: q.queueName, jobId: q.jobId, eventId: 'e1', retryContext: { retryCount: 0 } }, reqCtx);

  console.log(`TIER ${tier} handler returns:`, JSON.stringify(out, null, 1));
  console.log('JIRA calls landed:'); for (const c of calls) console.log(`  ${c.as.padEnd(4)} ${c.method} ${c.path}${c.body ? ' ' + JSON.stringify(c.body).slice(0, 80) : ''}`);
  console.log('KVS state:', JSON.stringify(Object.fromEntries([...proxy.kvs].map(([k, v]) => [k, v.value]))));
  console.log('labels after consumer:', issues['SPK-1'].fields.labels);
  console.log('proxy routes:', [...new Set(proxy.log.map((l) => `${l.route} <- ${l.target}`))].join('\n  '));
  console.log('authed (forge-proxy-authorization present):', proxy.log.every((l) => l.authed), 'egress attempts:', proxy.log.filter((l) => l.route === '/egress').map((l) => l.target));
  console.log('UNMODELLED:', proxy.unmodelled);
  process.exit(0);
})().catch((e) => { console.error('FAILED', e); process.exit(1); });
