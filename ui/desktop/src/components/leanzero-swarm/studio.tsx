import { useEffect, useRef, useState, type KeyboardEvent, type ReactNode } from 'react';
import { ChevronDown, Loader2, Minus, Plus } from 'lucide-react';
import {
  DISABLED,
  FOCUS,
  MOTION,
  RADIUS,
  ROW,
  SURFACE,
  StatusDot,
  TNUM,
  TONE_FILL,
  TONE_TEXT,
  TYPE,
  WEIGHT,
  cx,
  type NodeIndex,
  type Tone,
} from '../lz';

/**
 * LeanZero Studio compositions the hub surfaces share and `src/components/lz` does not yet
 * carry: a text-input recipe, a tone banner, a weight stepper, a listbox select and a switch.
 * Every class is a token utility from `lz/tokens`; nothing here owns data or talks to the
 * backend. Candidates for promotion into `lz/` once the remake wave settles.
 */

/** A 32px text input on the Studio outline; add the width at the call site. */
export const INPUT = cx(
  'h-8 bg-lz-surface px-3 text-lz-body text-lz-ink placeholder:text-lz-ink-4',
  SURFACE.outline,
  RADIUS.control,
  FOCUS,
  MOTION,
  DISABLED
);

/** A resizable mono textarea on the same outline. */
export const TEXTAREA = cx(
  'min-h-[72px] w-full resize-y bg-lz-surface px-3 py-2 font-mono text-lz-mono text-lz-ink placeholder:text-lz-ink-4',
  SURFACE.outline,
  RADIUS.control,
  FOCUS,
  MOTION,
  DISABLED
);

/** A form-field label: the meta register, normal case (uppercase belongs to zone headers only). */
export const FIELD_LABEL = TYPE.meta;

/** Node identity hue for the i-th node in a list — the six-step ramp, cycling. */
export function nodeHue(index: number): NodeIndex {
  return ((Math.max(0, index) % 6) + 1) as NodeIndex;
}

export interface ToneBannerProps {
  tone: Extract<Tone, 'ok' | 'warn' | 'err' | 'accent' | 'stopped'>;
  /** The short name of what is speaking ("Sign-in", "Mesh", "Run"). */
  label: string;
  /** The message — backend text rides through VERBATIM, never paraphrased. */
  text: string;
  action?: ReactNode;
  /** The dot pulses (by scale) while something is in flight. */
  live?: boolean;
  testId?: string;
  className?: string;
}

/** A surface card carrying one solid status dot, a toned label and the message in body ink. */
export function ToneBanner({ tone, label, text, action, live, testId, className }: ToneBannerProps) {
  return (
    <div
      role={tone === 'err' ? 'alert' : 'status'}
      data-testid={testId}
      data-tone={tone}
      className={cx('flex items-center gap-3 px-4 py-3', SURFACE.card, className)}
    >
      <StatusDot tone={tone} label={label} size={10} live={live} />
      <span className={cx('shrink-0 text-lz-meta', WEIGHT.semibold, TONE_TEXT[tone])}>{label}</span>
      <span className={cx('min-w-0 flex-1 break-words', TYPE.body)}>{text}</span>
      {action}
    </div>
  );
}

const STEP_BUTTON = cx(
  'flex size-7 items-center justify-center bg-lz-surface text-lz-ink-2 hover:bg-lz-surface-2 hover:text-lz-ink [&_svg]:size-3.5',
  SURFACE.outline,
  RADIUS.control,
  FOCUS,
  MOTION
);

/**
 * The −/n/+ stepper for a node's routing share (1–9). Custom control, never a native slider
 * or select; the value is the accent in tabular figures so a column of them lines up.
 */
export function WeightStepper({
  value,
  onChange,
  label = 'weight',
}: {
  value: number;
  onChange: (v: number) => void;
  label?: string;
}) {
  const clamp = (v: number) => Math.max(1, Math.min(9, v));
  return (
    <span className="inline-flex items-center gap-1">
      <button
        type="button"
        onClick={() => onChange(clamp(value - 1))}
        className={STEP_BUTTON}
        aria-label={`Less work (${label})`}
      >
        <Minus />
      </button>
      <span
        className={cx('w-6 text-center text-lz-body text-lz-accent', WEIGHT.semibold, TNUM)}
      >
        {value}
      </span>
      <button
        type="button"
        onClick={() => onChange(clamp(value + 1))}
        className={STEP_BUTTON}
        aria-label={`More work (${label})`}
      >
        <Plus />
      </button>
    </span>
  );
}

/** A switch on the accent: solid fill when on, strong border when off. Never a native checkbox. */
export function StudioSwitch({
  checked,
  onChange,
  'aria-label': ariaLabel,
  disabled,
}: {
  checked: boolean;
  onChange: (v: boolean) => void;
  'aria-label'?: string;
  disabled?: boolean;
}) {
  return (
    <button
      type="button"
      role="switch"
      aria-checked={checked}
      aria-label={ariaLabel}
      disabled={disabled}
      onClick={() => onChange(!checked)}
      className={cx(
        'inline-flex h-5 w-9 shrink-0 items-center px-0.5',
        RADIUS.pill,
        checked ? 'bg-lz-accent' : 'bg-lz-border-strong',
        DISABLED,
        FOCUS,
        MOTION
      )}
    >
      <span
        aria-hidden
        className={cx(
          'block size-4 bg-white transition-transform duration-120 ease-lz',
          RADIUS.pill,
          checked ? 'translate-x-4' : 'translate-x-0'
        )}
      />
    </button>
  );
}

export interface StudioSelectOption {
  value: string;
  label: string;
  disabled?: boolean;
}

export interface StudioSelectProps<T extends StudioSelectOption> {
  options: readonly T[];
  value: T | null;
  onChange: (option: T | null) => void;
  placeholder: string;
  'aria-label': string;
  disabled?: boolean;
  /** The trigger shows a spinner and does not open. */
  loading?: boolean;
  /** Custom rendering; defaults to the label. `where` says whether this is a listbox option or the trigger's value. */
  renderOption?: (option: T, where: 'option' | 'value') => ReactNode;
  /** A per-option test id (the listbox pattern keeps `role="option"` for locators). */
  optionTestId?: (option: T) => string;
  className?: string;
}

/**
 * A listbox select — never a native `<select>`. The trigger is `role="combobox"`, the popover
 * `role="listbox"` of `role="option"` buttons; disabled options stay VISIBLE (honest) and cannot
 * be picked. Arrow keys move, Enter picks, Escape closes, an outside click closes.
 */
export function StudioSelect<T extends StudioSelectOption>({
  options,
  value,
  onChange,
  placeholder,
  'aria-label': ariaLabel,
  disabled,
  loading,
  renderOption,
  optionTestId,
  className,
}: StudioSelectProps<T>) {
  const [open, setOpen] = useState(false);
  const [highlight, setHighlight] = useState(0);
  const rootRef = useRef<HTMLDivElement>(null);
  const render = renderOption ?? ((o: T) => o.label);

  useEffect(() => {
    if (!open) return undefined;
    const onDocMouseDown = (e: MouseEvent) => {
      if (rootRef.current && !rootRef.current.contains(e.target as Node)) setOpen(false);
    };
    document.addEventListener('mousedown', onDocMouseDown);
    return () => document.removeEventListener('mousedown', onDocMouseDown);
  }, [open]);

  useEffect(() => {
    if (open) setHighlight(Math.max(0, options.findIndex((o) => o.value === value?.value)));
  }, [open, options, value]);

  const pick = (o: T) => {
    if (o.disabled) return;
    onChange(o);
    setOpen(false);
  };

  const onKeyDown = (e: KeyboardEvent<HTMLElement>) => {
    if (e.key === 'Escape') {
      if (open) {
        e.stopPropagation();
        setOpen(false);
      }
      return;
    }
    if (!open) {
      if (e.key === 'ArrowDown' || e.key === 'ArrowUp') {
        e.preventDefault();
        setOpen(true);
      }
      return;
    }
    if (e.key === 'ArrowDown') {
      e.preventDefault();
      setHighlight((h) => Math.min(h + 1, options.length - 1));
    } else if (e.key === 'ArrowUp') {
      e.preventDefault();
      setHighlight((h) => Math.max(0, h - 1));
    } else if (e.key === 'Enter') {
      e.preventDefault();
      const o = options[highlight];
      if (o) pick(o);
    }
  };

  const inert = disabled || loading || options.length === 0;

  return (
    <div ref={rootRef} className={cx('relative', className)} onKeyDown={onKeyDown}>
      <button
        type="button"
        role="combobox"
        aria-haspopup="listbox"
        aria-expanded={open}
        aria-label={ariaLabel}
        disabled={inert}
        onClick={() => setOpen((o) => !o)}
        className={cx(
          'flex h-8 w-full items-center gap-2 bg-lz-surface px-3 text-left text-lz-body text-lz-ink [&>svg]:size-4 [&>svg]:shrink-0',
          SURFACE.outline,
          RADIUS.control,
          DISABLED,
          FOCUS,
          MOTION
        )}
      >
        <span className="min-w-0 flex-1 truncate">
          {value ? render(value, 'value') : <span className="text-lz-ink-4">{placeholder}</span>}
        </span>
        {loading ? <Loader2 className="animate-spin text-lz-ink-3" /> : <ChevronDown />}
      </button>
      {open && (
        <div
          role="listbox"
          aria-label={ariaLabel}
          className={cx(
            'absolute left-0 top-full z-[60] mt-1 max-h-64 w-full overflow-y-auto p-1',
            SURFACE.overlay
          )}
        >
          {options.map((o, i) => {
            const selected = value?.value === o.value;
            return (
              <button
                key={o.value}
                type="button"
                role="option"
                aria-selected={selected}
                aria-disabled={o.disabled || undefined}
                disabled={o.disabled}
                data-testid={optionTestId?.(o)}
                onMouseEnter={() => setHighlight(i)}
                onClick={() => pick(o)}
                className={cx(
                  'flex w-full items-center gap-2 px-2.5 text-left text-lz-body',
                  ROW.dense,
                  RADIUS.control,
                  selected
                    ? TONE_FILL.accent
                    : cx(
                        'text-lz-ink disabled:cursor-not-allowed disabled:text-lz-ink-3',
                        i === highlight && 'bg-lz-surface-2'
                      ),
                  MOTION
                )}
              >
                {render(o, 'option')}
              </button>
            );
          })}
        </div>
      )}
    </div>
  );
}
