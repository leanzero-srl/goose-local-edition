'use strict';
// Runs ONE function invocation inside a fresh Node process (copied into the bundle dir as
// __forge_runner__.cjs so the deny-default sandbox can read it). Mirrors @forge/tunnel's
// out/sandbox/sandbox-runner.js: require the loader, call main(lambdaEvent, lambdaContext).
// Protocol: one JSON message on stdin {mode, lambdaEvent, deadline, clockOffsetMs}; the result is
// written as JSON to fd 3. Console output of the app is the wrapper's tunnel-mode JSON lines on stdout.
const fs = require('fs');

function shiftClock(offset) {
  if (!offset) return;
  const Real = Date;
  class VirtualDate extends Real {
    constructor(...args) { if (args.length === 0) super(Real.now() + offset); else super(...args); }
    static now() { return Real.now() + offset; }
  }
  global.Date = VirtualDate;
}

let input = '';
process.stdin.setEncoding('utf8');
process.stdin.on('data', (c) => (input += c));
process.stdin.on('end', async () => {
  const msg = JSON.parse(input);
  shiftClock(msg.clockOffsetMs);
  const out = fs.createWriteStream(null, { fd: 3 });
  const done = (obj) => out.end(JSON.stringify(obj), () => process.exit(0));
  try {
    let result;
    if (msg.mode === 'probe') {
      // Load check: require each bundle the way the wrapper does before calling a handler, with an
      // inert runtime installed so module-level @forge/* construction behaves as in production.
      global.__forge_runtime__ = { appContext: msg.appContext, contextAri: msg.appContext?.contextAri, proxy: { token: 'probe', url: 'http://127.0.0.1:9' },
        metrics: { counter: () => ({ incr() {} }), timing: () => ({ measure: () => ({ stop() {} }) }), gauge: () => ({ set() {} }) }, featureFlags: () => false,
        lambdaContext: { getRemainingTimeInMillis: () => 25_000 }, tracing: { traceId: 'probe', spanId: 'probe' } };
      result = {};
      for (const f of msg.files) {
        try {
          const m = require(`./${f}.cjs`);
          result[f] = { functions: Object.keys(m).filter((k) => typeof m[k] === 'function'), error: null };
        } catch (e) {
          result[f] = { functions: [], error: `${e?.name ?? 'Error'}: ${e?.message ?? e}` };
        }
      }
      return done({ result });
    }
    if (msg.mode === 'shim') {
      result = await require('./__forge_shim__.cjs').run(msg.lambdaEvent, msg.deadline);
    } else {
      global.__forge_tunnel__ = true;
      const handler = require('./__forge__.cjs').main;
      result = await handler(msg.lambdaEvent, { awsRequestId: msg.lambdaEvent._meta.appContext.invocationId, getRemainingTimeInMillis: () => msg.deadline - Date.now() });
    }
    done({ result });
  } catch (e) {
    done({ crash: { name: e?.name, message: String(e?.message ?? e), stack: String(e?.stack ?? '') } });
  }
});
