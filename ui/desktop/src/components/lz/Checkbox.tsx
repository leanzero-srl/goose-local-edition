import { useId, type ReactNode } from 'react';
import { Check } from 'lucide-react';
import { FOCUS, MOTION, RADIUS, SURFACE, cx } from './tokens';

export interface CheckboxProps {
  checked: boolean;
  onChange: (checked: boolean) => void;
  /** The accessible name — the text a person reads beside the box. */
  label: ReactNode;
  /** One quieter line under the label; it becomes the control's `aria-describedby`. */
  description?: ReactNode;
  disabled?: boolean;
  /**
   * `inline` (default): the box and its words on one row. `card`: a full-width tile — a hairline
   * edge that turns into the 2px accent ring when checked, for a grid of choices.
   */
  variant?: 'inline' | 'card';
  className?: string;
  testId?: string;
}

/**
 * A checkbox on the Studio tokens — never the native `<input type="checkbox">`, whose chrome the
 * app cannot style. A `role="checkbox"` button carrying `aria-checked`: Space and Enter toggle it
 * (the button does both), the WHOLE row is the hit target, the name is the label alone and the
 * description is announced as its description. Checked is the solid accent fill with a white tick;
 * unchecked is a surface box on the strong outline; disabled is the solid surface-2 neutral.
 */
export function Checkbox({
  checked,
  onChange,
  label,
  description,
  disabled = false,
  variant = 'inline',
  className,
  testId,
}: CheckboxProps) {
  const id = useId();
  const labelId = `${id}-label`;
  const descId = `${id}-desc`;
  return (
    <button
      type="button"
      role="checkbox"
      aria-checked={checked}
      aria-labelledby={labelId}
      aria-describedby={description != null ? descId : undefined}
      disabled={disabled}
      data-testid={testId ?? 'lz-checkbox'}
      data-state={checked ? 'checked' : 'unchecked'}
      onClick={() => onChange(!checked)}
      className={cx(
        'flex w-full min-w-0 items-start gap-3 text-left disabled:pointer-events-none',
        variant === 'card'
          ? cx(
              'p-3',
              RADIUS.card,
              checked
                ? cx('border border-lz-accent bg-lz-surface', SURFACE.selectedRing)
                : cx('border bg-lz-surface', SURFACE.hairline, SURFACE.hover),
              'disabled:bg-lz-surface-2'
            )
          : 'py-0.5',
        FOCUS,
        MOTION,
        className
      )}
    >
      <span
        aria-hidden
        className={cx(
          'mt-px flex size-4 shrink-0 items-center justify-center rounded-[4px] border [&_svg]:size-3',
          checked
            ? 'border-lz-accent bg-lz-accent text-lz-accent-ink'
            : 'border-lz-border-strong bg-lz-surface',
          disabled && 'border-lz-border bg-lz-surface-2 text-lz-ink-3',
          MOTION
        )}
      >
        {checked && <Check strokeWidth={3} />}
      </span>
      <span className="flex min-w-0 flex-col gap-0.5">
        <span
          id={labelId}
          className={cx(
            'text-lz-body',
            variant === 'card' ? 'font-lz-medium' : undefined,
            disabled ? 'text-lz-ink-3' : 'text-lz-ink'
          )}
        >
          {label}
        </span>
        {description != null && (
          <span id={descId} className="text-lz-meta text-lz-ink-3">
            {description}
          </span>
        )}
      </span>
    </button>
  );
}
