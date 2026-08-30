import { useEffect, useRef, useState } from 'react';
import { SlidersHorizontal, Check } from 'lucide-react';
import { Tooltip, TooltipTrigger, TooltipContent } from '../ui/Tooltip';
import { CHIP_RADIUS, SWARM_STATUS } from './formationVisualState';
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
 * Honors the hard UI rules: full border (never a left rail), one solid saturated hue per knob value
 * (never a tint), custom inline inputs (no native controls beyond a plain text field).
 */

const STRIP_HUE = SWARM_STATUS.action;

function KnobField({
  id,
  value,
  hue,
  readOnly,
  placeholder,
  onCommit,
}: {
  id: SamplingKnobId;
  value: number | undefined;
  hue: string;
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
      <span className="font-mono text-xs text-text-secondary">{unsetLabel}</span>
    ) : (
      <span className="font-mono text-xs font-bold tabular-nums" style={{ color: hue }}>
        {value}
      </span>
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
      className="w-[5.5rem] border border-border-primary bg-background-primary px-1.5 py-0.5 text-right font-mono text-xs font-bold tabular-nums placeholder:font-normal placeholder:text-text-secondary focus:border-border-secondary focus-visible:outline-none"
      style={{ borderRadius: CHIP_RADIUS, color: hue }}
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
      className={`border border-border-primary px-3 py-2 ${className}`}
      style={{ borderRadius: CHIP_RADIUS }}
      data-testid="sampling-knobs"
    >
      <div className="flex items-center gap-2">
        <SlidersHorizontal className="h-3 w-3 shrink-0" style={{ color: STRIP_HUE }} />
        <span
          className="shrink-0 font-mono text-[10px] font-bold uppercase tracking-[0.14em]"
          style={{ color: STRIP_HUE }}
        >
          sampling
        </span>
        <span className="truncate text-[10px] text-text-secondary">
          — {explain ?? (active ? 'this run launched with these values' : 'the next run uses these values')}
        </span>
        {!active && onSaveDefaults && (
          <button
            type="button"
            onClick={() => {
              onSaveDefaults();
              setSaved(true);
              if (savedTimer.current) clearTimeout(savedTimer.current);
              savedTimer.current = setTimeout(() => setSaved(false), 2000);
            }}
            className="ml-auto flex shrink-0 items-center gap-1 border border-border-primary px-2 py-0.5 text-[10px] font-semibold text-text-secondary transition-colors hover:border-text-secondary hover:text-text-primary"
            style={{ borderRadius: CHIP_RADIUS }}
          >
            {saved ? (
              <>
                <Check className="h-3 w-3" style={{ color: SWARM_STATUS.done }} />
                saved
              </>
            ) : (
              'save as defaults'
            )}
          </button>
        )}
      </div>
      <div className="mt-1.5 flex flex-wrap items-center gap-x-4 gap-y-1.5">
        {SAMPLING_KNOBS.map((k) => (
          <Tooltip key={k.id}>
            <TooltipTrigger asChild>
              <label className="flex items-center gap-1.5">
                <span className="font-mono text-[10px] font-bold uppercase tracking-wider text-text-secondary">
                  {k.label}
                </span>
                <KnobField
                  id={k.id}
                  value={value[k.id]}
                  hue={k.hue}
                  readOnly={active}
                  placeholder={placeholders?.[k.id]}
                  onCommit={(v) => setKnob(k.id, v)}
                />
              </label>
            </TooltipTrigger>
            <TooltipContent className="max-w-xs">{k.hint}</TooltipContent>
          </Tooltip>
        ))}
      </div>
    </div>
  );
}
