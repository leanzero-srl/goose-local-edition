'use strict';
// Forge LLM, emulated on the dev/scoring site: the endpoint the runtime wrapper reaches for `@forge/llm`.
// MEASURED 2026-10-03 (Atlassian's pinned wrapper, 4f8170e0…): `list()` sends GET `${proxy}/llm/`, `chat()`/
// `stream()` send POST `${proxy}/llm/<model>` with the request body plus `stream: false|true`, the invocation's
// proxy bearer, `x-b3-traceid`/`x-b3-spanid`, and no `forge-proxy-target`. The kit's proxy forwards both here.
//
// Shapes are @forge/llm 1.0.7's (out/interfaces/internal.d.ts; docs /runtime-reference/forge-llms-api-reference/,
// "Last updated Aug 3, 2026"): ModelListResponse {models:[{model, status: 'active'|'deprecated'}]}, LlmResponse
// {choices:[{finish_reason, index, message:{role:'assistant', content, tool_calls?}}], usage?}, ToolCall
// {id, type:'function', index, function:{name, arguments: object}}. An error answers {code, message}, which
// @forge/llm turns into ForgeLlmAPIError{code, status, statusText, traceId} (out/utils/error-handling.js).
// Validation rules from the docs page: temperature and top_p never together; claude-opus-4-7, claude-opus-4-8,
// claude-opus-5 and claude-sonnet-5 accept neither.
//
// The model is a SCRIPTED fake (DESIGN §5.2, R6): it grades how an app handles answers, never model quality.
// Each chat call takes the next step of the script; `phase(name)` restarts it (the scorer calls it per phase):
//   clean     tool call: digit-free summary, changeIds = the change ids the prompt itself carries
//   digits    tool call: a summary WITH digits, ids = one from the prompt + one hidden from the caller + one unknown
//   refusal   finish_reason 'refusal', text, no tool call
//   malformed tool call whose arguments do not match the tool's schema (summary not a string, changeIds not an array)
//   error     500, {code: 'INTERNAL_SERVER_ERROR'} -> ForgeLlmAPIError{status: 500} (DESIGN §5.2)
// then `clean` for every later call. stream() gets the same answer as ONE chunk: @forge/llm reads the response body as
// newline-separated JSON and yields each line as a whole ChatResponse (out/streaming/llm-stream-parser.js:5,17,53;
// out/streaming/stream-response-wrapper.js:41-48; stream-response-wrapper.d.ts:10 `AsyncIterable<ChatResponse>`),
// and a ToolCall's `arguments` is an object (interfaces/internal.d.ts), so a tool call cannot arrive split; how
// the real service chunks text is undocumented, so the harness does not split it either. INFERRED, not measured: finish_reason values ('tool_use', 'end_turn',
// 'refusal' as Anthropic models name them), the error code strings, and the model names' statuses.
const crypto = require('crypto');
const { createRng } = require('./rng.cjs');

const SCRIPT = ['clean', 'digits', 'refusal', 'malformed', 'error'];
// Names from the docs page (its example `claude-opus-4-6`, its validation-rule list); which are deprecated is the
// harness's choice: the docs' own example is deprecated, so copying it is the trap (DESIGN §2.4).
const MODELS = { active: ['claude-opus-5', 'claude-sonnet-5'], deprecated: ['claude-opus-4-6', 'claude-opus-4-7'] };
const NO_SAMPLING = new Set(['claude-opus-4-7', 'claude-opus-4-8', 'claude-opus-5', 'claude-sonnet-5']);

function createLlm({ pack, now }) {
  const r = createRng(crypto.createHash('sha256').update(`llm:${pack.seed}`).digest('hex').slice(0, 16));
  const models = r.shuffle([...MODELS.active.map((m) => ({ model: m, status: 'active' })), ...MODELS.deprecated.map((m) => ({ model: m, status: 'deprecated' }))]);
  const changes = [...pack.history, ...pack.live];
  const changeById = new Map(changes.map((c) => [String(c.changelogId), c]));
  const issueById = new Map(pack.issues.map((i) => [i.id, i]));
  const maxId = Math.max(...changes.map((c) => Number(c.changelogId)));
  const unknownId = String(maxId + 7919 + r.int(1, 999));
  const log = [];
  let phase = 'default';
  let step = 0;

  const text = (content) => (typeof content === 'string' ? content : Array.isArray(content) ? content.map((p) => p?.text ?? '').join(' ') : '');
  const promptText = (body) => (body.messages ?? []).map((m) => text(m.content)).join('\n');
  const idsIn = (s) => [...new Set((s.match(/\b\d{3,}\b/g) ?? []).filter((x) => changeById.has(x)))];
  const sprintsOf = (c) => c.items.filter((it) => it.field === 'Sprint').flatMap((it) => [it.from, it.to].join(',').split(/[,\s]+/)).filter(Boolean);
  const hiddenFor = (viewer, cited) => {
    const sprints = new Set(cited.flatMap((id) => sprintsOf(changeById.get(id))));
    const hidden = changes.filter((c) => issueById.get(c.issueId)?.hiddenFrom?.includes(viewer))
      .sort((a, b) => Number(a.changelogId) - Number(b.changelogId));
    const same = hidden.find((c) => sprintsOf(c).some((s) => sprints.has(s)));
    return String((same ?? hidden[0])?.changelogId ?? unknownId);
  };
  const usage = (body, out) => {
    const input = Math.ceil(JSON.stringify(body.messages ?? []).length / 4);
    const output = Math.ceil(JSON.stringify(out).length / 4);
    return { input_tokens: input, output_tokens: output, total_tokens: input + output };
  };
  const toolCall = (tool, args) => ({ id: `toolu_${crypto.createHash('sha256').update(`${pack.seed}:${log.length}`).digest('hex').slice(0, 24)}`,
    type: 'function', index: 0, function: { name: tool, arguments: args } });

  function answer(kind, body, viewer) {
    const tools = (body.tools ?? []).filter((t) => t?.type === 'function' && t.function?.name);
    const choice = body.tool_choice;
    const forced = choice && typeof choice === 'object' ? choice.function?.name : null;
    const tool = forced ?? (choice === 'none' ? null : tools[0]?.function?.name ?? null);
    const cited = idsIn(promptText(body));
    const clean = { summary: 'Scope grew after the sprint started: work was added while some planned items moved out.', changeIds: cited.slice(0, 3) };
    const message = (m, finish) => ({ choices: [{ finish_reason: finish, index: 0, message: { role: 'assistant', ...m } }] });
    if (kind === 'refusal') return message({ content: "I can't help with that request." }, 'refusal');
    if (!tool) return message({ content: clean.summary }, 'end_turn');
    if (kind === 'clean') return message({ content: '', tool_calls: [toolCall(tool, clean)] }, 'tool_use');
    if (kind === 'digits') {
      const ids = [cited[0], hiddenFor(viewer, cited), unknownId].filter(Boolean);
      return message({ content: '', tool_calls: [toolCall(tool, { summary: 'Scope grew by 13 points: 5 issues were added and 2 removed after day 3.', changeIds: ids })] }, 'tool_use');
    }
    return message({ content: '', tool_calls: [toolCall(tool, { summary: ['Scope grew'], changeIds: 'see the ledger' })] }, 'tool_use');
  }

  // -> {status, body, headers?, stream?}
  function handle({ method, model, body, caller = {} }) {
    const entry = { t: new Date(now()).toISOString(), phase, invocationId: caller.invocationId ?? null, moduleType: caller.moduleType ?? null,
      moduleKey: caller.moduleKey ?? null, functionKey: caller.functionKey ?? null, asUser: caller.asUser ?? null, method, model: model ?? null };
    log.push(entry);
    const done = (status, out, headers) => { Object.assign(entry, { status, response: out }); return { status, body: out, headers }; };
    if (method === 'GET') { entry.op = 'list'; return done(200, { models }); }
    entry.op = body?.stream ? 'stream' : 'chat';
    entry.request = body;
    const listed = models.find((m) => m.model === model);
    entry.modelStatus = listed?.status ?? 'unknown';
    if (!listed) return done(404, { code: 'MODEL_NOT_FOUND', message: `Model '${model}' is not available to Forge LLMs.` });
    if (body.temperature !== undefined && body.top_p !== undefined) return done(400, { code: 'INVALID_REQUEST', message: 'temperature and top_p cannot be specified together. Provide only one, not both.' });
    if (NO_SAMPLING.has(model) && (body.temperature !== undefined || body.top_p !== undefined)) return done(400, { code: 'INVALID_REQUEST', message: `${model} does not support the temperature and top_p sampling parameters.` });
    const kind = SCRIPT[step] ?? 'clean';
    step += 1;
    entry.step = kind;
    if (kind === 'error') return done(500, { code: 'INTERNAL_SERVER_ERROR', message: 'The Forge LLM service failed to process the request.' });
    const out = answer(kind, body, caller.asUser ?? pack.viewer);
    out.usage = usage(body, out);
    return { ...done(200, out), stream: Boolean(body.stream) };
  }

  return {
    handle,
    log,
    models: () => models.map((m) => ({ ...m })),
    phase: (name) => { phase = String(name ?? 'default'); step = 0; return { phase, script: SCRIPT }; },
    state: () => ({ phase, step, next: SCRIPT[step] ?? 'clean', script: SCRIPT, unknownId }),
    reset: () => { log.length = 0; phase = 'default'; step = 0; },
  };
}

module.exports = { createLlm, SCRIPT, MODELS };
