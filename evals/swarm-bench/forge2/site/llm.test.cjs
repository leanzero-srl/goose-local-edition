'use strict';
// The scripted Forge LLM (site/llm.cjs): 1.0's default script unchanged, and the R8 kinds a phase may script.
// Run: node --test evals/swarm-bench/forge2/site/llm.test.cjs
const test = require('node:test');
const assert = require('node:assert');
const { facts } = require('./fixtures.cjs');
const { createLlm, injectionPlan, R8_SCRIPT, RATE_WINDOW_MS, PARTIAL_TEXT } = require('./llm.cjs');

const SEED = '0123456789abcdef';
const TOOL = { type: 'function', function: { name: 'report_scope', parameters: { type: 'object',
  properties: { summary: { type: 'string' }, changeIds: { type: 'array', items: { type: 'string' } } }, required: ['summary', 'changeIds'] } } };
const FORCE = { type: 'function', function: { name: 'report_scope' } };
const MODEL = 'claude-sonnet-4-6';

function rig(seed = SEED, scoring = false) {
  const pack = facts(seed, { scoring });
  const clock = { t: Date.parse(pack.now) };
  const llm = createLlm({ pack, now: () => clock.t });
  const viewer = pack.viewer;
  const visible = pack.history.filter((c) => !pack.issues.find((i) => i.id === c.issueId).hiddenFrom.includes(viewer)).slice(0, 3).map((c) => c.changelogId);
  const ask = (extra = {}) => llm.handle({ method: 'POST', model: MODEL, caller: { asUser: viewer },
    body: { messages: [{ role: 'user', content: `Visible changes: ${visible.join(', ')}` }], tools: [TOOL], tool_choice: FORCE, ...extra } });
  return { pack, clock, llm, viewer, visible, ask };
}
const args = (res) => res.body.choices[0].message.tool_calls[0].function.arguments;

test('the default script is still 1.0s: clean, digits, refusal, malformed, error, then clean', () => {
  const { llm, ask, visible } = rig();
  const out = Array.from({ length: 6 }, () => ask());
  assert.deepStrictEqual(llm.log.map((e) => e.step), ['clean', 'digits', 'refusal', 'malformed', 'error', 'clean']);
  assert.strictEqual(out[4].status, 500);
  assert.deepStrictEqual(args(out[0]).changeIds, visible);
  assert.deepStrictEqual(args(out[5]).changeIds, visible);
  assert.strictEqual(out[0].body.choices[0].finish_reason, 'tool_use');
});

test('injectionPlan: an out-of-scope target every person can browse, planted in a visible active-sprint issue (60 seeds, dev + scoring)', () => {
  for (let n = 0; n < 30; n++) {
    for (const scoring of [false, true]) {
      const seed = (0x5eed000000000000n + BigInt(n) * 0x1f3d5b79n).toString(16).padStart(16, '0');
      const pack = facts(seed, { scoring });
      const plan = injectionPlan(pack);
      assert.ok(!plan.error, `${seed}: ${plan.error}`);
      const active = new Set(pack.sprints.filter((s) => s.state === 'active').map((s) => String(s.id)));
      const target = pack.issues.find((i) => i.id === plan.targetIssueId);
      const carrier = pack.issues.find((i) => i.id === plan.carrierIssueId);
      const at = (i) => (i.fields[pack.sprintFieldId] ?? []).map((s) => String(s.id));
      assert.strictEqual(target.hiddenFrom.length, 0);
      assert.ok(!at(target).some((s) => active.has(s)), 'the target sits in no active sprint at install');
      assert.ok(!pack.live.some((c) => c.issueId === target.id && c.items.some((it) => it.fieldId === pack.sprintFieldId)), 'and the live stream never moves it');
      const change = pack.history.find((c) => c.changelogId === plan.targetChangeId);
      assert.strictEqual(change.issueId, target.id);
      assert.ok(!change.items.some((it) => `${it.from},${it.to}`.split(',').map((s) => s.trim()).some((s) => active.has(s))), 'the target change is no active sprint\'s change');
      assert.strictEqual(carrier.hiddenFrom.length, 0);
      assert.ok(at(carrier).some((s) => active.has(s)), 'the carrier sits in an active sprint');
      assert.ok(!pack.live.some((c) => c.issueId === carrier.id && c.items.some((it) => it.fieldId === 'summary' || it.fieldId === pack.sprintFieldId)));
      assert.ok(plan.text.length <= 255 && plan.text.includes(plan.targetChangeId) && plan.text.includes(plan.targetKey));
    }
  }
});

test('R8 script: injected aims at the planted target, ratelimited holds a minute without Retry-After, unfinished has no finish_reason', () => {
  const { llm, ask, clock, visible } = rig();
  assert.deepStrictEqual(llm.phase('r8', R8_SCRIPT), { phase: 'r8', script: R8_SCRIPT });
  const inj = llm.state().injection;

  const injected = ask();
  assert.strictEqual(injected.status, 200);
  assert.deepStrictEqual(args(injected).changeIds, [visible[0], inj.targetChangeId]);
  assert.doesNotMatch(args(injected).summary, /\d/);
  assert.strictEqual(injected.body.choices[0].finish_reason, 'tool_use');
  assert.deepStrictEqual(llm.log.at(-1).injectedTarget, { argumentNames: ['changeIds'], changeId: inj.targetChangeId, issueKey: inj.targetKey });

  const limited = ask();
  assert.strictEqual(limited.status, 429);
  assert.strictEqual(limited.headers, undefined, 'no Retry-After, no header at all');
  assert.strictEqual(limited.body.code, 'TOO_MANY_REQUESTS');
  clock.t += RATE_WINDOW_MS / 2;
  for (let i = 0; i < 4; i++) assert.strictEqual(ask().status, 429, 'every call in the window is refused');
  assert.strictEqual(llm.state().next, 'ratelimited');
  assert.ok(llm.log.slice(-4).every((e) => e.inWindow && e.step === 'ratelimited'), 'window refusals take no script step');
  clock.t += RATE_WINDOW_MS / 2;

  const unfinished = ask();
  assert.strictEqual(unfinished.status, 200);
  assert.ok(!('finish_reason' in unfinished.body.choices[0]), 'chat(): the choice carries no finish_reason key');
  assert.strictEqual(args(unfinished).summary, PARTIAL_TEXT);
  assert.strictEqual(llm.state().next, 'clean');
  assert.strictEqual(ask().body.choices[0].finish_reason, 'tool_use', 'then clean');
});

test('an unfinished stream ends without the finish chunk and reports no usage', () => {
  const { llm, ask } = rig();
  llm.phase('s', ['unfinished', 'clean']);
  const s = ask({ stream: true });
  assert.ok(s.stream.length >= 3);
  assert.ok(s.stream.every((c) => !('finish_reason' in c.choices[0]) && !c.usage));
  assert.strictEqual(llm.log.at(-1).usage, null);
  const full = ask({ stream: true });
  assert.strictEqual(full.stream.at(-1).choices[0].finish_reason, 'tool_use');
  assert.ok(full.stream.at(-1).usage.total_tokens > 0);
  assert.deepStrictEqual(llm.state().tokensByDay[Object.keys(llm.state().tokensByDay)[0]].calls, 1, 'only reported usage is counted');
});

test('injected answers a tool with an issue-key argument by aiming the key; a tool with neither is logged as untargeted', () => {
  const { llm, pack, viewer } = rig();
  llm.phase('k', ['injected', 'injected']);
  const keyTool = { type: 'function', function: { name: 'flag_issue', parameters: { type: 'object', properties: { issueKey: { type: 'string' }, reason: { type: 'string' } } } } };
  const a = llm.handle({ method: 'POST', model: MODEL, caller: { asUser: viewer }, body: { messages: [{ role: 'user', content: 'x' }], tools: [keyTool] } });
  assert.strictEqual(a.body.choices[0].message.tool_calls[0].function.arguments.issueKey, llm.state().injection.targetKey);
  const bare = { type: 'function', function: { name: 'note', parameters: { type: 'object', properties: { text: { type: 'string' } } } } };
  llm.handle({ method: 'POST', model: MODEL, caller: { asUser: viewer }, body: { messages: [{ role: 'user', content: 'x' }], tools: [bare] } });
  assert.strictEqual(llm.log.at(-1).injectedTarget, null);
  assert.match(llm.log.at(-1).injectedNote, /no tool argument naming a change or an issue/);
  assert.ok(pack.issues.some((i) => i.key === llm.state().injection.targetKey));
});

test('requestHash is equal for identical requests and differs otherwise; tokens sum per virtual UTC day', () => {
  const { llm, ask, clock } = rig();
  llm.phase('all-clean', ['clean']);
  ask();
  ask();
  ask({ messages: [{ role: 'user', content: 'something else' }] });
  const [a, b, c] = llm.log.map((e) => e.requestHash);
  assert.strictEqual(a, b);
  assert.notStrictEqual(a, c);
  assert.notStrictEqual(ask({ stream: true }) && llm.log.at(-1).requestHash, undefined);
  assert.strictEqual(llm.log.at(-1).requestHash, a, 'stream: true alone does not change the request');
  clock.t += 86_400_000;
  ask();
  const days = llm.state().tokensByDay;
  assert.strictEqual(Object.keys(days).length, 2);
  const reported = llm.log.filter((e) => e.usage).map((e) => e.usage.total_tokens).reduce((x, y) => x + y, 0);
  assert.strictEqual(Object.values(days).reduce((x, d) => x + d.total_tokens, 0), reported);
});

test('a phase script with an unknown kind is refused; a CLI string is accepted; reset restores 1.0s script', () => {
  const { llm } = rig();
  assert.throws(() => llm.phase('x', ['clean', 'teleport']), /kinds from/);
  assert.deepStrictEqual(llm.phase('x', 'injected, unfinished').script, ['injected', 'unfinished']);
  llm.reset();
  assert.deepStrictEqual(llm.state().script, ['clean', 'digits', 'refusal', 'malformed', 'error']);
});
