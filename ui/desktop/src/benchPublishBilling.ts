import { createHash } from 'node:crypto';
import path from 'node:path';
import util from 'node:util';
import {
  cachedPriceProblem,
  declaredPublishBilling,
  hasOpenRouterBill,
  MAX_DECLARED_USD_PER_MTOK,
  openRouterPublishBilling,
  publishTokens,
  type DeclaredPrices,
  type PublishBilling,
} from './benchBilledCost';

/**
 * The run's price as leanzero.net stores it (`billing`; the run page's "Recorded inference cost"). Before
 * 2026-10-18 the app sent none, so every published run showed no price although OpenRouter had billed every
 * call. An OpenRouter bill in the run's own verdict.json is the price and is always sent; declared prices
 * (the publish card's per-1M-token fields) are used only when no such bill exists — Google direct, Bedrock,
 * a local engine — and are sent as an ESTIMATE the site recomputes from the run's own token counts.
 * Also returns the verdict it read, so the Forge 2.0 comparison receipt is built from the same bytes.
 */
export async function benchPublishBilling(
  stored: Record<string, unknown>,
  declared: unknown,
  readFile: (file: string) => Promise<Buffer>
): Promise<{ record?: PublishBilling; verdict?: Record<string, unknown>; error?: string }> {
  const prices = declared === undefined || declared === null ? null : readDeclaredPrices(declared);
  if (prices === 'invalid')
    return {
      error: `Declared prices must be USD per 1M tokens, from 0 to ${MAX_DECLARED_USD_PER_MTOK}, with both an input and an output price.`,
    };
  const billed = hasOpenRouterBill(stored.billedCost);
  if (typeof stored.workdir !== 'string') {
    if (billed)
      return {
        error:
          "This run's OpenRouter bill is recorded, but its run folder is not, so the bill cannot be attached. Re-score the run from its saved build to publish it with its price.",
      };
    if (prices)
      return {
        error:
          "This run's folder is not recorded, so a declared price cannot be attached. Clear the price fields to publish without one.",
      };
    return {};
  }
  // The verdict whose bill is sent: the run dir's verdict.json. A Gauntlet re-score stores its
  // verdict-evidence folder as the workdir, with its verdict one level up. A file whose bill is not the
  // stored row's bill belongs to another run and is never hashed.
  let bytes: Buffer | null = null;
  let verdict: Record<string, unknown> | null = null;
  for (const dir of [stored.workdir, path.dirname(stored.workdir)]) {
    try {
      const raw = await readFile(path.join(dir, 'verdict.json'));
      const v = JSON.parse(raw.toString('utf8')) as Record<string, unknown>;
      const agent = (v?.agent ?? {}) as { billed_cost?: unknown };
      if (stored.billedCost === undefined || util.isDeepStrictEqual(agent.billed_cost, stored.billedCost)) {
        bytes = raw;
        verdict = v;
        break;
      }
    } catch {
      // no verdict here; try the next folder
    }
  }
  if (!bytes || !verdict) {
    if (billed)
      return {
        error:
          "This run's OpenRouter bill is recorded, but its verdict.json is no longer in the run folder, so the bill cannot be attached. Re-score the run from its saved build to publish it with its price.",
      };
    return prices
      ? {
          error:
            "This run's verdict.json is no longer in its folder, so a price cannot be attached. Clear the price fields to publish without one.",
        }
      : {};
  }
  const sha = createHash('sha256').update(bytes).digest('hex');
  const agent = (verdict.agent ?? {}) as { billed_cost?: unknown; usage?: unknown };
  const bill = openRouterPublishBilling(agent.billed_cost, sha);
  if (bill) return { record: bill, verdict };
  if (!prices) return { verdict };
  const tokens = publishTokens(agent.usage, stored.telemetry);
  if (!tokens)
    return {
      verdict,
      error:
        'This run recorded no complete token counts, so declared prices cannot be turned into a cost. Clear the price fields to publish without one.',
    };
  const missingCached = cachedPriceProblem(tokens, prices);
  if (missingCached) return { verdict, error: missingCached };
  const record = declaredPublishBilling(tokens, prices, sha);
  return record ? { record, verdict } : { verdict, error: 'The declared prices could not be applied to this run.' };
}

export function readDeclaredPrices(value: unknown): DeclaredPrices | 'invalid' {
  if (!value || typeof value !== 'object') return 'invalid';
  const v = value as Record<string, unknown>;
  const ok = (n: unknown) =>
    typeof n === 'number' && Number.isFinite(n) && n >= 0 && n <= MAX_DECLARED_USD_PER_MTOK;
  if (!ok(v.inputUsdPerMTok) || !ok(v.outputUsdPerMTok)) return 'invalid';
  if (v.cachedInputUsdPerMTok !== undefined && !ok(v.cachedInputUsdPerMTok)) return 'invalid';
  return {
    inputUsdPerMTok: v.inputUsdPerMTok as number,
    outputUsdPerMTok: v.outputUsdPerMTok as number,
    ...(v.cachedInputUsdPerMTok !== undefined
      ? { cachedInputUsdPerMTok: v.cachedInputUsdPerMTok as number }
      : {}),
  };
}

/** Whether a site refusal is about the price, so the publish is retried WITHOUT it (the price must never cost
 *  the user the whole publish — e.g. a site that does not accept declared prices yet answers 400). */
export function refusalIsAboutBilling(status: number, message: string): boolean {
  return status >= 400 && status < 500 && /\bbilling\b/i.test(message);
}

/** Optional additions a publish can live without: a site that does not know them yet (older leanzero.net) or
 *  refuses them must never cost the whole publish. */
export const DROPPABLE_PUBLISH_FIELDS = ['billing', 'comparisonCohort'] as const;

/**
 * Which optional fields to drop and retry after a 4xx: those the site names as unknown ("Unknown field(s):
 * comparisonCohort"), the price when the refusal is about billing, the receipt when it is about the comparison
 * cohort. Empty = the refusal is about the run itself and stands.
 */
export function droppableAfterRefusal(status: number, message: string, present: string[]): string[] {
  if (status < 400 || status >= 500) return [];
  const drop = new Set<string>();
  const unknown = message.match(/Unknown field\(s\):\s*([^.]*)/i);
  if (unknown)
    for (const name of unknown[1].split(/[,\s]+/).filter(Boolean))
      if ((DROPPABLE_PUBLISH_FIELDS as readonly string[]).includes(name)) drop.add(name);
  if (refusalIsAboutBilling(status, message)) drop.add('billing');
  if (/^\s*comparisonCohort\b/i.test(message)) drop.add('comparisonCohort');
  return [...drop].filter((name) => present.includes(name));
}
