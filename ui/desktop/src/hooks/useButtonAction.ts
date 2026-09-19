import { useRef, useState, type MouseEventHandler } from 'react';

/** Returned promises keep feedback attached to the actual operation, including failures. */
export function useButtonAction(onClick?: MouseEventHandler<HTMLButtonElement>) {
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string>();
  const pending = useRef(false);
  const click: MouseEventHandler<HTMLButtonElement> = (event) => {
    if (pending.current) return;
    setError(undefined);
    try {
      const result: unknown = onClick?.(event);
      if (result && typeof (result as Promise<unknown>).then === 'function') {
        pending.current = true;
        setBusy(true);
        void Promise.resolve(result)
          .catch((cause: unknown) => {
            setError(cause instanceof Error ? cause.message : String(cause));
          })
          .finally(() => {
            pending.current = false;
            setBusy(false);
          });
      }
    } catch (cause) {
      setError(cause instanceof Error ? cause.message : String(cause));
    }
  };
  return { busy, error, click };
}
