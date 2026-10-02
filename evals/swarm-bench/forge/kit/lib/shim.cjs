'use strict';
// TIER A — the in-repo stand-in for Atlassian's runtime wrapper. It installs exactly the two globals
// @forge/api and @forge/kvs read (`global.__forge_runtime__`, `global.__forge_fetch__`) and speaks the
// wrapper's proxy protocol, so the SAME proxy serves both. It exists ONLY for offline development of the
// harness itself: the emulator runs it solely behind `runtime: 'shim'`, and every result produced with
// it is marked `runtime: "shim"`, `publishable: false`. There is no automatic fallback to it.
const path = require('path');

function proxyRoute(meta) {
  const cap = '/fpp/as/app/provider/atlassian/capability';
  switch (meta.type) {
    case 'fpp': {
      const acct = meta.provider === 'user' && meta.accountId !== undefined ? `/account/${encodeURIComponent(meta.accountId)}` : '';
      const where = 'contextAri' in meta ? `/contextAri/${encodeURIComponent(meta.contextAri)}` : `/remote/${meta.remote}`;
      return `/fpp/provider/${meta.provider}${where}${acct}`;
    }
    case 'kvs': case 'sql': case 'os': case 'realtime': return `${cap}/${meta.type}`;
    case 'egress': return '/egress';
    default: throw new Error(`shim: no proxy route for __forge_fetch__ type ${meta.type}`);
  }
}

async function run(lambdaEvent, deadline) {
  const meta = lambdaEvent._meta;
  const nativeFetch = globalThis.fetch;
  global.__forge_fetch__ = async (m, p, init = {}) => {
    const headers = new Headers(init.headers);
    headers.set('forge-proxy-authorization', `Bearer ${meta.proxy.token}`);
    headers.set('forge-proxy-target', p.toString());
    return nativeFetch(new URL(proxyRoute(m), meta.proxy.url), { ...init, headers });
  };
  globalThis.fetch = (url, init) => global.__forge_fetch__({ type: 'egress' }, new URL(url.toString()).toString(), init);
  global.__forge_runtime__ = {
    appContext: meta.appContext, contextAri: meta.contextAri, proxy: meta.proxy, aaid: meta.aaid, tracing: meta.tracing,
    lambdaContext: { getRemainingTimeInMillis: () => deadline - Date.now() },
    metrics: { counter: () => ({ incr() {}, incrBy() {}, decr() {}, decrBy() {} }), timing: () => ({ measure: () => ({ stop() {} }) }), gauge: () => ({ set() {} }) },
    featureFlags: () => false,
  };
  const [file, fn] = lambdaEvent.handler.split('.');
  try {
    const mod = require(path.join(process.env.LAMBDA_TASK_ROOT, `${file}.cjs`));
    if (typeof mod[fn] !== 'function') throw new Error(`Handler ${fn} not found in ${file}`);
    const context = { installContext: meta.contextAri, ...(meta.aaid ? { principal: { accountId: meta.aaid } } : {}) };
    const body = await mod[fn](lambdaEvent.body, context);
    return { success: true, body: body === undefined ? undefined : JSON.parse(JSON.stringify(body)) };
  } catch (e) {
    return { success: false, error: { errorType: e?.name ?? 'Error', errorMessage: String(e?.message ?? e) } };
  }
}

module.exports = { run, proxyRoute };
