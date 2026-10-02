import { useRef, type KeyboardEvent, type ReactNode } from 'react';
import { Gauge, Hammer } from 'lucide-react';
import { FAMILY_FILL, FOCUS, MOTION, RADIUS, SURFACE, WEIGHT, cx } from '../lz';
import { BENCH_FAMILIES, FAMILY_WORD, type BenchFamily } from './baselines';
const FAMILY_ICON: Record<BenchFamily, ReactNode> = { sb: <Gauge />, forge: <Hammer /> };

/**
 * THE benchmark-type switch (owner 2026-10-02: "a toggle like it will be on the website, something
 * striking"): two large tiles, the chosen family a solid fill in its own hue, the other an outlined
 * surface. A radiogroup with roving focus — arrows move AND select, Home/End jump — so the keyboard
 * reaches it exactly as the pointer does. Switching re-scopes the whole view to that family.
 */
export function BenchmarkFamilyToggle({
  value,
  onChange,
  names,
}: {
  value: BenchFamily;
  onChange: (family: BenchFamily) => void;
  /** The bundled era per family as copy names it (`Gauntlet 7.2 · payments`, `Forge 1.0 · Scope Ledger`). */
  names: Record<BenchFamily, string>;
}) {
  const refs = useRef<Partial<Record<BenchFamily, HTMLButtonElement | null>>>({});
  const move = (to: BenchFamily) => {
    onChange(to);
    refs.current[to]?.focus();
  };
  const onKeyDown = (event: KeyboardEvent<HTMLButtonElement>, index: number) => {
    const last = BENCH_FAMILIES.length - 1;
    const next =
      event.key === 'ArrowRight' || event.key === 'ArrowDown'
        ? index === last
          ? 0
          : index + 1
        : event.key === 'ArrowLeft' || event.key === 'ArrowUp'
          ? index === 0
            ? last
            : index - 1
          : event.key === 'Home'
            ? 0
            : event.key === 'End'
              ? last
              : null;
    if (next == null) return;
    event.preventDefault();
    move(BENCH_FAMILIES[next]);
  };
  return (
    <div
      role="radiogroup"
      aria-label="Benchmark type"
      data-testid="bench-family-toggle"
      className={cx(
        'flex w-full gap-1 border-2 border-lz-border-strong bg-lz-surface p-1',
        RADIUS.card
      )}
    >
      {BENCH_FAMILIES.map((family, index) => {
        const active = family === value;
        return (
          <button
            key={family}
            ref={(node) => {
              refs.current[family] = node;
            }}
            type="button"
            role="radio"
            aria-checked={active}
            tabIndex={active ? 0 : -1}
            data-testid={`bench-family-${family}`}
            data-family={family}
            data-state={active ? 'on' : 'off'}
            onClick={() => onChange(family)}
            onKeyDown={(event) => onKeyDown(event, index)}
            className={cx(
              'flex h-16 min-w-0 flex-1 items-center gap-3 px-5 text-left [&>svg]:size-7 [&>svg]:shrink-0',
              RADIUS.control,
              FOCUS,
              MOTION,
              active ? FAMILY_FILL[family] : cx('text-lz-ink-2', SURFACE.hover)
            )}
          >
            {FAMILY_ICON[family]}
            <span className="flex min-w-0 flex-col">
              <span className={cx('text-lz-h1 leading-none', WEIGHT.semibold)}>
                {FAMILY_WORD[family]}
              </span>
              <span className={cx('mt-1 truncate text-lz-meta', WEIGHT.medium)}>
                {names[family]}
              </span>
            </span>
          </button>
        );
      })}
    </div>
  );
}
