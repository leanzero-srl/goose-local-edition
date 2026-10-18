import { useEffect, useState } from 'react';
import { FOCUS, TYPE, cx } from '../lz';

/**
 * LeanZero's own installs only: the operator key leanzero.net checks (it stores only the key's sha256) to list a
 * single-model CLOUD run as a LeanZero cloud baseline, and to name LeanZero runs by their title, with no operator
 * script. WRITE-ONLY: main keeps the key in the OS keychain (Electron safeStorage), outside goose's config.yaml, and
 * this field never receives it back — only whether one is saved. Anyone else leaves it empty.
 */
export function OperatorKey({ disabled }: { disabled: boolean }) {
  const [text, setText] = useState('');
  const [saved, setSaved] = useState(false);
  const [available, setAvailable] = useState(true);
  const [loaded, setLoaded] = useState(false);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    let alive = true;
    void window.electron
      .benchmarkOperatorKeyStatus?.()
      .then((status) => {
        if (!alive || !status) return;
        setSaved(status.saved);
        setAvailable(status.available);
        setLoaded(true);
      })
      .catch((err: unknown) => {
        if (alive)
          setError(`The operator key setting could not be read: ${err instanceof Error ? err.message : String(err)}`);
      });
    return () => {
      alive = false;
    };
  }, []);

  const write = async (value: string) => {
    const result: { ok: boolean; saved?: boolean; error?: string } = await window.electron
      .benchmarkOperatorKeySet(value)
      .catch((err: unknown) => ({ ok: false, error: err instanceof Error ? err.message : String(err) }));
    if (!result.ok) setError(`The operator key could not be saved: ${result.error ?? 'unknown error'}`);
    else {
      setError(null);
      setSaved(Boolean(result.saved));
      setText('');
    }
  };

  return (
    <div className="flex flex-col gap-2 text-sm">
      <label className="flex flex-wrap items-center gap-2">
        <span>LeanZero operator key</span>
        <span className="flex items-center gap-1 rounded-lg border border-lz-border bg-lz-surface px-3 py-2 text-lz-ink">
          <input
            aria-label="LeanZero operator key"
            type="password"
            autoComplete="off"
            placeholder={saved ? 'saved — type a new key to replace it' : 'LeanZero staff only'}
            value={text}
            onChange={(event) => setText(event.target.value.slice(0, 256))}
            onKeyDown={(event) => {
              if (event.key === 'Enter' && text.trim()) void write(text);
            }}
            onBlur={() => {
              if (text.trim()) void write(text);
            }}
            disabled={disabled || !loaded || !available}
            className={cx('w-64 bg-transparent text-lz-ink', FOCUS)}
          />
        </span>
        {saved && (
          <button
            type="button"
            onClick={() => void write('')}
            disabled={disabled}
            className={cx('rounded-lg border border-lz-border px-3 py-2 text-lz-ink', FOCUS)}
          >
            Remove key
          </button>
        )}
      </label>
      {error ? (
        <p role="alert" className="text-lz-err">
          {error}
        </p>
      ) : (
        <span className={TYPE.meta}>
          {!available
            ? "This Mac's keychain encryption is not available, so an operator key cannot be stored."
            : saved
              ? 'Saved in this Mac’s keychain. Single-model cloud runs you publish are listed as LeanZero cloud baselines once leanzero.net accepts the key.'
              : 'Leave empty unless LeanZero gave you a key. Without one, runs publish as community entries.'}
        </span>
      )}
    </div>
  );
}
