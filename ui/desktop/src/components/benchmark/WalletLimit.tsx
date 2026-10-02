import { useEffect, useRef, useState } from 'react';
import { acpReadConfig, acpRemoveConfig, acpUpsertConfig } from '../../acp/config';
import { BENCH_MAX_USD_KEY, parseMaxUsd } from '../../benchBudget';
import { TYPE } from '../lz';

/**
 * The operator's spend limit for a single-model run: once OpenRouter has billed this much, the harness
 * stops the model and scores what it built (bench_budget.py's wallet guard). Stored in goose's config
 * as BENCH_MAX_USD, which main reads at launch. Saved on every valid edit, so a Run click never races
 * an unsaved value; an invalid entry is shown and never saved.
 */
export function WalletLimit({ disabled }: { disabled: boolean }) {
  const [text, setText] = useState('');
  const [loaded, setLoaded] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const stored = useRef(false);

  useEffect(() => {
    let alive = true;
    acpReadConfig(BENCH_MAX_USD_KEY)
      .then((value) => {
        if (!alive) return;
        stored.current = value != null;
        if (value != null) setText(String(value));
        setLoaded(true);
      })
      .catch((err: unknown) => {
        if (alive)
          setError(
            `Your saved limit could not be read: ${err instanceof Error ? err.message : String(err)}`
          );
      });
    return () => {
      alive = false;
    };
  }, []);

  const save = (next: string) => {
    setText(next);
    const parsed = parseMaxUsd(next);
    if (parsed.kind === 'invalid') {
      setError('Enter a dollar amount above zero, like 5 or 2.50. This value is not saved.');
      return;
    }
    setError(null);
    const write =
      parsed.kind === 'ok'
        ? acpUpsertConfig(BENCH_MAX_USD_KEY, parsed.usd).then(() => {
            stored.current = true;
          })
        : stored.current
          ? acpRemoveConfig(BENCH_MAX_USD_KEY, false).then(() => {
              stored.current = false;
            })
          : Promise.resolve();
    write.catch((err: unknown) =>
      setError(`The limit could not be saved: ${err instanceof Error ? err.message : String(err)}`)
    );
  };

  const parsed = parseMaxUsd(text);
  return (
    <div className="flex flex-col gap-2 text-sm">
      <label className="flex flex-wrap items-center gap-2">
        <span>Stop a run at</span>
        <span className="flex items-center gap-1 rounded-lg border border-lz-border bg-lz-surface px-3 py-2 text-lz-ink">
          <span aria-hidden>$</span>
          <input
            aria-label="Stop a run at (US dollars, OpenRouter)"
            inputMode="decimal"
            placeholder="no limit"
            value={text}
            onChange={(event) => save(event.target.value)}
            disabled={disabled || !loaded}
            className="w-24 bg-transparent text-lz-ink outline-none"
          />
        </span>
        <span className={TYPE.meta}>(OpenRouter)</span>
      </label>
      {error ? (
        <p role="alert" className="text-lz-err">
          {error}
        </p>
      ) : (
        <span className={TYPE.meta}>
          {parsed.kind === 'ok'
            ? `Once OpenRouter has billed $${parsed.usd.toFixed(2)}, the run stops and what the model built is scored.`
            : 'Every single-model run has the same call budget. Set an amount to also stop a run once OpenRouter has billed it.'}
        </span>
      )}
    </div>
  );
}
