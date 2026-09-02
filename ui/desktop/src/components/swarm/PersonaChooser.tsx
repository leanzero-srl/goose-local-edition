import { Code2, Bot } from 'lucide-react';
import { FOCUS, MOTION, RADIUS, SURFACE, WEIGHT, cx } from '../lz';

/**
 * Goose Local Edition — persona chooser for the chat input bar. Two personas:
 *  - 'coding': interactive build-with-the-fleet (one brief → one build).
 *  - 'agent':  the autonomous implementation — runs a loop with a recipe + skills, iterating on its own.
 * Presentational only (value + onChange). The Studio segmented register by hand: the active option is
 * the accent fill with white ink, the others the quiet ink with a solid hover step; segments are divided
 * by the container's divide-x hairline, never a left border on a button (no left rail, no faded tints,
 * no native dropdown). Kept a pressed-button group so the aria-pressed contract stays as it was.
 */

export type Persona = 'coding' | 'agent';

const OPTIONS: { value: Persona; label: string; Icon: typeof Code2 }[] = [
  { value: 'coding', label: 'Coding', Icon: Code2 },
  { value: 'agent', label: 'Agent', Icon: Bot },
];

export function PersonaChooser({
  value,
  onChange,
  className = '',
}: {
  value: Persona;
  onChange: (p: Persona) => void;
  className?: string;
}) {
  return (
    <div
      className={cx(
        'inline-flex divide-x divide-lz-border-strong overflow-hidden border border-lz-border-strong',
        RADIUS.control,
        className
      )}
      role="group"
      aria-label="Persona"
    >
      {OPTIONS.map((opt) => {
        const active = opt.value === value;
        const Icon = opt.Icon;
        return (
          <button
            key={opt.value}
            type="button"
            onClick={() => onChange(opt.value)}
            aria-pressed={active}
            title={
              opt.value === 'agent'
                ? 'Autonomous — runs a loop with a recipe + skills'
                : 'Interactive build with your fleet'
            }
            className={cx(
              'flex items-center gap-1 px-2 py-0.5 text-lz-meta',
              MOTION,
              FOCUS,
              active
                ? cx(SURFACE.selected, WEIGHT.semibold)
                : cx('text-lz-ink-3 hover:text-lz-ink', SURFACE.hover)
            )}
          >
            <Icon className="h-3.5 w-3.5" />
            {opt.label}
          </button>
        );
      })}
    </div>
  );
}

export default PersonaChooser;
