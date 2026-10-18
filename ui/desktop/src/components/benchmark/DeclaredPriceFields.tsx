import { useEffect, useState } from 'react';
import {
  declaredPricesFromFields,
  declaredPriceUsd,
  type DeclaredPrices,
  type RunTokens,
} from '../../benchBilledCost';
import { fetchCanonicalModelInfo } from '../../utils/canonical';
import { isLocalProviderName } from '../settings/models/leanzeroSelectorPolicy';
import { DISABLED, FOCUS, MOTION, RADIUS, SURFACE, TONE_TEXT, TYPE, WEIGHT, cx } from '../lz';

export type PriceFields = { input: string; output: string; cached: string };

const INPUT = cx(
  'h-8 w-full bg-lz-surface px-3 text-lz-body text-lz-ink placeholder:text-lz-ink-4 aria-[invalid=true]:border-lz-err',
  SURFACE.outline,
  RADIUS.control,
  FOCUS,
  MOTION,
  DISABLED
);

/**
 * The publisher's declared prices for a run with no provider bill (Mihai 2026-10-18: "anyone picking up the app
 * ... knows exactly how to publish and set the prices ... fields for the user to add input and output costs if
 * goose can't read the prices"). Prefilled from goose's own price list for a CLOUD model (never for a model
 * that ran on this Mac, whatever provider name served it); the site shows the result as an estimate.
 */
export function useDeclaredPrices({
  resetKey,
  billed,
  provider,
  model,
  local,
  tokens,
}: {
  resetKey: string | null;
  billed: boolean;
  provider: string | null;
  model: string | null;
  local: boolean;
  tokens: RunTokens | null;
}): {
  fields: PriceFields;
  setFields: (update: (now: PriceFields) => PriceFields) => void;
  prefilled: boolean;
  prices: DeclaredPrices | null;
  problem: string | null;
} {
  const [fields, setFields] = useState<PriceFields>({ input: '', output: '', cached: '' });
  const [prefilled, setPrefilled] = useState(false);
  useEffect(() => {
    setFields({ input: '', output: '', cached: '' });
    setPrefilled(false);
    if (billed || local || !provider || !model || isLocalProviderName(provider)) return;
    let alive = true;
    void fetchCanonicalModelInfo(provider, model).then((info) => {
      const input = info?.inputTokenCost;
      const output = info?.outputTokenCost;
      if (!alive || typeof input !== 'number' || typeof output !== 'number') return;
      if (info?.currency && !['$', 'USD', 'usd'].includes(info.currency)) return;
      const cached = info?.cacheReadTokenCost;
      setFields((now) =>
        now.input || now.output || now.cached
          ? now
          : {
              input: String(input),
              output: String(output),
              cached: typeof cached === 'number' ? String(cached) : '',
            }
      );
      setPrefilled(true);
    });
    return () => {
      alive = false;
    };
  }, [resetKey, billed, local, provider, model]);
  const declared = declaredPricesFromFields(fields, tokens);
  const problem = billed
    ? null
    : (declared.problem ??
      (declared.prices && !tokens
        ? 'This run recorded no complete token counts, so a price cannot be estimated — clear the price fields to publish without one.'
        : null));
  return { fields, setFields, prefilled, prices: billed ? null : declared.prices, problem };
}

export function DeclaredPriceFields({
  idBase,
  state,
  tokens,
  disabled,
  testIdPrefix = 'publish-price',
}: {
  idBase: string;
  state: ReturnType<typeof useDeclaredPrices>;
  tokens: RunTokens | null;
  disabled: boolean;
  testIdPrefix?: string;
}) {
  const hintId = `${idBase}-hint`;
  return (
    <fieldset
      data-testid={testIdPrefix}
      className="m-0 min-w-0 max-w-[560px] border-0 p-0"
      disabled={disabled}
    >
      <legend className={cx('mb-1.5 block', TYPE.meta)}>
        Price <span className="text-lz-ink-4">(optional — no provider bill for this run)</span>
      </legend>
      <div className="grid grid-cols-1 gap-3 sm:grid-cols-3">
        {(
          [
            ['input', 'Input price', 'e.g. 1.25'],
            ['output', 'Output price', 'e.g. 10'],
            ['cached', 'Cached input price', tokens && tokens.cachedTokens > 0 ? 'required' : 'same as input'],
          ] as const
        ).map(([key, label, hint]) => (
          <div key={key}>
            <label htmlFor={`${idBase}-${key}`} className={cx('mb-1 block', TYPE.meta)}>
              {label} <span className="text-lz-ink-4">(USD per 1M tokens)</span>
            </label>
            <input
              id={`${idBase}-${key}`}
              data-testid={`${testIdPrefix}-${key}`}
              type="text"
              inputMode="decimal"
              value={state.fields[key]}
              onChange={(e) =>
                state.setFields((now) => ({ ...now, [key]: e.target.value.slice(0, 12) }))
              }
              placeholder={hint}
              aria-invalid={state.problem != null}
              aria-describedby={hintId}
              className={INPUT}
            />
          </div>
        ))}
      </div>
      <p id={hintId} data-testid={`${testIdPrefix}-hint`} className={cx('mt-1.5', TYPE.meta)}>
        {!tokens
          ? 'This run recorded no complete token counts, so it publishes without a price.'
          : state.prices
            ? `Published as an estimate: $${declaredPriceUsd(tokens, state.prices).toFixed(2)} for ${tokens.promptTokens.toLocaleString('en-US')} prompt (${tokens.cachedTokens.toLocaleString('en-US')} cached) and ${tokens.completionTokens.toLocaleString('en-US')} completion tokens — not a provider bill. Cache writes are not priced.`
            : 'Enter what you pay for this model to show its cost as an estimate, or leave empty to publish without a price.'}
        {state.prefilled && " Prefilled from goose's price list — check before publishing."}
        {state.problem && (
          <span className={cx('ml-1', WEIGHT.medium, TONE_TEXT.err)}>{state.problem}</span>
        )}
      </p>
    </fieldset>
  );
}
