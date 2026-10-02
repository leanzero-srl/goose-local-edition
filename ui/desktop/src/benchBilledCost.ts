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
