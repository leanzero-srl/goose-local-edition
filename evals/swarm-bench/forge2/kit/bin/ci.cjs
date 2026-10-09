'use strict';
// `forge-dev ci send`: play the CI system against your app's web trigger. One deployment event, signed with the
// contract's scheme (X-LZ-Timestamp; X-LZ-Signature = sha256=<hex HMAC-SHA256(secret, "<timestamp>.<raw body>")>),
// goes through the same web-trigger ingress the scorer uses (lib/webtrigger.cjs), at the dev site's virtual time;
// the queues are drained afterwards, as `events` and `scheduled` do. A variation flag breaks one thing on purpose.
//
// forge-dev.cjs wires it (the emulator, its state and the printers are forge-dev's):
//   if (cmd === 'ci') {
//     const r = await require('./ci.cjs').ciCommand(argv.slice(1), { emu, printInvocation, printDeliveries });
//     for (const d of r.deliveries) consumed.add(d.eventId);
//     await saveState(emu, consumed);
//     return r.code;
//   }
const crypto = require('crypto');
const { CI, ciSignature, encodeCiEvent, invokeWebtrigger, webtriggerModules } = require('../lib/webtrigger.cjs');

const CI_USAGE = `  ci send --env staging|production --issues KEY-1,KEY-2 [--secret <s>] [--module <webtrigger key>]
          [--event-id <id>] [--skew <seconds>] [--bad-signature | --unsigned | --tamper | --stale | --replay | --header-case]
        play the CI system: POST one signed deployment event to your web trigger, print what it answered and
        what it did, then drain the queues. The secret is the one your admin page showed when you rotated it
        (--secret, or $FORGE_CI_SECRET). The body is the raw JSON {eventId, sentAt, environment, issueKeys},
        signed byte for byte as sent; the timestamp is the dev site's virtual clock (+ --skew).
          --bad-signature  signed with a different secret        --unsigned     no X-LZ-Signature header
          --tamper         body changed after signing (environment flipped)
          --stale          timestamp 600 s in the past (--skew -600; --skew 600 is the future side)
          --replay         the same request twice, byte for byte  --header-case  header names in another case
          --event-id <id>  reuse an eventId (a CI retry re-signs the same event)
`;

const VARIANTS = ['bad-signature', 'unsigned', 'tamper', 'stale', 'replay', 'header-case'];
const OPTIONS = ['env', 'issues', 'secret', 'module', 'event-id', 'skew'];
const STALE_SKEW_S = -2 * CI.windowSeconds; // ratio: twice the window, unmistakably stale

function parse(args) {
  const out = { sub: null, variant: null, opts: {} };
  for (let i = 0; i < args.length; i++) {
    const a = args[i];
    if (!a.startsWith('--')) {
      if (out.sub) throw new Error(`ci: unexpected argument '${a}'\n${CI_USAGE}`);
      out.sub = a;
      continue;
    }
    const name = a.slice(2);
    if (VARIANTS.includes(name)) {
      if (out.variant) throw new Error(`ci: --${out.variant} and --${name}: one variation per send`);
      out.variant = name;
      continue;
    }
    if (!OPTIONS.includes(name)) throw new Error(`ci: unknown option ${a}\n${CI_USAGE}`);
    if (i + 1 >= args.length) throw new Error(`ci: ${a} needs a value`);
    out.opts[name] = args[++i];
  }
  return out;
}

function chooseModule(manifest, requested) {
  const keys = webtriggerModules(manifest).map((m) => m.key);
  if (requested !== undefined) {
    if (!keys.includes(requested)) throw new Error(`ci: --module ${requested} is not a webtrigger module in manifest.yml (webtrigger modules: ${keys.join(', ') || 'none'})`);
    return requested;
  }
  if (keys.length === 1) return keys[0];
  throw new Error(keys.length ? `ci: manifest.yml has several webtrigger modules (${keys.join(', ')}): name one with --module` : 'ci: manifest.yml declares no webtrigger module');
}

function printExchange(print, label, moduleKey, request, r) {
  print(`== ci ${label}: POST /x/webtrigger/${moduleKey}`);
  for (const [name, value] of request.headers) print(`  ${name}: ${value}`);
  print(`  body (${Buffer.byteLength(request.body)} bytes, raw): ${JSON.stringify(request.body)}`);
  const headers = Object.entries(r.headers ?? {}).map(([n, v]) => `${n}: ${v.join(', ')}`).join('; ');
  print(`-> ${r.statusCode}${r.statusText ? ` ${r.statusText}` : ''}${headers ? `  [${headers}]` : ''}  body: ${JSON.stringify(r.body)}`);
  if (r.error) print(`   emulator: ${r.error}`);
}

async function ciCommand(args, { emu, printInvocation = null, printDeliveries = null, print = console.log }) {
  const { sub, variant, opts } = parse(args);
  if (sub !== 'send') throw new Error(`ci: ${sub ? `unknown subcommand '${sub}'` : 'missing subcommand'}\n${CI_USAGE}`);
  const moduleKey = chooseModule(emu.manifest, opts.module);
  const environment = opts.env;
  if (!CI.environments.includes(environment)) throw new Error(`ci: --env must be one of ${CI.environments.join(', ')} (got ${environment ?? 'nothing'})`);
  const issueKeys = String(opts.issues ?? '').split(',').map((s) => s.trim()).filter(Boolean);
  if (!issueKeys.length) throw new Error('ci: --issues KEY-1,KEY-2 names the issues the deployment shipped');
  const secret = opts.secret ?? process.env.FORGE_CI_SECRET;
  if (!secret && !['unsigned', 'bad-signature'].includes(variant)) {
    throw new Error('ci: no CI secret — rotate it in your admin page (it is shown once), then pass --secret <value> or set FORGE_CI_SECRET');
  }
  if (variant === 'stale' && opts.skew !== undefined) throw new Error('ci: --stale is --skew -600; give one of them');
  const skew = variant === 'stale' ? STALE_SKEW_S : Number(opts.skew ?? 0);
  if (!Number.isInteger(skew)) throw new Error(`ci: --skew takes whole seconds (got ${opts.skew})`);

  const timestamp = Math.floor(emu.clock.now() / 1000) + skew;
  const event = { eventId: opts['event-id'] ?? crypto.randomUUID(), sentAt: timestamp, environment, issueKeys };
  const body = encodeCiEvent(event);
  const flipped = CI.environments.find((e) => e !== environment);
  const sentBody = variant === 'tamper' ? encodeCiEvent({ ...event, environment: flipped }) : body;
  const [tsName, sigName] = variant === 'header-case' ? ['x-Lz-Timestamp', 'X-lz-SIGNATURE'] : [CI.timestampHeader, CI.signatureHeader];
  const headers = [['Content-Type', 'application/json'], [tsName, String(timestamp)]];
  if (variant !== 'unsigned') headers.push([sigName, ciSignature(variant === 'bad-signature' ? crypto.randomBytes(32).toString('hex') : secret, timestamp, body)]);
  const request = { method: 'POST', headers, body: sentBody };

  const labels = variant === 'replay' ? ['first delivery', 'replay (the same bytes again)'] : [variant ?? 'valid'];
  if (variant === 'tamper') print(`(signed for environment ${environment}; the body sent says ${flipped})`);
  const exchanges = [];
  for (const label of labels) {
    const r = await invokeWebtrigger(emu, moduleKey, request);
    printExchange(print, label, moduleKey, request, r);
    if (r.invocation && printInvocation) printInvocation(r.invocation, `web trigger ${moduleKey}`);
    exchanges.push({ label, request, response: { statusCode: r.statusCode, headers: r.headers, body: r.body, error: r.error }, invocationId: r.invocation?.invocationId ?? null });
  }
  const deliveries = await emu.drainQueues();
  if (deliveries.length) {
    if (printDeliveries) printDeliveries(deliveries);
    else for (const d of deliveries) print(`-- queue ${d.queueName} event ${d.eventId} attempt ${d.attempt}: ${d.outcome}`);
  }
  // 1 when a send got the platform's 500 (the function threw, timed out or returned an unusable result); any
  // status the app chose is its answer, not a tool failure.
  const failed = exchanges.some((x) => x.response.error && x.response.statusCode === 500);
  return { code: failed ? 1 : 0, exchanges, deliveries };
}

module.exports = { ciCommand, CI_USAGE };
