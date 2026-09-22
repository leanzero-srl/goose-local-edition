import { useEffect, useRef, useState } from 'react';
import { BaseModal } from '../ui/BaseModal';
import { Button } from '../lz';
import { INPUT } from '../leanzero-swarm/studio';

/** The app's own rename dialog — never window.prompt. Enter saves, Escape cancels. */
export function RenameDialog({
  title,
  label,
  initial,
  saveLabel,
  cancelLabel,
  busy,
  error,
  onSave,
  onCancel,
}: {
  title: string;
  label: string;
  initial: string;
  saveLabel: string;
  cancelLabel: string;
  busy?: boolean;
  error?: string | null;
  onSave: (value: string) => void;
  onCancel: () => void;
}) {
  const [value, setValue] = useState(initial);
  const ref = useRef<HTMLInputElement>(null);
  useEffect(() => {
    ref.current?.focus();
    ref.current?.select();
  }, []);
  const trimmed = value.trim();
  return (
    <BaseModal
      isOpen
      title={title}
      actions={
        <div className="flex justify-end gap-2 px-6 pb-4">
          <Button variant="ghost" onClick={onCancel} disabled={busy}>
            {cancelLabel}
          </Button>
          <Button
            variant="primary"
            disabled={busy || !trimmed || trimmed === initial.trim()}
            onClick={() => onSave(trimmed)}
            data-testid="rename-save"
          >
            {saveLabel}
          </Button>
        </div>
      }
    >
      <form
        className="flex flex-col gap-2"
        onSubmit={(e) => {
          e.preventDefault();
          if (trimmed && !busy) onSave(trimmed);
        }}
        onKeyDown={(e) => {
          if (e.key === 'Escape') onCancel();
        }}
      >
        <label className="flex flex-col gap-1 text-lz-meta text-lz-ink-3">
          {label}
          <input
            ref={ref}
            className={INPUT}
            value={value}
            onChange={(e) => setValue(e.target.value)}
            aria-label={label}
            disabled={busy}
          />
        </label>
        {error && (
          <p role="alert" className="text-lz-meta text-lz-err">
            {error}
          </p>
        )}
      </form>
    </BaseModal>
  );
}
