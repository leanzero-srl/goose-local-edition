/**
 * The run's token usage as leanzero.net stores it (`tokenUsage`, website src/lib/benchmark-token-usage.ts): every
 * model call of the run (the entrant's and the harness's helper calls), split by model. The owner compares it with
 * OpenRouter's own per-generation records, so for an OpenRouter run it IS those records: native prompt / completion /
 * cached / reasoning tokens summed per generation id ("provider-records"). Before this, Forge runs published no token
 * counts at all and Gauntlet runs only goose's own per-node counters.
 *
 * Sources, best first:
 *  - provider-records: OpenRouter's `GET /api/v1/generation?id=` record for every generation id the run kept (read
 *    again at publish: the harness reads the same records for the bill but keeps only their totals, and a record
 *    that was late at run end is usually there by then). Exact when every call has its record.
 *  - provider-records from the harness bill: the totals the harness summed from the same records, split by model
 *    only where the bill itself proves the split; used when the records cannot be read again.
 *  - engine-telemetry: a run on this Mac's engine — the engine's own usage per call (`.swarm/telemetry.jsonl`).
 *  - session-counters: a cloud provider with no per-call records (Google direct) — the API usage goose summed per call.
 * A field the source does not report (a local engine's reasoning count) is null for every model; a field known for
 * some models only makes the record a lower bound (exact = false), never a guessed zero.
 */

export type TokenUsageSource =
  | 'provider-records'
  | 'session-counters'
  | 'engine-telemetry'
  | 'estimate';

export interface TokenUsageModel {
  model: string;
  promptTokens: number;
  completionTokens: number;
  cachedTokens: number | null;
  reasoningTokens: number | null;
  calls: number;
}

export interface TokenUsage {
  promptTokens: number;
  completionTokens: number;
  cachedTokens: number | null;
  reasoningTokens: number | null;
  /** prompt + completion (reasoning is reported as the provider gives it and is not added). */
  totalTokens: number;
  calls: number;
  source: TokenUsageSource;
  exact: boolean;
  /** Calls of the run whose token counts are not in the sums (counted in `calls`). */
  missingCalls: number;
  note: string | null;
  byModel: TokenUsageModel[];
}

export const TOKEN_USAGE_SOURCES: readonly TokenUsageSource[] = [
  'provider-records',
  'session-counters',
  'engine-telemetry',
  'estimate',
];
const MAX_TOKENS = 1e12;
const MAX_CALLS = 1e7;
const MAX_MODELS = 32;
const GENERATION_ID = /^gen-[0-9]+-[A-Za-z0-9]+$/;

const count = (value: unknown, max = MAX_TOKENS): value is number =>
  typeof value === 'number' && Number.isInteger(value) && value >= 0 && value <= max;

/** One line of the run's `.swarm/telemetry.jsonl` (goose writes one per provider call). */
export interface TelemetryCall {
  model: string | null;
  responseId: string | null;
  /** "completed" | "incomplete" (a dropped stream); null on an engine that predates the field. */
  ended: string | null;
  /** The call's own usage as the provider/engine answered it; null when the call carried none. */
  promptTokens: number | null;
  completionTokens: number | null;
}

export function parseTelemetryCalls(text: string): { calls: TelemetryCall[]; unparseable: number } {
  const calls: TelemetryCall[] = [];
  let unparseable = 0;
  for (const line of text.split('\n')) {
    if (!line.trim()) continue;
    let entry: unknown;
    try {
      entry = JSON.parse(line);
    } catch {
      unparseable++;
      continue;
    }
    if (!entry || typeof entry !== 'object' || Array.isArray(entry)) {
      unparseable++;
      continue;
    }
    const e = entry as Record<string, unknown>;
    const usage = e.usage === true && count(e.prompt_tokens) && count(e.completion_tokens);
    calls.push({
      model: typeof e.model === 'string' && e.model.trim() ? e.model.trim().slice(0, 120) : null,
      responseId: typeof e.response_id === 'string' && e.response_id ? e.response_id : null,
      ended: typeof e.ended === 'string' ? e.ended : null,
      promptTokens: usage ? (e.prompt_tokens as number) : null,
      completionTokens: usage ? (e.completion_tokens as number) : null,
    });
  }
  return { calls, unparseable };
}

/** OpenRouter's generation record, the fields token usage reads (`data` of /api/v1/generation). */
export interface GenerationRecord {
  id?: unknown;
  model?: unknown;
  /** USD OpenRouter billed for the generation. */
  usage?: unknown;
  native_tokens_prompt?: unknown;
  native_tokens_completion?: unknown;
  native_tokens_cached?: unknown;
  native_tokens_reasoning?: unknown;
}

const UNKNOWN_MODEL = '(model not recorded)';

interface Bucket {
  model: string;
  prompt: number;
  completion: number;
  cached: number;
  cachedKnown: number;
  reasoning: number;
  reasoningKnown: number;
  counted: number;
  calls: number;
}

function bucketsToUsage(
  buckets: Map<string, Bucket>,
  source: TokenUsageSource,
  missingCalls: number,
  notes: string[],
  lowerBound = false
): TokenUsage | null {
  const list = [...buckets.values()].filter((b) => b.calls > 0);
  if (!list.length || list.length > MAX_MODELS) return null;
  // Per model: a field known for every counted call is a number; known for none is null; partly known is null too
  // (the model's figure is not whole) and makes the record a lower bound.
  let partial = false;
  const field = (b: Bucket, value: number, known: number): number | null => {
    if (b.counted > 0 && known === b.counted) return value;
    if (known > 0) partial = true;
    return null;
  };
  const byModel: TokenUsageModel[] = list
    .sort(
      (a, b) =>
        b.prompt + b.completion - (a.prompt + a.completion) || a.model.localeCompare(b.model)
    )
    .map((b) => ({
      model: b.model,
      promptTokens: b.prompt,
      completionTokens: b.completion,
      cachedTokens: field(b, b.cached, b.cachedKnown),
      reasoningTokens: field(b, b.reasoning, b.reasoningKnown),
      calls: b.calls,
    }));
  const total = (pick: (m: TokenUsageModel) => number | null): number | null => {
    const known = byModel.map(pick).filter((v): v is number => v !== null);
    if (!known.length) return null;
    if (known.length < byModel.length) partial = true;
    return known.reduce((a, b) => a + b, 0);
  };
  const promptTokens = byModel.reduce((a, m) => a + m.promptTokens, 0);
  const completionTokens = byModel.reduce((a, m) => a + m.completionTokens, 0);
  const cachedTokens = total((m) => m.cachedTokens);
  const reasoningTokens = total((m) => m.reasoningTokens);
  const calls = byModel.reduce((a, m) => a + m.calls, 0);
  if (partial)
    notes.push(
      'Cached or reasoning tokens are known for only part of the calls; those totals are lower bounds.'
    );
  const usage: TokenUsage = {
    promptTokens,
    completionTokens,
    cachedTokens,
    reasoningTokens,
    totalTokens: promptTokens + completionTokens,
    calls,
    source,
    exact: missingCalls === 0 && !partial && !lowerBound && source !== 'estimate',
    missingCalls,
    note: notes.length ? notes.join(' ').slice(0, 400).trim() : null,
    byModel,
  };
  return tokenUsageProblem(usage) ? null : usage;
}

const bucketFor = (buckets: Map<string, Bucket>, model: string): Bucket => {
  let b = buckets.get(model);
  if (!b) {
    b = {
      model,
      prompt: 0,
      completion: 0,
      cached: 0,
      cachedKnown: 0,
      reasoning: 0,
      reasoningKnown: 0,
      counted: 0,
      calls: 0,
    };
    buckets.set(model, b);
  }
  return b;
};

/**
 * Every generation id the run kept, in the harness's order: telemetry first, then the ids its bill found outside
 * telemetry (sessions.db / request logs) and the ids it could not bill.
 */
export function runGenerationIds(calls: TelemetryCall[], billedCost: unknown): string[] {
  const bill =
    billedCost && typeof billedCost === 'object' ? (billedCost as Record<string, unknown>) : {};
  const outside = (bill.telemetry as { ids_outside_telemetry?: unknown } | undefined)
    ?.ids_outside_telemetry;
  const missing = Array.isArray(bill.missing) ? bill.missing : [];
  const ids = [
    ...calls.map((c) => c.responseId),
    ...(Array.isArray(outside) ? outside : []),
    ...missing.map((m) => (m as { id?: unknown } | null)?.id),
  ].filter((id): id is string => typeof id === 'string' && GENERATION_ID.test(id));
  return [...new Set(ids)];
}

/**
 * Token usage from OpenRouter's own generation records. `records[id]` is the record, or null when it could not be
 * read. A call is attributed to the model goose REQUESTED (telemetry), so the split names the entry's model id, not
 * the provider's dated alias; an id the telemetry does not name falls back to the record's own model.
 */
export function tokenUsageFromProviderRecords(input: {
  calls: TelemetryCall[];
  ids: string[];
  records: Record<string, GenerationRecord | null | undefined>;
  /** The harness bill's `generation_ids_collected`: ids it found that this read does not know (it billed some from
   *  sessions.db / request logs that the telemetry does not list) are calls of the run with no tokens here. */
  collected?: number;
  /** The telemetry is the run's per-call record (bill telemetry status "recorded"): only then is a telemetry line
   *  without a generation id a separate call. An older engine's lines carry no ids for calls the bill found by id. */
  telemetryAuthoritative?: boolean;
  /** goose's session counters (`agent.usage`): with the per-call usage, the run's own evidence to reconcile with. */
  usage?: unknown;
}): TokenUsage | null {
  const requested = new Map<string, string>();
  // goose's per-call usage by generation id: the same calls, as the provider streamed their usage to goose.
  const streamed = new Map<string, { prompt: number; completion: number }>();
  for (const call of input.calls) {
    if (call.responseId && call.model && !requested.has(call.responseId))
      requested.set(call.responseId, call.model);
    if (call.responseId && call.promptTokens !== null && call.completionTokens !== null)
      streamed.set(call.responseId, {
        prompt: call.promptTokens,
        completion: call.completionTokens,
      });
  }
  const buckets = new Map<string, Bucket>();
  let missing = 0;
  let unreadable = 0;
  const matched = {
    calls: 0,
    recordPrompt: 0,
    recordCompletion: 0,
    streamPrompt: 0,
    streamCompletion: 0,
  };
  for (const id of input.ids) {
    const record = input.records[id];
    const recordModel =
      typeof record?.model === 'string' && record.model ? record.model.slice(0, 120) : null;
    const b = bucketFor(buckets, requested.get(id) ?? recordModel ?? UNKNOWN_MODEL);
    b.calls++;
    if (
      !record ||
      (record.id !== undefined && record.id !== id) ||
      !count(record.native_tokens_prompt) ||
      !count(record.native_tokens_completion)
    ) {
      if (record) unreadable++;
      missing++;
      continue;
    }
    b.counted++;
    b.prompt += record.native_tokens_prompt;
    b.completion += record.native_tokens_completion;
    const stream = streamed.get(id);
    if (stream) {
      matched.calls++;
      matched.recordPrompt += record.native_tokens_prompt;
      matched.recordCompletion += record.native_tokens_completion;
      matched.streamPrompt += stream.prompt;
      matched.streamCompletion += stream.completion;
    }
    if (
      count(record.native_tokens_cached) &&
      record.native_tokens_cached <= record.native_tokens_prompt
    ) {
      b.cached += record.native_tokens_cached;
      b.cachedKnown++;
    }
    if (count(record.native_tokens_reasoning)) {
      b.reasoning += record.native_tokens_reasoning;
      b.reasoningKnown++;
    }
  }
  // A call goose made that carries no generation id has no record to read: it is a call of the run all the same.
  if (input.telemetryAuthoritative === true)
    for (const call of input.calls)
      if (!call.responseId || !GENERATION_ID.test(call.responseId)) {
        bucketFor(buckets, call.model ?? UNKNOWN_MODEL).calls++;
        missing++;
      }
  const uncollected =
    count(input.collected, MAX_CALLS) && input.collected > input.ids.length
      ? input.collected - input.ids.length
      : 0;
  if (uncollected) {
    bucketFor(buckets, UNKNOWN_MODEL).calls += uncollected;
    missing += uncollected;
  }
  const notes: string[] = [];
  if (missing)
    notes.push(
      `${missing} call${missing === 1 ? '' : 's'} of the run ha${missing === 1 ? 's' : 've'} no readable OpenRouter record${unreadable ? ` (${unreadable} malformed)` : ''}; ${missing === 1 ? 'its' : 'their'} tokens are not in the totals.`
    );
  // The run's own evidence must agree: goose's per-call usage for the SAME generation ids, and goose's session counters
  // (entrant only, so never above every call's records), must not exceed the records. When they do, OpenRouter's
  // figures are still the ones sent (the provider's own record, which the owner compares against) but as a LOWER
  // BOUND, and the note names goose's larger count. Measured: Jev Router 6,276,908 / 36,831 in the records against
  // 6,853,350 / 61,901 streamed for the same 117 ids.
  const gapPrompt = matched.streamPrompt - matched.recordPrompt;
  const gapCompletion = matched.streamCompletion - matched.recordCompletion;
  let lowerBound = false;
  if (gapPrompt > 0 || gapCompletion > 0) {
    lowerBound = true;
    notes.push(
      `goose's per-call usage for the same ${matched.calls} calls is ${matched.streamPrompt.toLocaleString('en-US')} in / ${matched.streamCompletion.toLocaleString('en-US')} out, more than OpenRouter's records (${gapPrompt > 0 ? `+${gapPrompt.toLocaleString('en-US')} in` : ''}${gapPrompt > 0 && gapCompletion > 0 ? ', ' : ''}${gapCompletion > 0 ? `+${gapCompletion.toLocaleString('en-US')} out` : ''}): the records are a lower bound.`
    );
  } else {
    const counters = sessionCounters(input.usage);
    const recordPrompt = [...buckets.values()].reduce((a, b) => a + b.prompt, 0);
    const recordCompletion = [...buckets.values()].reduce((a, b) => a + b.completion, 0);
    if (counters && (counters.prompt > recordPrompt || counters.completion > recordCompletion)) {
      lowerBound = true;
      notes.push(
        `goose's session counters (${counters.prompt.toLocaleString('en-US')} in / ${counters.completion.toLocaleString('en-US')} out) exceed OpenRouter's records: the records are a lower bound.`
      );
    }
  }
  return bucketsToUsage(buckets, 'provider-records', missing, notes, lowerBound);
}

/** Integer sums of goose's isolated session counters (`agent.usage.sessions`), or null when any is missing. */
export function sessionCounters(
  usage: unknown
): { prompt: number; completion: number; cached: number | null } | null {
  const sessions = (usage as { sessions?: unknown } | null | undefined)?.sessions;
  if (!Array.isArray(sessions) || !sessions.length) return null;
  let prompt = 0;
  let completion = 0;
  let cached: number | null = 0;
  for (const s of sessions as Array<Record<string, unknown>>) {
    if (!count(s?.accumulated_input_tokens) || !count(s?.accumulated_output_tokens)) return null;
    prompt += s.accumulated_input_tokens as number;
    completion += s.accumulated_output_tokens as number;
    const read = s.accumulated_cache_read_tokens;
    cached = cached !== null && count(read) ? cached + read : null;
  }
  return { prompt, completion, cached: cached !== null && cached <= prompt ? cached : null };
}

/**
 * Token usage from the per-call usage goose recorded (`.swarm/telemetry.jsonl`): the engine's own counts for a run on
 * this Mac ("engine-telemetry"), the provider's API usage for a cloud provider without per-call records
 * ("session-counters"). The telemetry carries no cached or reasoning count; a model whose prompt and completion sums
 * equal goose's session counters exactly takes its cached count from them.
 */
export function tokenUsageFromCalls(input: {
  calls: TelemetryCall[];
  usage: unknown;
  source: 'engine-telemetry' | 'session-counters';
}): TokenUsage | null {
  const buckets = new Map<string, Bucket>();
  let missing = 0;
  for (const call of input.calls) {
    const b = bucketFor(buckets, call.model ?? UNKNOWN_MODEL);
    b.calls++;
    if (call.promptTokens === null || call.completionTokens === null) {
      missing++;
      continue;
    }
    b.counted++;
    b.prompt += call.promptTokens;
    b.completion += call.completionTokens;
  }
  const counters = sessionCounters(input.usage);
  if (counters && counters.cached !== null) {
    const owner = [...buckets.values()].filter(
      (b) => b.counted > 0 && b.prompt === counters.prompt && b.completion === counters.completion
    );
    if (owner.length === 1) {
      owner[0].cached = counters.cached;
      owner[0].cachedKnown = owner[0].counted;
    }
  }
  const notes: string[] = [];
  if (missing)
    notes.push(
      `${missing} call${missing === 1 ? '' : 's'} carried no usage (a dropped stream); ${missing === 1 ? 'its' : 'their'} tokens are not in the totals.`
    );
  return bucketsToUsage(buckets, input.source, missing, notes);
}

/**
 * The harness bill's own totals (`agent.billed_cost.tokens`, summed from the same OpenRouter records at run end), when
 * the records cannot be read again. Split by model only where the bill proves it: one model, or the bill's
 * reconciliation of goose's entrant counters against the helper calls it billed outside them.
 */
export function tokenUsageFromHarnessBill(input: {
  calls: TelemetryCall[];
  billedCost: unknown;
  reason: string;
}): TokenUsage | null {
  const bill =
    input.billedCost && typeof input.billedCost === 'object'
      ? (input.billedCost as Record<string, unknown>)
      : null;
  if (!bill || bill.provider !== 'openrouter') return null;
  if (bill.status !== 'complete' && bill.status !== 'incomplete') return null;
  const tokens = bill.tokens as Record<string, unknown> | undefined;
  if (!tokens || !count(tokens.prompt) || !count(tokens.completion)) return null;
  const t = tokens as Record<string, number>;
  const missing = Array.isArray(bill.missing) ? bill.missing.length : 0;
  // Telemetry lines name separate calls only when the bill says the telemetry is the run's per-call record; an older
  // engine's lines carry no ids for calls the bill found by id (counting them again would double every call).
  const authoritative = (bill.telemetry as { status?: unknown } | undefined)?.status === 'recorded';
  const withoutId = authoritative
    ? input.calls.filter((c) => !c.responseId || !GENERATION_ID.test(c.responseId)).length
    : 0;
  const callsByModel = new Map<string, number>();
  if (authoritative) {
    const ids = runGenerationIds(input.calls, bill);
    const requested = new Map(
      input.calls
        .filter((c) => c.responseId && c.model)
        .map((c) => [c.responseId as string, c.model as string])
    );
    for (const id of ids) {
      const model = requested.get(id);
      if (!model) return null; // a call the split cannot name
      callsByModel.set(model, (callsByModel.get(model) ?? 0) + 1);
    }
    for (const c of input.calls)
      if (!c.responseId || !GENERATION_ID.test(c.responseId)) {
        if (!c.model) return null;
        callsByModel.set(c.model, (callsByModel.get(c.model) ?? 0) + 1);
      }
  } else {
    // No per-call record to split by: only a bill that names one model and no helper calls is attributable.
    const helpers = Object.keys(
      ((bill.budget as Record<string, unknown> | undefined)?.calls_other_models ?? {}) as object
    );
    const collected = bill.generation_ids_collected;
    if (
      helpers.length ||
      typeof bill.model !== 'string' ||
      !bill.model ||
      !count(collected, MAX_CALLS)
    )
      return null;
    callsByModel.set(bill.model.slice(0, 120), collected);
  }
  const cached = count(t.cached) ? t.cached : null;
  const reasoning = count(t.reasoning) ? t.reasoning : null;
  const models = [...callsByModel.keys()];
  let byModel: TokenUsageModel[];
  if (models.length === 1)
    byModel = [
      {
        model: models[0],
        promptTokens: t.prompt,
        completionTokens: t.completion,
        cachedTokens: cached,
        reasoningTokens: reasoning,
        calls: callsByModel.get(models[0])!,
      },
    ];
  else {
    const rec = bill.reconciliation as Record<string, unknown> | undefined;
    const entrantTokens = rec?.goose_tokens as Record<string, unknown> | undefined;
    const helperTokens = rec?.billed_tokens_outside_session_counters as
      | Record<string, unknown>
      | undefined;
    const helpers = Object.keys(
      ((bill.budget as Record<string, unknown> | undefined)?.calls_other_models ?? {}) as object
    );
    const entrant = models.filter((m) => !helpers.includes(m));
    if (
      rec?.status !== 'billed_calls_outside_session_counters' ||
      models.length !== 2 ||
      helpers.length !== 1 ||
      !models.includes(helpers[0]) ||
      entrant.length !== 1 ||
      !entrantTokens ||
      !helperTokens ||
      !['prompt', 'completion', 'cached'].every(
        (k) => count(entrantTokens[k]) && count(helperTokens[k])
      )
    )
      return null;
    const e = entrantTokens as Record<string, number>;
    const h = helperTokens as Record<string, number>;
    if (e.prompt + h.prompt !== t.prompt || e.completion + h.completion !== t.completion)
      return null;
    // Reasoning is in the bill's total only; zero splits as zero, anything else is not guessed.
    const split = reasoning === 0 ? 0 : null;
    byModel = [
      {
        model: entrant[0],
        promptTokens: e.prompt,
        completionTokens: e.completion,
        cachedTokens: e.cached,
        reasoningTokens: split,
        calls: callsByModel.get(entrant[0])!,
      },
      {
        model: helpers[0],
        promptTokens: h.prompt,
        completionTokens: h.completion,
        cachedTokens: h.cached,
        reasoningTokens: split,
        calls: callsByModel.get(helpers[0])!,
      },
    ];
    if (cached === null || e.cached + h.cached !== cached) return null;
  }
  const missingCalls = missing + withoutId;
  const calls = byModel.reduce((a, m) => a + m.calls, 0);
  const promptTokens = t.prompt;
  const completionTokens = t.completion;
  const partialReasoning = byModel.length > 1 && reasoning !== null && reasoning > 0;
  const usage: TokenUsage = {
    promptTokens,
    completionTokens,
    cachedTokens: byModel.every((m) => m.cachedTokens === null)
      ? null
      : byModel.reduce((a, m) => a + (m.cachedTokens ?? 0), 0),
    reasoningTokens: byModel.every((m) => m.reasoningTokens === null)
      ? null
      : byModel.reduce((a, m) => a + (m.reasoningTokens ?? 0), 0),
    totalTokens: promptTokens + completionTokens,
    calls,
    source: 'provider-records',
    exact: bill.status === 'complete' && missingCalls === 0 && !partialReasoning,
    missingCalls,
    note: `Totals from the OpenRouter records the harness summed at run end; the records could not be read again at publish (${input.reason.slice(0, 160)}).${missingCalls ? ` ${missingCalls} call${missingCalls === 1 ? '' : 's'} had no record then.` : ''}`
      .slice(0, 400)
      .trim(),
    byModel,
  };
  return tokenUsageProblem(usage) ? null : usage;
}

/** The site's rule for a tokenUsage record (website benchmark-token-usage.ts); null = consistent. */
export function tokenUsageProblem(t: TokenUsage): string | null {
  const keys = [
    'promptTokens',
    'completionTokens',
    'cachedTokens',
    'reasoningTokens',
    'totalTokens',
    'calls',
    'source',
    'exact',
    'missingCalls',
    'note',
    'byModel',
  ];
  if (Object.keys(t).some((k) => !keys.includes(k))) return 'unknown field';
  if (!count(t.promptTokens) || !count(t.completionTokens) || !count(t.totalTokens))
    return 'token counts';
  if (t.totalTokens !== t.promptTokens + t.completionTokens)
    return 'totalTokens is not prompt + completion';
  if (!count(t.calls, MAX_CALLS) || !count(t.missingCalls, MAX_CALLS) || t.missingCalls > t.calls)
    return 'call counts';
  if (!TOKEN_USAGE_SOURCES.includes(t.source) || typeof t.exact !== 'boolean')
    return 'source or exact';
  if (t.exact && t.missingCalls > 0) return 'exact with missing calls';
  if (t.exact && t.source === 'estimate') return 'an estimate is never exact';
  if (
    t.note !== null &&
    (typeof t.note !== 'string' ||
      !t.note.trim() ||
      t.note !== t.note.trim() ||
      t.note.length > 400)
  )
    return 'note';
  for (const v of [t.cachedTokens, t.reasoningTokens])
    if (v !== null && !count(v)) return 'cached or reasoning count';
  if (t.cachedTokens !== null && t.cachedTokens > t.promptTokens) return 'cached exceeds prompt';
  if (!Array.isArray(t.byModel) || !t.byModel.length || t.byModel.length > MAX_MODELS)
    return 'byModel';
  const names = new Set<string>();
  for (const m of t.byModel) {
    if (typeof m.model !== 'string' || !m.model || m.model.length > 120 || names.has(m.model))
      return 'byModel model';
    names.add(m.model);
    if (!count(m.promptTokens) || !count(m.completionTokens) || !count(m.calls, MAX_CALLS))
      return 'byModel counts';
    for (const v of [m.cachedTokens, m.reasoningTokens])
      if (v !== null && !count(v)) return 'byModel cached or reasoning';
    if (m.cachedTokens !== null && m.cachedTokens > m.promptTokens)
      return 'byModel cached exceeds prompt';
  }
  const sum = (pick: (m: TokenUsageModel) => number) => t.byModel.reduce((a, m) => a + pick(m), 0);
  if (
    sum((m) => m.promptTokens) !== t.promptTokens ||
    sum((m) => m.completionTokens) !== t.completionTokens ||
    sum((m) => m.calls) !== t.calls
  )
    return 'byModel sums differ from the totals';
  for (const name of ['cachedTokens', 'reasoningTokens'] as const) {
    const values = t.byModel.map((m) => m[name]);
    const known = values.filter((v): v is number => v !== null);
    const totalValue = t[name];
    if (!known.length ? totalValue !== null : totalValue !== known.reduce((a, b) => a + b, 0))
      return 'byModel cached or reasoning sums differ from the totals';
    if (known.length && known.length < values.length && t.exact)
      return 'a partly known figure is never exact';
  }
  return null;
}

export interface GenerationReadResult {
  records: Record<string, GenerationRecord | null>;
  failures: Record<string, string>;
  /** The read budget ran out with records still unread. */
  timedOut: boolean;
}

// policy: one request may take 30 s (OpenRouter answers in well under a second); the whole read of a run's records
// gets 75 s, after which the publish falls back to the harness bill's totals instead of holding the publication lock.
const REQUEST_TIMEOUT_MS = 30_000;
const READ_DEADLINE_MS = 75_000;

/**
 * Read OpenRouter's generation record for each id (the harness's own read: same endpoint, same bounded concurrency,
 * a short re-ask for records that are not there yet). `fetchImpl` is the global fetch in the app; the key travels
 * only in the Authorization header to `host`, the host that billed the run.
 */
export async function readGenerationRecords(
  ids: string[],
  options: {
    key: string;
    host: string;
    fetchImpl?: typeof fetch;
    concurrency?: number;
    sleep?: (ms: number) => Promise<void>;
    retryDelaysMs?: number[];
    /** ONE budget for the whole read (every request, every pass): a hung OpenRouter must not hold the publish. */
    deadlineMs?: number;
  }
): Promise<GenerationReadResult> {
  const doFetch = options.fetchImpl ?? fetch;
  const budget = options.deadlineMs ?? READ_DEADLINE_MS;
  const deadline = AbortSignal.timeout(budget);
  const started = Date.now();
  const sleep = options.sleep ?? ((ms: number) => new Promise((r) => setTimeout(r, ms)));
  const base = `${options.host.replace(/\/+$/, '')}/api/v1/generation?id=`;
  const records: Record<string, GenerationRecord | null> = Object.fromEntries(
    ids.map((id) => [id, null])
  );
  const failures: Record<string, string> = {};
  let pending = [...ids];
  for (const wait of [0, ...(options.retryDelaysMs ?? [2000, 8000])]) {
    if (!pending.length || deadline.aborted) break;
    if (wait) {
      if (Date.now() - started + wait >= budget) break;
      await sleep(wait);
    }
    const still: string[] = [];
    const queue = [...pending];
    const worker = async () => {
      for (let id = queue.shift(); id !== undefined; id = queue.shift()) {
        if (deadline.aborted) {
          failures[id] = 'read budget spent';
          still.push(id);
          continue;
        }
        try {
          const res = await doFetch(base + encodeURIComponent(id), {
            headers: { Authorization: `Bearer ${options.key}` },
            signal: AbortSignal.any([AbortSignal.timeout(REQUEST_TIMEOUT_MS), deadline]),
          });
          if (!res.ok) {
            failures[id] = `HTTP ${res.status}`;
            still.push(id);
            continue;
          }
          const body = (await res.json()) as { data?: unknown };
          const data = body?.data;
          if (!data || typeof data !== 'object' || Array.isArray(data)) {
            failures[id] = 'no generation record';
            still.push(id);
            continue;
          }
          records[id] = data as GenerationRecord;
          delete failures[id];
        } catch (error) {
          failures[id] = error instanceof Error ? error.name : 'request failed';
          still.push(id);
        }
      }
    };
    await Promise.all(
      Array.from({ length: Math.max(1, Math.min(options.concurrency ?? 4, queue.length)) }, worker)
    );
    pending = still;
  }
  return { records, failures, timedOut: deadline.aborted && pending.length > 0 };
}

/** The https host whose generation endpoint billed the run (`billed_cost.source`), or null. */
export function billingHost(billedCost: unknown): string | null {
  const source = (billedCost as { source?: unknown } | null | undefined)?.source;
  if (typeof source !== 'string') return null;
  const m = source.match(/^(https:\/\/[^\s/]+)\/api\/v1\/generation \(usage per generation id\)$/);
  return m ? m[1] : null;
}

/** "12.3M in · 206k out · 10.6M cached (exact, OpenRouter records)" — the app's line, the site's wording. */
export function formatTokenCount(n: number): string {
  if (n < 1_000) return String(n);
  if (n < 1_000_000) return `${n < 100_000 ? (n / 1_000).toFixed(1) : Math.round(n / 1_000)}k`;
  if (n < 1_000_000_000) return `${(n / 1_000_000).toFixed(1)}M`;
  return `${(n / 1_000_000_000).toFixed(1)}B`;
}

export function tokenUsageLine(t: TokenUsage, provider?: string | null): string {
  const label =
    t.source === 'provider-records'
      ? provider === 'openrouter'
        ? 'OpenRouter records'
        : 'provider records'
      : t.source === 'engine-telemetry'
        ? 'engine telemetry'
        : t.source === 'session-counters'
          ? 'goose session counters'
          : 'estimate';
  const cached =
    t.cachedTokens !== null && t.cachedTokens > 0
      ? ` · ${formatTokenCount(t.cachedTokens)} cached`
      : '';
  const figures = `${formatTokenCount(t.promptTokens)} in · ${formatTokenCount(t.completionTokens)} out${cached}`;
  // "estimated" only for an estimate; a record with calls or tokens it could not count is a LOWER BOUND.
  if (t.source === 'estimate')
    return `Tokens: about ${formatTokenCount(t.promptTokens)} in · ${formatTokenCount(t.completionTokens)} out (estimated)`;
  if (t.exact) return `Tokens: ${figures} (exact, ${label})`;
  return `Tokens: at least ${figures} (lower bound, ${label})`;
}

/**
 * The bill OpenRouter's records make when the publish-time read is EXACT (every generation id the run kept has its
 * record and the run's own usage agrees): the sum of each record's billed `usage`. Null otherwise — a partial read
 * never replaces the harness's bill.
 */
export function recordsBill(
  usage: TokenUsage | null,
  ids: string[],
  records: Record<string, GenerationRecord | null | undefined>
): { billedUsd: number; requestCount: number } | null {
  if (!usage?.exact || usage.source !== 'provider-records' || !ids.length) return null;
  let micro = 0;
  for (const id of ids) {
    const billed = records[id]?.usage;
    if (typeof billed !== 'number' || !Number.isFinite(billed) || billed < 0) return null;
    micro += Math.round(billed * 1e9);
  }
  return { billedUsd: micro / 1e9, requestCount: ids.length };
}
