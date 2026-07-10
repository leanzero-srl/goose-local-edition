import { Code2, Bot } from 'lucide-react';

/**
 * Goose Local Edition — persona chooser for the chat input bar. Two personas:
 *  - 'coding': interactive build-with-the-fleet (one brief → one build).
 *  - 'agent':  the autonomous implementation — runs a loop with a recipe + skills, iterating on its own.
 * Presentational only (value + onChange). Sharp, full-border, solid azure on the active option per the
 * hard UI rules (no left rail, no faded tints, no native <select>).
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
      className={`inline-flex border border-border-primary ${className}`}
      style={{ borderRadius: 3 }}
      role="group"
      aria-label="Persona"
    >
      {OPTIONS.map((opt, i) => {
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
            className={`flex items-center gap-1 px-2 py-0.5 text-xs transition-colors ${
              active ? 'font-semibold text-background-primary' : 'text-text-secondary hover:text-text-primary'
            } ${i > 0 ? 'border-l border-border-primary' : ''}`}
            style={{ backgroundColor: active ? '#2e8bff' : 'transparent' }}
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
