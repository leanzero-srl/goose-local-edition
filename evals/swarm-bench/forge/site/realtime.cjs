'use strict';
// Forge Realtime, emulated: the broker behind `@forge/realtime` (functions) and `@forge/bridge` `realtime.*`
// (Custom UI). It lives in the site process so every emulator attached to the site — the scorer's, or the dev
// kit's `serve` and CLI processes — publishes and subscribes on one set of channels.
//
// MEASURED 2026-10-03 (Atlassian's pinned wrapper 4f8170e0…): publish, publishGlobal and signRealtimeToken all
// POST GraphQL to `${proxy}/fpp/as/app/provider/atlassian/capability/realtime` with the proxy bearer; publish
// variables {installationId, name, payload: JSON.stringify(eventPayload), context?, isGlobal, token?} and header
// `x-forge-context-token: <invocation body.contextToken>` — the wrapper sets __forge_runtime__.realtime.contextToken
// from the invocation body, so an async-event consumer sends the literal string "undefined".
//
// Semantics, quoted from /runtime-reference/realtime-events-api/ and /apis-reference/ui-api-bridge/realtime/
// ("Last updated Jun 25, 2026"):
// - publish "Publishes events if there is an existing subscription for the same channel context. The resulting
//   eventId and eventTimestamp will be null if there are no existing subscribers."
// - "a subscription in a module ... will only receive messages that are published from the same module in the
//   same Jira issue"; "events sent by the publish API can only be received by subscriptions created using the
//   subscribe @forge/bridge API, and publishGlobal events can only be received by ... subscribeGlobal".
// - token: "The published event will only be received by subscriptions that have been created with a token
//   containing the same channel context claims"; claims "must match exactly between the publisher and subscriber";
//   permissions default to both subscribe and publish; expiresAt is epoch SECONDS (JWT exp).
// - contextOverrides "will override the existing Atlassian app context scope of the channel".
// - "The publish API is only supported for functions invoked from the app frontend. This is not currently
//   available for async events and web triggers. Use publishGlobal instead".
// - Async events: the limitation sentence above continues "Use publishGlobal instead, alongside the realtimeToken
//   API to apply additional restrictions" — so a consumer's publishGlobal is accepted with or without a token, and
//   signRealtimeToken is answered wherever it is called (the wrapper sends it with no frontend context); every
//   publish and sign is logged with its invoking module type, so the scorer sees which path an app used.
// - Channel names: no syntax is documented; any non-empty name is accepted, matched exactly.
// - /realtime/authorizing-realtime-channels/ (Jun 25, 2026): contextOverrides "must match exactly in the subscribe()
//   and publish() calls"; "If it's provided as an empty array, then the channel will not be secured by any Atlassian
//   app context values"; global channels suit "publishing messages from a Forge function that isn't associated with a
//   UI context, for example functions for Atlassian app events"; "publishing to non-global channels is only supported
//   for functions invoked from the app frontend".
// - /realtime/error-handling-for-realtime-methods/ (Jun 25, 2026): publish returns PublishResult with `errors`;
//   token pre-validation messages "Realtime token validation failed: INVALID_TOKEN|TOKEN_EXPIRED|
//   CHANNEL_NAME_MISMATCH|MISSING_PERMISSION"; a rejected publish: "Error publishing event to channel".
// PAYLOAD a subscriber receives: @forge/realtime always sends `payload: JSON.stringify(eventPayload)` (publish.js),
// strings included, so the wire value of publish('c', 'hi') is '"hi"'. The bridge docs type the callback
// `(payload: string | Record<string, unknown>)` and say it "takes a string or JSON payload", and the docs' own
// examples publish plain strings ('Here is an event payload!') to such callbacks. A subscriber can only receive a
// plain string for a string publish if the platform JSON-parses the wire payload — which also turns an object
// publish back into an object. So deliveries carry the PARSED payload (object in, object out; string in, string
// out). The parse happens in Atlassian's host, which is not public (R6: inferred from the SDK and the docs).
// INFERRED (R6, not measured): that a non-global publish without a frontend context fails with the documented
// publish error (logged `rejected: PUBLISH_WITHOUT_FRONTEND_CONTEXT`), and the token lifetime.
const crypto = require('crypto');

const TOKEN_TTL_S = 3600; // harness policy: no doc states a realtime token's lifetime; one hour outlives any graded session
const PRODUCT_CONTEXT = ['board', 'issue', 'project', 'content', 'space', 'repository', 'pullRequest'];

const parsePayload = (wire) => { try { return JSON.parse(wire); } catch { return wire; } };
const b64 = (o) => Buffer.from(JSON.stringify(o)).toString('base64url');
const stable = (v) => (v === null || typeof v !== 'object' ? JSON.stringify(v ?? null)
  : Array.isArray(v) ? `[${v.map(stable).join(',')}]` : `{${Object.keys(v).sort().map((k) => `${JSON.stringify(k)}:${stable(v[k])}`).join(',')}}`);

function createRealtime({ now, secret }) {
  const subscriptions = new Map();
  const contexts = new Map();
  const events = [];      // every publish attempt, delivered or not (the scorer's realtime log)
  const deliveries = [];  // {seq, subscriptionId, origin, payload} for remote pollers
  const listeners = new Set();
  let seq = 0;
  let eventSeq = 0;

  const sign = (payload) => {
    const head = b64({ alg: 'HS256', typ: 'JWT' });
    const body = b64(payload);
    return `${head}.${body}.${crypto.createHmac('sha256', secret).update(`${head}.${body}`).digest('base64url')}`;
  };
  const verify = (token, channelName, need) => {
    const parts = String(token).split('.');
    if (parts.length !== 3) return { error: 'INVALID_TOKEN' };
    const mac = crypto.createHmac('sha256', secret).update(`${parts[0]}.${parts[1]}`).digest('base64url');
    if (mac !== parts[2]) return { error: 'INVALID_TOKEN' };
    const p = JSON.parse(Buffer.from(parts[1], 'base64url').toString());
    if (p.exp <= now() / 1000) return { error: 'TOKEN_EXPIRED' };
    if (p.channel?.name !== channelName) return { error: 'CHANNEL_NAME_MISMATCH' };
    if (p.permissions && !p.permissions.includes(need)) return { error: 'MISSING_PERMISSION' };
    return { claims: p.claims };
  };

  // A frontend context: the module and the product context the surface runs in (extension ids).
  const idOf = (v) => (v && typeof v === 'object' ? String(v.id ?? v.key ?? stable(v)) : v === undefined ? null : String(v));
  const contextKey = (ctx, overrides) => {
    const ext = ctx?.extension ?? {};
    if (Array.isArray(overrides)) return stable({ installation: true, overrides: [...overrides].sort(), ...Object.fromEntries(overrides.map((p) => [p, idOf(ext[p])])) });
    return stable({ moduleKey: ctx?.moduleKey ?? null, ...Object.fromEntries(PRODUCT_CONTEXT.map((p) => [p, idOf(ext[p])]).filter(([, v]) => v !== null)),
      dashboard: ext.context?.dashboardId ?? null });
  };
  const mintContext = (ctx) => {
    const token = `rtctx-${crypto.randomBytes(12).toString('hex')}`;
    contexts.set(token, ctx);
    return token;
  };

  function signToken({ channelName, claims, permissions }) {
    const exp = Math.floor(now() / 1000) + TOKEN_TTL_S;
    const perms = permissions && permissions.length ? permissions : null;
    return { jwt: sign({ channel: { name: channelName }, claims: claims ?? null, ...(perms ? { permissions: perms } : {}), exp }), expiresAt: exp };
  }

  function subscribe({ channelName, isGlobal = false, token, contextOverrides, replaySeconds, frontend, origin }) {
    if (typeof channelName !== 'string' || !channelName) return { errors: [{ message: 'A realtime channel name must be a non-empty string.' }] };
    let claims = null;
    if (token) {
      const v = verify(token, channelName, 'subscribe');
      if (v.error) return { errors: [{ message: `Realtime token validation failed: ${v.error}` }] };
      claims = v.claims;
    }
    const id = `rtsub-${crypto.randomBytes(6).toString('hex')}`;
    const sub = { id, channelName, isGlobal: Boolean(isGlobal), claims: stable(claims), hasToken: Boolean(token),
      context: isGlobal ? null : contextKey(frontend, contextOverrides), origin: origin ?? null, at: new Date(now()).toISOString() };
    subscriptions.set(id, sub);
    if (replaySeconds > 0) {
      const since = now() - replaySeconds * 1000;
      for (const e of events) if (e.ms >= since && e.eventId && matches(sub, e)) deliver(sub, e);
    }
    return { subscriptionId: id };
  }

  const matches = (sub, e) => sub.channelName === e.channel && sub.isGlobal === e.isGlobal && sub.claims === e.claims
    && sub.hasToken === e.hasToken && (sub.isGlobal || sub.context === e.context);
  const deliver = (sub, e) => {
    const d = { seq: ++seq, subscriptionId: sub.id, origin: sub.origin, eventId: e.eventId, channel: e.channel, payload: parsePayload(e.payload), wire: e.payload, at: new Date(now()).toISOString() };
    deliveries.push(d);
    for (const fn of listeners) fn(d);
    return d;
  };

  // {channelName, payload: string, isGlobal, token, contextToken, contextOverrides, origin} -> PublishResult
  function publish({ channelName, payload, isGlobal = false, token, contextToken, contextOverrides, origin = {} }) {
    const t = now();
    const e = { seq: ++eventSeq, ms: t, at: new Date(t).toISOString(), channel: channelName, isGlobal: Boolean(isGlobal), payload,
      origin, hasToken: Boolean(token), claims: stable(null), context: null, delivered: [], eventId: null, eventTimestamp: null };
    events.push(e);
    const fail = (reason, message) => { e.rejected = reason; e.errors = [{ message, extensions: { errorType: reason } }]; return { eventId: null, eventTimestamp: null, errors: e.errors }; };
    if (typeof channelName !== 'string' || !channelName) return fail('INVALID_CHANNEL_NAME', 'A realtime channel name must be a non-empty string.');
    if (token) {
      const v = verify(token, channelName, 'publish');
      if (v.error) return fail(v.error, `Realtime token validation failed: ${v.error}`);
      e.claims = stable(v.claims);
    }
    if (!isGlobal) {
      const ctx = contexts.get(contextToken);
      if (!ctx) {
        return fail('PUBLISH_WITHOUT_FRONTEND_CONTEXT', 'Error publishing event to channel');
      }
      e.context = contextKey(ctx, contextOverrides);
    }
    const targets = [...subscriptions.values()].filter((sub) => matches(sub, e));
    if (targets.length) { e.eventId = crypto.randomUUID(); e.eventTimestamp = String(t); }
    for (const sub of targets) e.delivered.push(deliver(sub, e).subscriptionId);
    return { eventId: e.eventId, eventTimestamp: e.eventTimestamp };
  }

  return {
    signToken, subscribe, publish, mintContext,
    unsubscribe: (id) => subscriptions.delete(id),
    deliveriesSince: (since = 0, origin = null) => deliveries.filter((d) => d.seq > since && (!origin || d.origin === origin)),
    onDeliver: (fn) => { listeners.add(fn); return () => listeners.delete(fn); },
    events,
    eventsSince: (since = 0) => ({ events: events.filter((e) => e.seq > since), next: eventSeq }),
    subscriptions: () => [...subscriptions.values()],
    // The site's reset (forge-dev reset) clears the publish log but keeps live subscriptions and the delivery
    // sequence: a page served in another process keeps its subscription and its poll position.
    reset: () => { events.length = 0; deliveries.length = 0; },
  };
}

module.exports = { createRealtime, TOKEN_TTL_S };
