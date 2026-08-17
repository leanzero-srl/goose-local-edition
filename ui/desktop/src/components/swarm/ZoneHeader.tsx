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

/** The zone palette — one solid, saturated hue per zone, used for its mark and name. */
export const ZONE_HUES = {
  run: '#2e8bff',
  planning: '#b14cff',
  fleet: '#17c4c4',
  work: '#f5a623',
  log: '#8a8a8a',
  bench: '#ff3ea5',
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
        className="font-mono text-[10px] font-bold uppercase tracking-[0.14em] shrink-0"
        style={{ color: hue }}
      >
        {label}
      </span>
      {explain ? (
        <span className="text-[10px] text-text-secondary normal-case truncate">— {explain}</span>
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
      className={`w-full flex items-center gap-2 px-3 py-1.5 text-left hover:bg-background-primary/40 transition-colors ${className}`}
    >
      {body}
    </button>
  ) : (
    <div className={`flex items-center gap-2 px-3 py-1.5 ${className}`}>{body}</div>
  );
};

export default ZoneHeader;
