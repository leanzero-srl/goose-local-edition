import type { ReactNode } from 'react';
import { SURFACE, TYPE, cx } from '../lz';

/**
 * One labelled per-model setting: the label on the left, the control and its one-line note on the
 * right. Shared by the MLX engine's thinking rows (whose choices come from the model's chat
 * template) and the cloud models' custom fields (whose choices come from the provider's metadata),
 * so a model's knobs read the same wherever they are set.
 */
export function FieldRow({
  label,
  note,
  children,
  testId,
}: {
  label: string;
  note?: ReactNode;
  children: ReactNode;
  testId?: string;
}) {
  return (
    <div
      data-testid={testId}
      className={cx(
        'grid grid-cols-[minmax(160px,240px)_1fr] items-center gap-4 border-t py-2',
        SURFACE.hairline
      )}
    >
      <span className={cx('truncate', TYPE.body)}>{label}</span>
      <div className="flex min-w-0 flex-col gap-1">
        <div className="flex min-w-0 flex-wrap items-center gap-2">{children}</div>
        {note != null && <span className={TYPE.meta}>{note}</span>}
      </div>
    </div>
  );
}

/** A full-width line in the same rhythm as the rows, for "nothing to set, and why". */
export function FieldNotice({ text, testId }: { text: string; testId?: string }) {
  return (
    <div className={cx('border-t py-2', SURFACE.hairline)}>
      <span className={TYPE.meta} data-testid={testId}>
        {text}
      </span>
    </div>
  );
}
