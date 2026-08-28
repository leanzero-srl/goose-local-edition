import React from 'react';
import { ChevronDown, ChevronRight } from 'lucide-react';

/**
 * The ONE header register every zone of the swarm run view uses — a solid color mark, a 10px mono
 * uppercase name, and a plain-language explainer of what the zone IS. Mihai's critique of the old view
 * was that its parts "float" with no visual definition or explanation; this applies the same treatment
 * to every zone (run header, planning, fleet, work, event log) AND to the benchmark chrome around the
 * panel, so "which is which" is never ambiguous. Solid saturated mark per zone — never a tint, never a
 * left rail.
 */

/** The zone palette — one solid, saturated hue per zone, used for its mark and name. Every entry routes
 *  through a theme token so a zone header is legible on both canvases; the fallback is the light value.
 *  The old literals were fixed dark-mode hues, which washed out on the light canvas — the one place this
 *  view was allowed to look faded, and it did. */
export const ZONE_HUES = {
  run: 'var(--color-accent-local, #1d4ed8)',
  planning: 'var(--color-tertiary-local, #7c3aed)',
  fleet: 'var(--color-secondary-local, #0891b2)',
  work: 'var(--color-status-warn, #d97706)',
  log: 'var(--color-text-secondary)',
  bench: 'var(--color-node-5, #db2777)',
  /** Known active bugs — solid error red. A run that shipped green while carrying MINOR defects has to
   *  say so as loudly as it says "verified", or the green is a lie by omission. */
  bugs: 'var(--color-status-error, #dc2626)',
} as const;

export const ZoneHeader: React.FC<{
  hue: string;
  label: string;
  /** What this zone IS, in plain words — rendered lowercase-secondary after the name. */
  explain?: string;
  /** Right-aligned content (counts, chips, a summary when collapsed). */
  right?: React.ReactNode;
  collapsed?: boolean;
  onToggle?: () => void;
  className?: string;
}> = ({ hue, label, explain, right, collapsed, onToggle, className = '' }) => {
  const body = (
    <>
      <span
        aria-hidden
        className="shrink-0"
        style={{ width: 8, height: 8, background: hue, borderRadius: 1 }}
      />
      <span
        className="font-mono text-[10px] font-bold uppercase tracking-[0.18em] shrink-0"
        style={{ color: hue }}
      >
        {label}
      </span>
      {explain ? (
        <span className="text-[11px] text-text-secondary normal-case truncate">— {explain}</span>
      ) : null}
      <span className="ml-auto flex items-center gap-2 shrink-0 min-w-0">{right}</span>
      {onToggle ? (
        collapsed ? (
          <ChevronRight className="h-3 w-3 shrink-0 text-text-secondary" />
        ) : (
          <ChevronDown className="h-3 w-3 shrink-0 text-text-secondary" />
        )
      ) : null}
    </>
  );
  return onToggle ? (
    <button
      type="button"
      onClick={onToggle}
      className={`w-full flex items-center gap-2 px-3 py-2 text-left hover:bg-background-primary transition-colors ${className}`}
    >
      {body}
    </button>
  ) : (
    <div className={`flex items-center gap-2 px-3 py-2 ${className}`}>{body}</div>
  );
};

export default ZoneHeader;
