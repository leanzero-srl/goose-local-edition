import { useEffect, useState } from 'react';
import { acpReadConfig, acpUpsertConfig } from '../../acp/config';
import { Switch } from '../ui/switch';
import { TYPE } from '../lz';

/** goose config.yaml key; the OpenRouter provider reads the same key
 *  (`openrouter.rs` OPENROUTER_MIDDLE_OUT_CONFIG_KEY). */
export const MIDDLE_OUT_KEY = 'OPENROUTER_MIDDLE_OUT';

/** On only when the owner turned it on, read the way goose reads a boolean. */
export function middleOutFrom(value: unknown): boolean {
  return value === true || (typeof value === 'string' && value.trim().toLowerCase() === 'true');
}

function message(err: unknown): string {
  return err instanceof Error ? err.message : String(err);
}

/**
 * Whether OpenRouter may compress a request that outgrows the model's window by dropping the
 * middle of the conversation (middle-out). Off by default: off, an over-long request is refused
 * and goose compacts the chat itself, so the model always sees what goose sent. Benchmark runs
 * never carry it.
 */
export function OpenRouterMiddleOutSwitch({ disabled = false }: { disabled?: boolean }) {
  const [enabled, setEnabled] = useState<boolean | null>(null);
  const [problem, setProblem] = useState<string | null>(null);

  useEffect(() => {
    let alive = true;
    acpReadConfig(MIDDLE_OUT_KEY, false).then(
      (value) => {
        if (alive) setEnabled(middleOutFrom(value));
      },
      (error: unknown) => {
        if (alive) setProblem(`Could not read this setting: ${message(error)}`);
      }
    );
    return () => {
      alive = false;
    };
  }, []);

  const toggle = async (checked: boolean) => {
    const before = enabled;
    setEnabled(checked);
    setProblem(null);
    try {
      await acpUpsertConfig(MIDDLE_OUT_KEY, checked, false);
    } catch (error) {
      setEnabled(before);
      setProblem(`Could not save this setting: ${message(error)}`);
    }
  };

  return (
    <div
      className="flex items-start justify-between gap-4 text-sm"
      data-testid="openrouter-middle-out"
    >
      <div className="flex flex-col gap-1">
        <span>Let OpenRouter compress long chats (middle-out)</span>
        <span className={TYPE.meta}>
          Off: a chat longer than the model's window is refused and goose summarizes it itself. On:
          OpenRouter silently drops the middle of the conversation to fit, and may route it to a
          host with a smaller window. The model then loses work it did earlier in the chat.
          Benchmark runs never use it.
        </span>
        {problem && (
          <p role="alert" className="text-lz-err">
            {problem}
          </p>
        )}
      </div>
      <Switch
        aria-label="Let OpenRouter compress long chats"
        data-testid="openrouter-middle-out-switch"
        checked={enabled ?? false}
        disabled={disabled || enabled == null}
        onCheckedChange={(checked: boolean) => void toggle(checked)}
        variant="mono"
      />
    </div>
  );
}
