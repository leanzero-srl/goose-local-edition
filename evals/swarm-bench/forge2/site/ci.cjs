'use strict';
// The scorer's CI sequence (SPEC R6, §2.7): the deployment events a CI system sends the entrant's web trigger,
// seeded, each step with the outcome the contract guarantees. Every request is rendered at SEND time against the
// virtual clock, so a fresh timestamp stays fresh however long the app's earlier invocations ran.
//
//   const seq = createCiSequence({ seed, secret, issueKeys });
//   seq.plan              -> [{ name, eventId, environment, issueKeys, expect }]      (the secret is not in it)
//   seq.next(nowSeconds)  -> { name, request: { method, headers: [[name, value], ...], body }, expect } | null
//   await runCiSequence(emu, { moduleKey, seed, secret, issueKeys })
//                         -> { plan, steps: [{ name, eventId, expect, statusCode, body, error, invocation }] }
//
// expect = { status, effect }: effect {environment, issueKeys} is what the step must show (the issues' ledger
// rows/issue say "Deployed to <environment>"); null means the step must change nothing — for the replay, nothing
// beyond the valid step's single effect. Whether an effect happened is the oracle's call (bench/forge2_oracle.py).
//
// Steps, in order: valid · replay (the valid request's exact bytes, sent next so it is inside the 300 s window
// whatever the app's first invocation cost) · bad-signature · unsigned · tampered (signed for staging, sent as
// production) · stale (timestamp in the past) · stale-future · header-case (header names spelled off-canonical).
// Each step owns its issues and its eventId, so any effect is attributable to exactly one step.
// Not here, on purpose: a concurrent duplicate — two invocations' KVS calls interleave by OS scheduling in the
// emulator, so that row would not be identical on every machine.
const crypto = require('crypto');
const { createRng, SEED_RE } = require('./rng.cjs');
const { CI, ciSignature, encodeCiEvent, invokeWebtrigger } = require('../kit/lib/webtrigger.cjs');

// A stale timestamp sits off the window by more than a whole web-trigger invocation twice over, so it stays stale
// even when the app reads its clock at the end of its run (the future side shrinks by that much).
const WEBTRIGGER_LIMIT_S = 55; // measured: developer.atlassian.com/platform/forge/limits-invocation (SPEC §2.2)
const STALE_MIN_S = CI.windowSeconds + 2 * WEBTRIGGER_LIMIT_S; // ratio: the window plus two whole invocations (410 s)
const STALE_MAX_S = 6 * CI.windowSeconds; // ratio: half a virtual hour
// Neither the contract's spelling, nor all-lower, nor all-upper: a lookup that only knows one of those fails here,
// and the same way on every seed.
const CASED_NAMES = ['x-Lz-Timestamp', 'X-lz-SIGNATURE'];
const KEYS_PER_STEP = { valid: 2, 'bad-signature': 1, unsigned: 1, tampered: 1, stale: 1, 'stale-future': 1, 'header-case': 1 };

function createCiSequence({ seed, secret, issueKeys }) {
  if (!SEED_RE.test(String(seed))) throw new Error(`ci sequence: seed must be 16 lowercase hex chars, got ${JSON.stringify(seed)}`);
  if (typeof secret !== 'string' || !secret) throw new Error('ci sequence: the CI secret the app showed is required (a non-empty string)');
  const pool = [...new Set(Array.isArray(issueKeys) ? issueKeys : [])].sort();
  const needed = Object.values(KEYS_PER_STEP).reduce((a, b) => a + b, 0);
  if (pool.length < needed) throw new Error(`ci sequence: needs ${needed} distinct issue keys, got ${pool.length}`);
  const rng = createRng(crypto.createHash('sha256').update(`forge2-ci:${seed}`).digest('hex').slice(0, 16));
  const keys = rng.sample(pool, needed);
  const take = (n) => keys.splice(0, n);
  let wrongSecret = rng.hex(64);
  while (wrongSecret === secret) wrongSecret = rng.hex(64);
  const pastSkew = rng.int(STALE_MIN_S, STALE_MAX_S);
  const futureSkew = rng.int(STALE_MIN_S, STALE_MAX_S);

  const event = (environment, n) => ({ eventId: rng.uuid(), environment, issueKeys: take(n) });
  const valid = event('staging', KEYS_PER_STEP.valid);
  const steps = [
    { name: 'valid', ...valid, skew: 0, expect: { status: 202, effect: { environment: valid.environment, issueKeys: valid.issueKeys } } },
    { name: 'replay', ...valid, replayOf: 'valid', expect: { status: 200, effect: null } },
    { name: 'bad-signature', ...event('staging', 1), skew: 0, signWith: wrongSecret, expect: { status: 401, effect: null } },
    { name: 'unsigned', ...event('production', 1), skew: 0, unsigned: true, expect: { status: 401, effect: null } },
    { name: 'tampered', ...event('production', 1), skew: 0, signedEnvironment: 'staging', expect: { status: 401, effect: null } },
    { name: 'stale', ...event('staging', 1), skew: -pastSkew, expect: { status: 401, effect: null } },
    { name: 'stale-future', ...event('staging', 1), skew: futureSkew, expect: { status: 401, effect: null } },
  ];
  const cased = event('production', 1);
  steps.push({ name: 'header-case', ...cased, skew: 0, names: CASED_NAMES, expect: { status: 202, effect: { environment: cased.environment, issueKeys: cased.issueKeys } } });

  const rendered = new Map();
  function render(step, now) {
    if (step.replayOf) {
      const original = rendered.get(step.replayOf);
      if (!original) throw new Error(`ci sequence: '${step.name}' replays '${step.replayOf}', which was not sent`);
      return { method: original.method, headers: original.headers.map((h) => [...h]), body: original.body };
    }
    const timestamp = now + step.skew;
    const body = (environment) => encodeCiEvent({ eventId: step.eventId, sentAt: timestamp, environment, issueKeys: step.issueKeys });
    const signedBody = body(step.signedEnvironment ?? step.environment);
    const [tsName, sigName] = step.names ?? [CI.timestampHeader, CI.signatureHeader];
    const headers = [[step.names ? 'content-type' : 'Content-Type', 'application/json'], [tsName, String(timestamp)]];
    if (!step.unsigned) headers.push([sigName, ciSignature(step.signWith ?? secret, timestamp, signedBody)]);
    const request = { method: 'POST', headers, body: body(step.environment) };
    rendered.set(step.name, request);
    return request;
  }

  let cursor = 0;
  return {
    plan: steps.map(({ name, eventId, environment, issueKeys: k, expect }) => ({ name, eventId, environment, issueKeys: k, expect })),
    next(nowSeconds) {
      if (!Number.isInteger(nowSeconds)) throw new Error(`ci sequence: next() takes the virtual clock in whole unix seconds, got ${nowSeconds}`);
      const step = steps[cursor];
      if (!step) return null;
      cursor += 1;
      return { name: step.name, eventId: step.eventId, request: render(step, nowSeconds), expect: step.expect };
    },
  };
}

// Drives the sequence in-process through the same ingress the emulator mounts, one step at a time, at the emulator's
// virtual clock. It does not drain queues: an app may finish the work in a consumer, and the probe drains in its phase.
async function runCiSequence(emu, { moduleKey, seed, secret, issueKeys }) {
  const seq = createCiSequence({ seed, secret, issueKeys });
  const steps = [];
  for (let s = seq.next(Math.floor(emu.clock.now() / 1000)); s; s = seq.next(Math.floor(emu.clock.now() / 1000))) {
    const r = await invokeWebtrigger(emu, moduleKey, s.request);
    steps.push({ name: s.name, eventId: s.eventId, expect: s.expect, request: s.request, statusCode: r.statusCode, body: r.body, error: r.error, invocation: r.invocation });
  }
  return { plan: seq.plan, steps };
}

module.exports = { createCiSequence, runCiSequence };
