'use strict';
// Web-trigger ingress (Forge 2.0 SPEC R6, §2.7). The emulator mounts `webtriggerRoute(emu)` for
// `/x/webtrigger/<moduleKey>`; the scorer's CI sequence (site/ci.cjs) and `forge-dev ci send` (bin/ci.cjs) call
// `invokeWebtrigger` in-process.
//
//   await invokeWebtrigger(emu, moduleKey, { method, path, headers: [[name, value], ...] | {name: value|[values]}, body })
//       -> { statusCode, statusText, headers: {name: [values]}, body, error, request, invocation }
//   webtriggerRoute(emu)                -> (req, res) => handled    a node:http handler for /x/webtrigger/<moduleKey>
//   webtriggerModules(manifest)         -> the manifest's webtrigger modules
//   CI, ciSignature(secret, timestamp, rawBody), encodeCiEvent(event)       the CI signing scheme (SPEC §2.7)
//
// The function receives Forge's documented request (events-reference/web-trigger; @forge/api 8.2.0 webTrigger.d.ts):
// {method, body: the raw body string, path, userPath, headers: {name: string[]}, queryParameters: {name: string[]}}.
// Header names keep the sender's spelling (values grouped under the first spelling of a case-insensitive name): the
// contract makes header names case-insensitive, and only a preserved spelling lets a case-varied sender test that.
// Unmeasured on live Forge (research/understand/real-forge-fidelity.md, backlog P10): whether the platform
// lower-cases names, and the exact `path`/`userPath` strings; the live request's undocumented `call`, `context` and
// `contextToken` are not emulated.
//
// The response follows the module's `response.type` (manifest-reference/modules/web-trigger): `dynamic`, the
// default, returns {statusCode, statusText?, headers?: {name: string[]}, body?: string}; `static` returns
// {outputKey} naming one of `response.outputs[{key, statusCode, contentType?, body?}]`. A result that fits neither,
// a function that throws, and one killed at its 55 s limit get the platform's "error response with status code 500"
// (the docs); `error` names the cause so it is never a guess. `error` is also set on the 404 for an undeclared
// module; it is null whenever the status is the app's own.
const crypto = require('crypto');

const ROUTE = /^\/x\/webtrigger\/([^/]+)(\/.*)?$/;
const REASON_CHARS = 300; // ratio: one terminal line of diagnosis in a response header

const CI = Object.freeze({
  timestampHeader: 'X-LZ-Timestamp',
  signatureHeader: 'X-LZ-Signature',
  windowSeconds: 300,
  environments: Object.freeze(['staging', 'production']),
});

// SPEC §2.7: `sha256=<hex HMAC-SHA256(secret, "<timestamp>.<raw body>")>`, keyed with the secret string's UTF-8 bytes.
const ciSignature = (secret, timestamp, rawBody) =>
  `sha256=${crypto.createHmac('sha256', secret).update(`${timestamp}.${rawBody}`).digest('hex')}`;

// Pretty-printed: never the bytes JSON.stringify of the parsed event gives back, so an app that verifies a
// re-serialisation instead of the raw body (research/BRIEF.md §2.2 trap 7) fails every valid event, in dev as in scoring.
const encodeCiEvent = (event) => `${JSON.stringify(event, null, 2)}\n`;

const webtriggerModules = (manifest) =>
  (Array.isArray(manifest?.modules?.webtrigger) ? manifest.modules.webtrigger : []).filter((m) => m && typeof m.key === 'string');

const headerPairs = (headers) => (Array.isArray(headers) ? headers
  : Object.entries(headers ?? {}).flatMap(([name, v]) => (Array.isArray(v) ? v : [v]).map((x) => [name, x])));

// Built in a Map: a sender-chosen name such as `__proto__` stays an ordinary own key.
function arrays(pairs, keyOf = (name) => name) {
  const out = new Map();
  const spelling = new Map();
  for (const [name, value] of pairs) {
    const id = keyOf(String(name));
    if (!spelling.has(id)) spelling.set(id, String(name));
    const key = spelling.get(id);
    out.set(key, [...(out.get(key) ?? []), String(value)]);
  }
  return Object.fromEntries(out);
}

function webtriggerRequest(moduleKey, { method = 'POST', path, headers = [], body = '' } = {}) {
  const url = new URL(path ?? `/x/webtrigger/${encodeURIComponent(moduleKey)}`, 'http://webtrigger.invalid');
  return {
    method: String(method).toUpperCase(),
    body: Buffer.isBuffer(body) ? body.toString('utf8') : String(body),
    path: url.pathname,
    userPath: url.pathname.match(ROUTE)?.[2] ?? '',
    headers: arrays(headerPairs(headers), (name) => name.toLowerCase()),
    queryParameters: arrays(url.searchParams),
  };
}

const clip = (s) => (s.length > REASON_CHARS ? `${s.slice(0, REASON_CHARS)}…` : s);
const validStatus = (n) => Number.isInteger(n) && n >= 100 && n <= 599;
const isHeaderMap = (h) => h !== null && typeof h === 'object' && !Array.isArray(h)
  && Object.values(h).every((v) => Array.isArray(v) && v.every((x) => typeof x === 'string'));
const platformError = (error) => ({ statusCode: 500, statusText: undefined, headers: {}, body: '', error });

function responseFor(module, result) {
  const shown = clip(JSON.stringify(result) ?? String(result));
  if (module.response?.type === 'static') {
    const outputs = Array.isArray(module.response.outputs) ? module.response.outputs : [];
    const out = outputs.find((o) => o && o.key === result?.outputKey);
    if (!out) {
      return platformError(`static web trigger '${module.key}': the function returned ${shown}; it must return {outputKey} naming one of `
        + `response.outputs (${outputs.map((o) => o?.key).join(', ') || 'none declared'})`);
    }
    const statusCode = Number(out.statusCode);
    if (!validStatus(statusCode)) return platformError(`response.outputs '${out.key}' of '${module.key}' declares no HTTP statusCode (100-599)`);
    return { statusCode, statusText: undefined, headers: out.contentType ? { 'Content-Type': [String(out.contentType)] } : {},
      body: out.body === undefined ? '' : String(out.body), error: null };
  }
  if (result === null || typeof result !== 'object' || Array.isArray(result)) {
    return platformError(`web trigger '${module.key}': the function returned ${shown}; a dynamic web trigger returns {statusCode, headers?, body?}`);
  }
  if (!validStatus(result.statusCode)) {
    return platformError(`web trigger '${module.key}': statusCode ${JSON.stringify(result.statusCode)} — it is required and must be an HTTP status (100-599)`);
  }
  if (result.body !== undefined && typeof result.body !== 'string') return platformError(`web trigger '${module.key}': the response body must be a string`);
  if (result.headers !== undefined && !isHeaderMap(result.headers)) {
    return platformError(`web trigger '${module.key}': response headers must map each name to an array of strings`);
  }
  if (result.statusText !== undefined && typeof result.statusText !== 'string') return platformError(`web trigger '${module.key}': statusText must be a string`);
  return { statusCode: result.statusCode, statusText: result.statusText, headers: result.headers ?? {}, body: result.body ?? '', error: null };
}

async function invokeWebtrigger(emu, moduleKey, http = {}) {
  const module = webtriggerModules(emu.manifest).find((m) => m.key === moduleKey);
  if (!module) {
    return { statusCode: 404, statusText: undefined, headers: {}, body: '', error: `no webtrigger module '${moduleKey}' in manifest.yml`, request: null, invocation: null };
  }
  const request = webtriggerRequest(moduleKey, http);
  if (typeof module.function !== 'string') return { ...platformError(`webtrigger '${moduleKey}' names no function`), request, invocation: null };
  const invocation = await emu.invoke(module.function, { moduleKey, event: request });
  if (!invocation.ok) {
    const why = invocation.timedOut ? `the function was stopped at its ${invocation.timeoutSec} s limit`
      : `the function failed: ${invocation.error?.name}: ${invocation.error?.message}`;
    return { ...platformError(clip(why)), request, invocation };
  }
  return { ...responseFor(module, invocation.result), request, invocation };
}

const headerSafe = (s) => clip(String(s)).replace(/[^\x20-\x7e]/g, '?');

function webtriggerRoute(emu) {
  return (req, res) => {
    const m = new URL(req.url, 'http://webtrigger.invalid').pathname.match(ROUTE);
    if (!m) return false;
    const chunks = [];
    req.on('error', () => res.destroy()); // the sender went away mid-body: nobody is left to answer
    req.on('data', (c) => chunks.push(c));
    req.on('end', async () => {
      const headers = [];
      for (let i = 0; i < req.rawHeaders.length; i += 2) headers.push([req.rawHeaders[i], req.rawHeaders[i + 1]]);
      let out;
      try {
        out = await invokeWebtrigger(emu, m[1], { method: req.method, path: req.url, headers, body: Buffer.concat(chunks) });
      } catch (e) {
        // The emulator failed, not the app: 502 and a named header, so no reader takes it for the app's answer.
        out = { statusCode: 502, headers: {}, body: '', error: `emulator failure, not the app's: ${e.message}` };
      }
      const head = { ...out.headers };
      if (out.error) head['x-forge-emulator-error'] = [headerSafe(out.error)];
      try {
        res.writeHead(out.statusCode, out.statusText, head);
      } catch (e) {
        res.writeHead(500, { 'x-forge-emulator-error': headerSafe(`the app's response headers are not valid HTTP: ${e.message}`) });
        res.end();
        return;
      }
      res.end(out.body);
    });
    return true;
  };
}

// The emulator's proxy route (POST <proxy>/x/webtrigger/<key>) answers through this: `request` carries the sender's
// header pairs as sent ([[name, value], ...]) and the raw body; the emulator-failure cause rides a named header.
async function handle(emu, moduleKey, request) {
  const r = await invokeWebtrigger(emu, moduleKey, request);
  const headers = { ...r.headers };
  if (r.error) headers['x-forge-emulator-error'] = [headerSafe(r.error)];
  return { statusCode: r.statusCode, headers, body: r.body };
}

module.exports = { CI, ciSignature, encodeCiEvent, webtriggerModules, webtriggerRequest, responseFor, invokeWebtrigger, webtriggerRoute, handle };
