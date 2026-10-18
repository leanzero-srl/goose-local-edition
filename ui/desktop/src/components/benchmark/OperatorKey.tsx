import { useEffect, useRef, useState } from 'react';
import { acpReadConfig, acpRemoveConfig, acpUpsertConfig } from '../../acp/config';
import { FOCUS, TYPE, cx } from '../lz';

/** goose config key main reads at publish (main.ts benchOperatorKey; benchPublishIdentity.ts). */
export const OPERATOR_KEY_CONFIG = 'LEANZERO_BENCH_OPERATOR_KEY';

/**
 * LeanZero's own installs only: the operator key leanzero.net checks (it stores only the key's sha256) to list a
 * single-model CLOUD run as a LeanZero cloud baseline by itself, with no operator script. Anyone else leaves it
 * empty and publishes community runs as before. Saved on every edit, like the spend limit; never shown back.
 */
export function OperatorKey({ disabled }: { disabled: boolean }) {
  const [text, setText] = useState('');
  const [saved, setSaved] = useState(false);
  const [loaded, setLoaded] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const stored = useRef(false);

  useEffect(() => {
    let alive = true;
    acpReadConfig(OPERATOR_KEY_CONFIG)
      .then((value) => {
        if (!alive) return;
        stored.current = typeof value === 'string' && value.length > 0;
        setSaved(stored.current);
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

  const save = (next: string) => {
    setText(next);
    const key = next.trim();
    setError(null);
    const write = key
      ? acpUpsertConfig(OPERATOR_KEY_CONFIG, key).then(() => {
          stored.current = true;
          setSaved(true);
        })
      : stored.current
        ? acpRemoveConfig(OPERATOR_KEY_CONFIG, false).then(() => {
            stored.current = false;
            setSaved(false);
          })
        : Promise.resolve();
    write.catch((err: unknown) =>
      setError(`The operator key could not be saved: ${err instanceof Error ? err.message : String(err)}`)
    );
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
            placeholder={saved ? 'saved — type to replace, clear to remove' : 'LeanZero staff only'}
            value={text}
            onChange={(event) => save(event.target.value)}
            disabled={disabled || !loaded}
            className={cx('w-64 bg-transparent text-lz-ink', FOCUS)}
          />
        </span>
      </label>
      {error ? (
        <p role="alert" className="text-lz-err">
          {error}
        </p>
      ) : (
        <span className={TYPE.meta}>
          {saved
            ? 'Saved. Single-model cloud runs you publish are listed as LeanZero cloud baselines once leanzero.net accepts the key.'
            : 'Leave empty unless LeanZero gave you a key. Without one, runs publish as community entries.'}
        </span>
      )}
    </div>
  );
}
