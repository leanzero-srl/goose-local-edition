'use strict';
// Forge LLM, emulated on the dev/scoring site: the endpoint the runtime wrapper reaches for `@forge/llm`.
// MEASURED 2026-10-03 (Atlassian's pinned wrapper, 4f8170e0…): `list()` sends GET `${proxy}/llm/`, `chat()`/
// `stream()` send POST `${proxy}/llm/<model>` with the request body plus `stream: false|true`, the invocation's
// proxy bearer, `x-b3-traceid`/`x-b3-spanid`, and no `forge-proxy-target`. The kit's proxy forwards both here.
//
// Shapes are @forge/llm 1.0.7's (out/interfaces/internal.d.ts; docs /runtime-reference/forge-llms-api-reference/,
// "Last updated Aug 3, 2026"): ModelListResponse {models:[{model, status: 'active'|'deprecated'}]}, LlmResponse
// {choices:[{finish_reason, index, message:{role:'assistant', content, tool_calls?}}], usage?}, ToolCall
// {id, type:'function', index, function:{name, arguments: object}} (internal.d.ts:57-63). Assistant `content` is a
// TextPart array and a tool answer carries text beside its tool_calls with finish_reason "tool_use", as in the
// package README's chat response example (README.md:95-127).
//
// ERRORS: @forge/llm reads a JSON body with `code` and `message` as a Forge error (out/utils/error-handling.js:11-13,
// 22) and throws ForgeLlmAPIError{code, message, status, statusText, traceId}; that is the shape answered here.
// VALIDATION, quoted from the reference page's "Validation rules": "temperature and top_p cannot be specified
// together. Provide only one, not both."; "The following models do not support the temperature and top_p sampling
// parameters. Omit both parameters from requests to these models: claude-opus-4-7 claude-opus-4-8 claude-opus-5
// claude-sonnet-5". The status code and `code` strings of these refusals are not documented (harness choice: 400).
//
// The model is a SCRIPTED fake (DESIGN §5.2, R6): it grades how an app handles answers, never model quality.
// Each chat/stream call takes the next step of the script; `phase(name)` restarts it (the scorer calls it per phase):
//   clean     tool call: digit-free summary, changeIds = the change ids the prompt itself carries
//   digits    tool call: a summary WITH digits, ids = one from the prompt + one hidden from the caller + one unknown
//   refusal   finish_reason 'refusal', text, no tool call
//   malformed tool call whose arguments do not match the tool's schema (summary not a string, changeIds not an array)
//   error     500, {code: 'INTERNAL_SERVER_ERROR'} -> ForgeLlmAPIError{status: 500} (DESIGN §5.2)
// then `clean` for every later call.
//
// STREAMING (stream()): the body is newline-delimited JSON, each line one ChatResponse — @forge/llm splits on '\n'
// (llm-stream-parser.js:5, 15-21), parses each line (53-55), keeps a line cut across network reads as a fragment and
// joins it with the next read (25-32, 45-50), and flushes a last line without a newline (36-41); each parsed line is
// yielded as a ChatResponse (stream-response-wrapper.js:41-48, .d.ts:10). The README's stream chunk (README.md:174-
// 195) is a TEXT DELTA (`content: [{type:'text', text:'The sun rises in the east, '}]`) with no finish_reason, and the
// errors page (/runtime-reference/forge-llms-api-errors/, Jul 23, 2026) detects a complete stream by a chunk whose
// choice has a finish_reason. So the harness streams: text deltas; then, for a tool answer, one delta per argument
// key — a ToolCall with the same `id` and `index` (internal.d.ts:57-63 types `arguments` as an object and gives each
// call an `index`, so deltas are partial argument OBJECTS to be merged by index); then a final chunk with
// finish_reason and usage. The proxy writes one line cut in two network writes, so the parser's fragment path runs.
// How the real service splits tool arguments is NOT documented (R6): an app that merges by index/id and also accepts
// a whole call in one chunk handles both readings.
const crypto = require('crypto');
const { createRng } = require('./rng.cjs');

const SCRIPT = ['clean', 'digits', 'refusal', 'malformed', 'error'];
// /runtime-reference/forge-llms-models/ ("Last updated Aug 3, 2026"), "Supported models", in the page's order, every
// one "ACTIVE": claude-haiku-4-5-20251001, claude-sonnet-4-5-20250929, claude-sonnet-4-6, claude-sonnet-5,
// claude-opus-4-6, claude-opus-4-7, claude-opus-4-8, claude-opus-5. The page marks NO model deprecated, so list()
// reports none (the SDK's status values are 'active' | 'deprecated', internal.d.ts).
const MODELS = ['claude-haiku-4-5-20251001', 'claude-sonnet-4-5-20250929', 'claude-sonnet-4-6', 'claude-sonnet-5',
  'claude-opus-4-6', 'claude-opus-4-7', 'claude-opus-4-8', 'claude-opus-5'];
const NO_SAMPLING = new Set(['claude-opus-4-7', 'claude-opus-4-8', 'claude-opus-5', 'claude-sonnet-5']);

function createLlm({ pack, now }) {
  const r = createRng(crypto.createHash('sha256').update(`llm:${pack.seed}`).digest('hex').slice(0, 16));
  const models = MODELS.map((m) => ({ model: m, status: 'active' }));
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

  const parts = (t) => [{ type: 'text', text: t }];
  // -> {final: LlmResponse (chat), chunks: LlmResponse[] (stream)}
  function answer(kind, body, viewer) {
    const tools = (body.tools ?? []).filter((t) => t?.type === 'function' && t.function?.name);
    const choice = body.tool_choice;
    const forced = choice && typeof choice === 'object' ? choice.function?.name : null;
    const tool = forced ?? (choice === 'none' ? null : tools[0]?.function?.name ?? null);
    const cited = idsIn(promptText(body));
    let text;
    let args = null;
    let finish;
    if (kind === 'refusal') { text = "I can't help with that request."; finish = 'refusal'; }
    else if (!tool) { text = 'Scope grew after the sprint started: work was added while some planned items moved out.'; finish = 'end_turn'; }
    else {
      text = `I'll report the sprint's scope change with ${tool}.`;
      finish = 'tool_use';
      args = kind === 'clean' ? { summary: 'Scope grew after the sprint started: work was added while some planned items moved out.', changeIds: cited.slice(0, 3) }
        : kind === 'digits' ? { summary: 'Scope grew by 13 points: 5 issues were added and 2 removed after day 3.', changeIds: [cited[0], hiddenFor(viewer, cited), unknownId].filter(Boolean) }
          : { summary: ['Scope grew'], changeIds: 'see the ledger' };
    }
    const call = args && toolCall(tool, args);
    const final = { choices: [{ index: 0, finish_reason: finish, message: { role: 'assistant', content: parts(text), ...(call ? { tool_calls: [call] } : {}) } }] };
    const words = text.split(/(?<= )/);
    const half = Math.ceil(words.length / 2);
    const delta = (m) => ({ choices: [{ index: 0, message: { role: 'assistant', content: [], ...m } }] });
    const chunks = [delta({ content: parts(words.slice(0, half).join('')) }), delta({ content: parts(words.slice(half).join('')) })];
    if (call) for (const [k, v] of Object.entries(args)) chunks.push(delta({ tool_calls: [{ ...call, function: { name: call.function.name, arguments: { [k]: v } } }] }));
    chunks.push({ choices: [{ index: 0, finish_reason: finish, message: { role: 'assistant', content: [] } }] });
    return { final, chunks };
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
    if (body.temperature !== undefined && body.top_p !== undefined) {
      return done(400, { code: 'INVALID_REQUEST', message: 'Invalid request: temperature and top_p cannot be specified together. Provide only one, not both.' });
    }
    if (NO_SAMPLING.has(model) && (body.temperature !== undefined || body.top_p !== undefined)) {
      return done(400, { code: 'INVALID_REQUEST', message: `Invalid request: ${model} does not support the temperature and top_p sampling parameters. Omit both parameters from requests to this model.` });
    }
    const kind = SCRIPT[step] ?? 'clean';
    step += 1;
    entry.step = kind;
    if (kind === 'error') return done(500, { code: 'INTERNAL_SERVER_ERROR', message: 'The Forge LLM service failed to process the request.' });
    const { final, chunks } = answer(kind, body, caller.asUser ?? pack.viewer);
    final.usage = usage(body, final);
    if (!body.stream) return done(200, final);
    chunks[chunks.length - 1].usage = final.usage;
    entry.chunks = chunks;
    return { ...done(200, final), stream: chunks };
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

module.exports = { createLlm, SCRIPT, MODELS, NO_SAMPLING };
