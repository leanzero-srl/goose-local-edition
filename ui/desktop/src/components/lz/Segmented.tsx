import type { KeyboardEvent, ReactNode } from 'react';
import { DISABLED, FOCUS, MOTION, RADIUS, SURFACE, cx } from './tokens';

export interface SegmentedOption<V extends string> {
  value: V;
  label: ReactNode;
  icon?: ReactNode;
  disabled?: boolean;
}

export interface SegmentedProps<V extends string> {
  options: readonly SegmentedOption<V>[];
  value: V;
  onChange: (value: V) => void;
  'aria-label': string;
  size?: 'sm' | 'md';
  className?: string;
}

const SIZE = { sm: 'h-6 px-2 text-[11px]', md: 'h-7 px-2.5 text-[12px]' } as const;

/**
 * A controlled single-choice group: neutral outline, the active segment is the accent fill with
 * white ink. A radiogroup with roving focus — Arrow keys move AND select, Home/End jump.
 */
export function Segmented<V extends string>({
  options,
  value,
  onChange,
  size = 'md',
  className,
  'aria-label': ariaLabel,
}: SegmentedProps<V>) {
  const onKeyDown = (e: KeyboardEvent<HTMLDivElement>) => {
    const enabled = options.filter((o) => !o.disabled);
    const i = enabled.findIndex((o) => o.value === value);
    if (i < 0) return;
    let next = -1;
    if (e.key === 'ArrowRight' || e.key === 'ArrowDown') next = (i + 1) % enabled.length;
    else if (e.key === 'ArrowLeft' || e.key === 'ArrowUp')
      next = (i - 1 + enabled.length) % enabled.length;
    else if (e.key === 'Home') next = 0;
    else if (e.key === 'End') next = enabled.length - 1;
    if (next < 0 || next === i) return;
    e.preventDefault();
    const target = enabled[next].value;
    onChange(target);
    e.currentTarget.querySelector<HTMLButtonElement>(`[data-value="${target}"]`)?.focus();
  };

  return (
    <div
      role="radiogroup"
      aria-label={ariaLabel}
      onKeyDown={onKeyDown}
      data-testid="lz-segmented"
      className={cx(
        'inline-flex items-center gap-0.5 bg-lz-surface p-0.5',
        SURFACE.outline,
        RADIUS.control,
        className
      )}
    >
      {options.map((o) => {
        const active = o.value === value;
        return (
          <button
            key={o.value}
            type="button"
            role="radio"
            aria-checked={active}
            data-value={o.value}
            tabIndex={active ? 0 : -1}
            disabled={o.disabled}
            onClick={() => onChange(o.value)}
            className={cx(
              'inline-flex items-center gap-1.5 whitespace-nowrap rounded-[4px] font-lz-medium [&_svg]:size-3.5 [&_svg]:shrink-0',
              SIZE[size],
              active ? SURFACE.selected : cx('text-lz-ink-2 hover:text-lz-ink', SURFACE.hover),
              DISABLED,
              FOCUS,
              MOTION
            )}
          >
            {o.icon != null && <span aria-hidden>{o.icon}</span>}
            {o.label}
          </button>
        );
      })}
    </div>
  );
}
