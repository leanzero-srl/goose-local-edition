// TIER A — a minimal stand-in for the Forge node runtime wrapper. It installs exactly the two
// globals @forge/api and @forge/kvs read (node_modules/@forge/api/out/api/runtime.js:109
// `global.__forge_runtime__`, fetch.js:23 `global.__forge_fetch__`, kvs/out/index.js:8/25) and
// speaks the same proxy protocol the real wrapper does (route + forge-proxy-target header), so the
// SAME forge-proxy.cjs serves both tiers. No line of the model's code is patched.
const { AsyncLocalStorage } = require('async_hooks');
const als = new AsyncLocalStorage();

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
    case 'tpp': return `/tpp/as/user/provider/${meta.provider}/remote/${meta.remote}/account/${encodeURIComponent(meta.accountId)}`;
    default: throw new Error(`emulator: no proxy route for __forge_fetch__ type ${meta.type}`);
  }
}

function installShim({ proxyUrl }) {
  const nativeFetch = globalThis.fetch;
  global.__forge_fetch__ = async (meta, path, init = {}) => {
    const headers = new Headers(init.headers);
    headers.set('forge-proxy-authorization', 'Bearer emulator');
    headers.set('forge-proxy-target', path.toString());
    return nativeFetch(new URL(proxyRoute(meta), proxyUrl), { ...init, headers });
  };
  // External fetch goes through the proxy's /egress door, as on the platform, so the manifest's
  // permissions.external.fetch.backend is enforced instead of the sandbox reaching the internet.
  globalThis.fetch = (url, init) => global.__forge_fetch__({ type: 'egress' }, new URL(url.toString()).toString(), init);
  Object.defineProperty(global, '__forge_runtime__', { configurable: true, get: () => als.getStore() });
}

const metrics = { counter: () => ({ incr() {} }), timing: () => ({ measure: () => ({ stop() {} }) }) };

function runInInvocation(appContext, fn) {
  const runtime = {
    appContext,
    contextAri: appContext.contextAri,
    proxy: { token: 'emulator', url: 'emulator', host: 'emulator' },
    tracing: { traceId: appContext.invocationId, spanId: '0' },
    lambdaContext: { getRemainingTimeInMillis: () => 25_000 },
    metrics,
  };
  return als.run(runtime, fn);
}

module.exports = { installShim, runInInvocation };
