/**
 * The run's REAL bill, as the harness recorded it (`agent.billed_cost`, mirrored to the run dir's
 * model-cost.json): OpenRouter's own generation records for every request the entrant made. It is
 * the only cost the Benchmark view may call a bill. goose's session `accumulated_cost` is a local
 * estimate from a price table and is never shown in its place.
 */
export type BilledCostStatus = 'complete' | 'incomplete' | 'unavailable';

export interface BilledCost {
  status: BilledCostStatus;
  billed_usd?: number;
  tokens?: { prompt?: number; cached?: number; completion?: number; reasoning?: number };
  hosts?: Record<string, number>;
  requests?: number;
  missing?: unknown[];
  reason?: string;
}

/**
 * The result-row field persisted from the verdict's `agent` block: the record verbatim when the
 * harness wrote one (even `unavailable`, even malformed — the view reports it), nothing otherwise.
 */
export function billedCostRowField(agent: unknown): { billedCost?: unknown } {
  return agent && typeof agent === 'object' && 'billed_cost' in agent
    ? { billedCost: (agent as { billed_cost: unknown }).billed_cost }
    : {};
}

/** The harness reads the bill from OpenRouter's generation records, whatever the entrant's route. */
const BILLING_SOURCE = 'OpenRouter';

const finite = (value: unknown): value is number =>
  typeof value === 'number' && Number.isFinite(value);

export type BilledCostView =
  | { kind: 'absent' }
  | { kind: 'malformed' }
  | { kind: 'record'; cost: BilledCost };

/** A record that names a status but breaks its own shape is reported, never read as a smaller bill. */
export function readBilledCost(value: unknown): BilledCostView {
  if (value === undefined || value === null) return { kind: 'absent' };
  if (typeof value !== 'object') return { kind: 'malformed' };
  const cost = value as BilledCost;
  if (cost.status === 'unavailable') return { kind: 'record', cost };
  if (cost.status !== 'complete' && cost.status !== 'incomplete') return { kind: 'malformed' };
  if (!finite(cost.billed_usd) || cost.billed_usd < 0) return { kind: 'malformed' };
  if (cost.status === 'incomplete' && !Array.isArray(cost.missing)) return { kind: 'malformed' };
  return { kind: 'record', cost };
}

const usd = (value: number) => `$${value.toFixed(4)}`;
const count = (value: number) => Math.round(value).toLocaleString('en-US');

export interface BilledCostLine {
  /** The figure for a stat cell — null when there is no billed amount to state. */
  amount: string | null;
  sentence: string;
  /** null: a complete bill is a plain fact, not a status. */
  tone: 'warn' | 'err' | 'stopped' | null;
  detail: string | null;
}

function detailOf(cost: BilledCost): string | null {
  const parts: string[] = [];
  const t = cost.tokens;
  if (t) {
    if (finite(t.prompt))
      parts.push(
        `prompt ${count(t.prompt)}${finite(t.cached) ? ` (cached ${count(t.cached)})` : ''}`
      );
    if (finite(t.completion)) parts.push(`completion ${count(t.completion)}`);
    if (finite(t.reasoning)) parts.push(`reasoning ${count(t.reasoning)}`);
  }
  const hosts = Object.entries(cost.hosts ?? {}).filter(([, n]) => finite(n));
  if (hosts.length)
    parts.push(
      `served by ${hosts
        .sort((a, b) => b[1] - a[1])
        .map(([name, n]) => `${name} ×${count(n)}`)
        .join(', ')}`
    );
  return parts.length ? parts.join(' · ') : null;
}

/**
 * What the view says about the bill. `provider` is the entrant's provider id; `null` is a swarm,
 * whose absent record is not a missing bill (local nodes bill nothing), so it says nothing.
 */
export function billedCostLine(value: unknown, provider: string | null): BilledCostLine | null {
  const view = readBilledCost(value);
  if (view.kind === 'absent')
    return provider
      ? {
          amount: null,
          sentence: 'No billing record was saved with this result.',
          tone: 'stopped',
          detail: null,
        }
      : null;
  if (view.kind === 'malformed')
    return {
      amount: null,
      sentence: 'The billing record saved with this result is unreadable.',
      tone: 'err',
      detail: null,
    };
  const { cost } = view;
  if (cost.status === 'unavailable')
    return {
      amount: null,
      sentence: `Billing not available for ${provider ?? 'this run'}.`,
      tone: 'stopped',
      detail: typeof cost.reason === 'string' && cost.reason ? cost.reason : null,
    };
  const billed = cost.billed_usd as number;
  if (cost.status === 'incomplete') {
    const missing = (cost.missing as unknown[]).length;
    return {
      amount: `≥ ${usd(billed)}`,
      sentence: `At least ${usd(billed)} — ${missing} call${missing === 1 ? '' : 's'} not found`,
      tone: 'warn',
      detail: detailOf(cost),
    };
  }
  const requests = finite(cost.requests) ? `, ${count(cost.requests)} requests` : '';
  return {
    amount: usd(billed),
    sentence: `Billed ${usd(billed)} (${BILLING_SOURCE}${requests})`,
    tone: null,
    detail: detailOf(cost),
  };
}

/**
 * The `billing` record leanzero.net stores with a published run (website src/lib/benchmark-billing.ts,
 * validateBenchmarkBilling): the run page shows it as "Recorded inference cost $X" ("At least $X" when
 * incomplete) or, for a publisher-declared price, "Estimated $X". Before 2026-10-18 the app never sent it,
 * so every published run showed no price even when OpenRouter had billed every call.
 */
export type PublishBilling =
  | {
      status: 'complete' | 'incomplete' | 'unavailable';
      source: 'openrouter-generation-records';
      billedUsd?: number;
      requestCount?: number;
      missingCallCount: number;
      includesAuxiliaryCalls: boolean | null;
      reasons: string[];
      nativeVerdictSha256: string;
    }
  | {
      status: 'estimated';
      source: 'declared-prices';
      billedUsd: number;
      inputUsdPerMTok: number;
      outputUsdPerMTok: number;
      cachedInputUsdPerMTok?: number;
      promptTokens: number;
      cachedTokens: number;
      completionTokens: number;
      includesAuxiliaryCalls: null;
      reasons: string[];
      nativeVerdictSha256: string;
    };

const OPENROUTER_BILL_SOURCE = 'https://openrouter.ai/api/v1/generation (usage per generation id)';
const SHA256 = /^[a-f0-9]{64}$/;

/**
 * The harness's OpenRouter bill (`agent.billed_cost`) as the site's billing record — the SAME mapping the
 * operator's backfill applied to already-published runs (loop-state forge2_billing.mjs nativeBilling), so an
 * app-published run and a backfilled one read identically. `verdictSha256` is the sha256 of the run's own
 * verdict.json bytes. Null when the record is not an OpenRouter bill (Google direct, a local engine) or does
 * not keep its own shape — never a guessed bill.
 */
export function openRouterPublishBilling(
  billedCost: unknown,
  verdictSha256: string
): PublishBilling | null {
  if (!billedCost || typeof billedCost !== 'object' || !SHA256.test(verdictSha256)) return null;
  const b = billedCost as Record<string, unknown>;
  if (b.provider !== 'openrouter' || b.source !== OPENROUTER_BILL_SOURCE) return null;
  if (b.status !== 'complete' && b.status !== 'incomplete') return null;
  if (!Array.isArray(b.missing) || !Array.isArray(b.incomplete_reasons)) return null;
  const hasAmount = finite(b.billed_usd) && b.billed_usd >= 0;
  if (!hasAmount && b.billed_usd !== null && b.billed_usd !== undefined) return null;
  const requests = b.request_count;
  if (requests !== undefined && (!finite(requests) || !Number.isInteger(requests) || requests < 0))
    return null;
  const auxiliary = (b.budget as { calls_other_models?: unknown } | undefined)?.calls_other_models;
  const hosts = b.hosts && typeof b.hosts === 'object' ? (b.hosts as object) : {};
  const includesAuxiliaryCalls =
    auxiliary && typeof auxiliary === 'object' && Object.keys(auxiliary).length
      ? Object.prototype.hasOwnProperty.call(hosts, 'Google')
        ? true
        : null
      : null;
  const reasons = (b.incomplete_reasons as unknown[]).filter(
    (r): r is string => typeof r === 'string' && r.trim().length > 0
  );
  if (
    b.missing.length &&
    b.missing.every((x) => (x as { reason?: unknown } | null)?.reason === 'HTTP 404')
  )
    reasons.push(
      'OpenRouter returned HTTP 404 for every missing generation record. Missing charges are unknown, not zero.'
    );
  return {
    // A bill that names missing calls is a lower bound whatever its status says (the site refuses a
    // 'complete' record with missing calls, which would refuse the whole publish).
    status: hasAmount
      ? b.missing.length
        ? 'incomplete'
        : (b.status as 'complete' | 'incomplete')
      : 'unavailable',
    source: 'openrouter-generation-records',
    ...(hasAmount ? { billedUsd: b.billed_usd as number } : {}),
    ...(requests !== undefined ? { requestCount: requests as number } : {}),
    missingCallCount: b.missing.length,
    includesAuxiliaryCalls,
    reasons: reasons.map((r) => r.slice(0, 400)).slice(0, 20),
    nativeVerdictSha256: verdictSha256,
  };
}

/** Whether a run carries an OpenRouter bill — then the bill is the price and no price is declared. */
export function hasOpenRouterBill(billedCost: unknown): boolean {
  return openRouterPublishBilling(billedCost, 'a'.repeat(64)) !== null;
}

export interface RunTokens {
  promptTokens: number;
  cachedTokens: number;
  completionTokens: number;
}

/**
 * The run's own token counts from the harness's isolated goose session counters (`agent.usage.sessions`):
 * prompt = accumulated input (cached reads included), cached = accumulated cache reads, completion =
 * accumulated output. Null when any session did not record its counters, or the cache reads exceed the
 * input (counters that do not add up are not priced).
 */
export function runTokens(usage: unknown): RunTokens | null {
  const sessions = (usage as { sessions?: unknown } | null | undefined)?.sessions;
  if (!Array.isArray(sessions) || sessions.length === 0) return null;
  let prompt = 0;
  let cached = 0;
  let completion = 0;
  for (const s of sessions as Array<Record<string, unknown>>) {
    const input = s?.accumulated_input_tokens;
    const output = s?.accumulated_output_tokens;
    const read = s?.accumulated_cache_read_tokens ?? 0;
    for (const n of [input, output, read])
      if (!finite(n) || n < 0 || !Number.isInteger(n)) return null;
    prompt += input as number;
    completion += output as number;
    cached += read as number;
  }
  if (cached > prompt) return null;
  return { promptTokens: prompt, cachedTokens: cached, completionTokens: completion };
}

export interface DeclaredPrices {
  inputUsdPerMTok: number;
  outputUsdPerMTok: number;
  cachedInputUsdPerMTok?: number;
}

/** The highest per-1M-token price the site accepts (website MAX_USD_PER_MTOK). */
export const MAX_DECLARED_USD_PER_MTOK = 10_000;

/** One price field: blank is "not declared"; anything else must be a plain non-negative number. */
export function parseDeclaredPrice(text: string): number | undefined | 'invalid' {
  const t = text.trim();
  if (!t) return undefined;
  if (!/^\d+(\.\d+)?$|^\.\d+$/.test(t)) return 'invalid';
  const n = Number(t);
  return Number.isFinite(n) && n >= 0 && n <= MAX_DECLARED_USD_PER_MTOK ? n : 'invalid';
}

/** The fields as typed → the prices to send, nothing (all blank), or the reason they cannot be sent. */
export function declaredPricesFromFields(fields: {
  input: string;
  output: string;
  cached: string;
}): { prices: DeclaredPrices | null; problem: string | null } {
  const input = parseDeclaredPrice(fields.input);
  const output = parseDeclaredPrice(fields.output);
  const cached = parseDeclaredPrice(fields.cached);
  if (input === 'invalid' || output === 'invalid' || cached === 'invalid')
    return {
      prices: null,
      problem: `Prices are USD per 1M tokens: a number from 0 to ${MAX_DECLARED_USD_PER_MTOK.toLocaleString('en-US')}.`,
    };
  if (input === undefined && output === undefined && cached === undefined)
    return { prices: null, problem: null };
  if (input === undefined || output === undefined)
    return {
      prices: null,
      problem: 'Enter both the input and the output price, or leave all price fields empty.',
    };
  return {
    prices: {
      inputUsdPerMTok: input,
      outputUsdPerMTok: output,
      ...(cached !== undefined ? { cachedInputUsdPerMTok: cached } : {}),
    },
    problem: null,
  };
}

/** USD for the run's tokens at declared prices — the site recomputes exactly this and refuses any other
 *  figure: uncached prompt at the input price, cached prompt at the cached price (else the input price),
 *  completion at the output price. */
export function declaredPriceUsd(tokens: RunTokens, prices: DeclaredPrices): number {
  return (
    ((tokens.promptTokens - tokens.cachedTokens) * prices.inputUsdPerMTok +
      tokens.cachedTokens * (prices.cachedInputUsdPerMTok ?? prices.inputUsdPerMTok) +
      tokens.completionTokens * prices.outputUsdPerMTok) /
    1e6
  );
}

/** The publisher-declared estimate as the site's billing record (no provider bill exists). */
export function declaredPublishBilling(
  tokens: RunTokens,
  prices: DeclaredPrices,
  verdictSha256: string
): PublishBilling | null {
  if (!SHA256.test(verdictSha256)) return null;
  for (const p of [prices.inputUsdPerMTok, prices.outputUsdPerMTok, prices.cachedInputUsdPerMTok])
    if (p !== undefined && (!finite(p) || p < 0 || p > MAX_DECLARED_USD_PER_MTOK)) return null;
  return {
    status: 'estimated',
    source: 'declared-prices',
    billedUsd: declaredPriceUsd(tokens, prices),
    inputUsdPerMTok: prices.inputUsdPerMTok,
    outputUsdPerMTok: prices.outputUsdPerMTok,
    ...(prices.cachedInputUsdPerMTok !== undefined
      ? { cachedInputUsdPerMTok: prices.cachedInputUsdPerMTok }
      : {}),
    promptTokens: tokens.promptTokens,
    cachedTokens: tokens.cachedTokens,
    completionTokens: tokens.completionTokens,
    includesAuxiliaryCalls: null,
    reasons: [],
    nativeVerdictSha256: verdictSha256,
  };
}
