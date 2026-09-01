import { useEffect, useRef, useState } from 'react';
import { Check } from 'lucide-react';
import { Tooltip, TooltipTrigger, TooltipContent } from '../ui/Tooltip';
import { Button, FOCUS, RADIUS, TNUM, WEIGHT, cx } from '../lz';
import {
  SAMPLING_KNOBS,
  clampKnob,
  type SamplingKnobId,
  type SamplingSettings,
} from './sampling';

/**
 * The ONE sampling strip every run surface uses (benchmark view + the normal swarm run window):
 * temperature, top-p, top-k, min-p and repeat penalty, inline-editable before launch, read-only with
 * the launched values while a run is live. An unset knob reads "model default" — the run sends
 * nothing for it, so the Settings default (config) and finally the model's own default apply.
 *
 * ONE QUIET META LINE. It used to be a bordered box with a tracked uppercase mono "SAMPLING" eyebrow
 * and five uppercase micro-labels each in its own hue — five colours on facts that carry no state.
 * Now: the meta register (11px, normal case, ink-3), a set value in ink with tabular figures, an
 * unset one as the words "model default". Custom inline inputs (no native controls beyond a plain
 * text field); never a left rail, never a tint.
 */

function KnobField({
  id,
  value,
  readOnly,
  placeholder,
  onCommit,
}: {
  id: SamplingKnobId;
  value: number | undefined;
  readOnly: boolean;
  /** What an UNSET knob means here (default "model default"; no knob is pinned anywhere any more). */
  placeholder?: string;
  onCommit: (v: number | undefined) => void;
}) {
  const [text, setText] = useState(value === undefined ? '' : String(value));
  useEffect(() => setText(value === undefined ? '' : String(value)), [value]);
  const unsetLabel = placeholder ?? 'model default';

  if (readOnly) {
    return value === undefined ? (
      <span className="text-lz-ink-3">{unsetLabel}</span>
    ) : (
      <span className={cx('text-lz-ink', WEIGHT.medium, TNUM)}>{value}</span>
    );
  }

  const commit = () => {
    if (text.trim() === '') return onCommit(undefined);
    const n = Number(text);
    onCommit(Number.isNaN(n) ? undefined : clampKnob(id, n));
  };

  return (
    <input
      type="text"
      inputMode="decimal"
      className={cx(
        'h-6 w-[5.5rem] border border-lz-border-strong bg-lz-surface px-1.5 text-right text-lz-meta text-lz-ink placeholder:text-lz-ink-3',
        WEIGHT.medium,
        TNUM,
        RADIUS.control,
        FOCUS
      )}
      value={text}
      placeholder={unsetLabel}
      aria-label={`${id} sampling value`}
      onChange={(e) => setText(e.target.value)}
      onBlur={commit}
      onKeyDown={(e) => {
        if (e.key === 'Enter') (e.target as HTMLInputElement).blur();
      }}
    />
  );
}

export default function SamplingKnobs({
  value,
  onChange,
  active = false,
  explain,
  placeholders,
  onSaveDefaults,
  className = '',
}: {
  value: SamplingSettings;
  onChange: (next: SamplingSettings) => void;
  /** True while a run is live — the strip goes read-only, showing the values that run launched with. */
  active?: boolean;
  explain?: string;
  /** Per-knob label for the UNSET state, where it differs from "model default". */
  placeholders?: Partial<Record<SamplingKnobId, string>>;
  onSaveDefaults?: () => void;
  className?: string;
}) {
  const [saved, setSaved] = useState(false);
  const savedTimer = useRef<ReturnType<typeof setTimeout> | null>(null);
  useEffect(
    () => () => {
      if (savedTimer.current) clearTimeout(savedTimer.current);
    },
    []
  );

  const setKnob = (id: SamplingKnobId, v: number | undefined) => {
    const next: SamplingSettings = { ...value };
    if (v === undefined) delete next[id];
    else next[id] = v;
    onChange(next);
  };

  return (
    <div
      className={cx(
        'flex flex-wrap items-center gap-x-3 gap-y-1 px-1 text-lz-meta text-lz-ink-3',
        className
      )}
      data-testid="sampling-knobs"
    >
      <span className={cx('shrink-0 text-lz-ink-2', WEIGHT.medium)}>Sampling</span>
      <span className="truncate">
        {explain ?? (active ? 'this run launched with these values' : 'the next run uses these values')}
      </span>
      {SAMPLING_KNOBS.map((k) => (
        <Tooltip key={k.id}>
          <TooltipTrigger asChild>
            <label className="flex items-center gap-1">
              <span>{k.label}</span>
              <KnobField
                id={k.id}
                value={value[k.id]}
                readOnly={active}
                placeholder={placeholders?.[k.id]}
                onCommit={(v) => setKnob(k.id, v)}
              />
            </label>
          </TooltipTrigger>
          <TooltipContent className="max-w-xs">{k.hint}</TooltipContent>
        </Tooltip>
      ))}
      {!active && onSaveDefaults && (
        <Button
          variant="ghost"
          size="sm"
          className="ml-auto"
          icon={saved ? <Check className="text-lz-ok" /> : undefined}
          onClick={() => {
            onSaveDefaults();
            setSaved(true);
            if (savedTimer.current) clearTimeout(savedTimer.current);
            savedTimer.current = setTimeout(() => setSaved(false), 2000);
          }}
        >
          {saved ? 'saved' : 'save as defaults'}
        </Button>
      )}
    </div>
  );
}
