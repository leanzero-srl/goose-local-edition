import path from 'node:path';
import {
  billMissedOnlyRecords,
  billingHost,
  recordsBill,
  parseTelemetryCalls,
  readGenerationRecords,
  runGenerationIds,
  tokenUsageFromCalls,
  tokenUsageFromHarnessBill,
  tokenUsageFromProviderRecords,
  tokenUsageProblem,
  type TelemetryCall,
  type TokenUsage,
} from './benchTokenUsage';

export interface TokenCaptureDeps {
  readFile: (file: string) => Promise<string>;
  /** The OpenRouter key and host goose is configured with (main: `goose benchmark-config`), or null. */
  openRouterCredentials: () => Promise<{ key: string; host: string } | null>;
  fetchImpl?: typeof fetch;
  /** Exact provider-record results keyed by the verdict's sha256, so a republish does not read every record again. */
  cache?: {
    get: (verdictSha256: string) => Promise<CachedCapture | null>;
    set: (verdictSha256: string, capture: CachedCapture) => Promise<void>;
  };
  sleep?: (ms: number) => Promise<void>;
  /** The whole-read budget for OpenRouter's records (benchTokenUsage READ_DEADLINE_MS when absent). */
  readDeadlineMs?: number;
}

export interface TokenCapture {
  usage: TokenUsage | null;
  /** Why no token usage is sent, in plain words for the publish card. */
  problem: string | null;
  /** An EXACT publish-time read is also a complete bill (Σ each record's billed USD): main sends it in place of a
   *  run-end bill that missed records, so the page never says "8 calls not found" beside "exact, 51 calls". Only
   *  when the run-end bill was incomplete ONLY because records were missing (billMissedOnlyRecords). */
  recordsBill?: { billedUsd: number; requestCount: number; missingAtRunEnd: number };
}

export interface CachedCapture {
  usage: TokenUsage;
  recordsBill?: { billedUsd: number; requestCount: number; missingAtRunEnd: number };
}

/** The run's per-call telemetry: the file the harness bill names (inside a `.swarm` folder only), else the run folders. */
async function readRunTelemetry(
  stored: Record<string, unknown>,
  billedCost: unknown,
  readFile: (file: string) => Promise<string>
): Promise<TelemetryCall[] | null> {
  const named = (billedCost as { telemetry?: { path?: unknown } } | null | undefined)?.telemetry
    ?.path;
  // A run folder another run can reuse (a fleet's swarm-<N>node-r0 slot) may hold the NEXT run's telemetry: a folder
  // is read only when it is this run's own (the re-pointed workdir, or a folder named after this run's id).
  const runId = typeof stored.runId === 'string' && stored.runId ? stored.runId : null;
  const ownFolder = (dir: string) => runId !== null && path.basename(dir).includes(runId);
  const workdirs = (
    typeof stored.workdir === 'string' && stored.workdir ? [stored.workdir] : []
  ).flatMap((d) => [d, path.dirname(d)]);
  const candidate =
    typeof stored.candidateWorkdir === 'string' && ownFolder(stored.candidateWorkdir)
      ? [stored.candidateWorkdir]
      : [];
  const candidates = [
    ...workdirs.map((d) => path.join(d, '.swarm', 'telemetry.jsonl')),
    ...candidate.map((d) => path.join(d, '.swarm', 'telemetry.jsonl')),
    ...(typeof named === 'string' &&
    path.isAbsolute(named) &&
    path.basename(named) === 'telemetry.jsonl' &&
    path.basename(path.dirname(named)) === '.swarm' &&
    ownFolder(path.dirname(path.dirname(named)))
      ? [named]
      : []),
  ];
  for (const file of [...new Set(candidates)]) {
    try {
      const { calls } = parseTelemetryCalls(await readFile(file));
      if (calls.length) return calls;
    } catch {
      // not here; next candidate
    }
  }
  return null;
}

/**
 * The run's token usage for its publish (`tokenUsage`). An OpenRouter run reads OpenRouter's own record of every
 * generation id it kept; when that is impossible (no key configured, offline) it falls back to the totals the harness
 * read from the same records at run end, then to the per-call usage goose recorded. A run on this Mac's engine uses
 * the engine's own per-call usage; any other cloud provider the API usage goose recorded per call.
 */
export async function captureTokenUsage(
  input: {
    stored: Record<string, unknown>;
    verdict: Record<string, unknown> | null;
    verdictSha256: string | null;
    route: 'cloud' | 'local' | 'unknown';
  },
  deps: TokenCaptureDeps
): Promise<TokenCapture> {
  const agent = (input.verdict?.agent ?? {}) as { billed_cost?: unknown; usage?: unknown };
  const bill = agent.billed_cost;
  const calls = await readRunTelemetry(input.stored, bill, deps.readFile);
  const openRouter =
    !!bill &&
    typeof bill === 'object' &&
    (bill as { provider?: unknown }).provider === 'openrouter';
  if (openRouter) {
    if (input.verdictSha256 && deps.cache) {
      const cached = await deps.cache.get(input.verdictSha256).catch(() => null);
      if (cached?.usage && !tokenUsageProblem(cached.usage))
        return {
          usage: cached.usage,
          problem: null,
          ...(cached.recordsBill ? { recordsBill: cached.recordsBill } : {}),
        };
    }
    const ids = runGenerationIds(calls ?? [], bill);
    let reason = 'the run kept no OpenRouter generation id';
    if (ids.length) {
      const host = billingHost(bill);
      const credentials = host ? await deps.openRouterCredentials().catch(() => null) : null;
      if (!host) reason = 'the bill names no https OpenRouter host';
      else if (!credentials) reason = 'no OpenRouter key is configured in goose';
      else if (credentials.host.replace(/\/+$/, '') !== host)
        reason = `goose's OpenRouter host is not the one that billed this run (${host})`;
      else {
        const read = await readGenerationRecords(ids, {
          key: credentials.key,
          host,
          fetchImpl: deps.fetchImpl,
          sleep: deps.sleep,
          deadlineMs: deps.readDeadlineMs,
        });
        const billRecord = bill as {
          generation_ids_collected?: unknown;
          telemetry?: { status?: unknown };
        };
        const usage = tokenUsageFromProviderRecords({
          calls: calls ?? [],
          ids,
          records: read.records,
          collected:
            typeof billRecord.generation_ids_collected === 'number'
              ? billRecord.generation_ids_collected
              : undefined,
          telemetryAuthoritative: billRecord.telemetry?.status === 'recorded' && !!calls?.length,
          usage: agent.usage,
          entrantModel:
            typeof (bill as { model?: unknown }).model === 'string'
              ? ((bill as { model: string }).model as string)
              : undefined,
        });
        const found = Object.values(read.records).filter(Boolean).length;
        if (usage && found > 0) {
          // A partial read (some records failed now) never beats the harness's own read of the same records.
          if (usage.missingCalls > 0) {
            const fromBill = tokenUsageFromHarnessBill({
              calls: calls ?? [],
              billedCost: bill,
              reason: read.timedOut
                ? `OpenRouter did not answer every record within the read budget; ${usage.missingCalls} unread`
                : `${usage.missingCalls} record(s) could not be read`,
            });
            if (fromBill && fromBill.missingCalls < usage.missingCalls)
              return { usage: fromBill, problem: null };
          }
          const missedAtRunEnd = billMissedOnlyRecords(bill);
          const sum = missedAtRunEnd ? recordsBill(usage, ids, read.records) : null;
          const exactBill = sum ? { ...sum, missingAtRunEnd: missedAtRunEnd } : null;
          if (usage.exact && input.verdictSha256 && deps.cache)
            await deps.cache
              .set(input.verdictSha256, { usage, ...(exactBill ? { recordsBill: exactBill } : {}) })
              .catch(() => {});
          return { usage, problem: null, ...(exactBill ? { recordsBill: exactBill } : {}) };
        }
        const failures = [...new Set(Object.values(read.failures))].slice(0, 3).join(', ');
        reason = read.timedOut
          ? 'OpenRouter did not answer within the read budget'
          : `OpenRouter answered no record (${failures || 'no usable record'})`;
      }
    }
    const fromBill = tokenUsageFromHarnessBill({ calls: calls ?? [], billedCost: bill, reason });
    if (fromBill) return { usage: fromBill, problem: null };
    if (calls) {
      const fromCalls = tokenUsageFromCalls({
        calls,
        usage: agent.usage,
        source: 'session-counters',
      });
      if (fromCalls) {
        const note = `OpenRouter's records could not be read (${reason}); counted from the API usage goose recorded per call.`;
        const withNote = {
          ...fromCalls,
          exact: false,
          note: `${note} ${fromCalls.note ?? ''}`.slice(0, 400).trim(),
        };
        if (!tokenUsageProblem(withNote)) return { usage: withNote, problem: null };
      }
    }
    return { usage: null, problem: `token counts could not be recorded: ${reason}` };
  }
  if (!calls)
    return {
      usage: null,
      problem: 'this run kept no per-call usage record (.swarm/telemetry.jsonl)',
    };
  const usage = tokenUsageFromCalls({
    calls,
    usage: agent.usage,
    source: input.route === 'local' ? 'engine-telemetry' : 'session-counters',
  });
  return usage
    ? { usage, problem: null }
    : {
        usage: null,
        problem: "this run's per-call usage record does not add up, so no token counts are sent",
      };
}
