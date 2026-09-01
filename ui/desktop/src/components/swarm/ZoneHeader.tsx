import React from 'react';
import { ChevronDown, ChevronRight } from 'lucide-react';

/**
 * The ONE header register every zone of the swarm run view uses — the zone-header type scale
 * (11px, semibold, uppercase, 0.08em) and a plain-language explainer of what the zone IS. Mihai's
 * critique of the old view was that its parts "float" with no visual definition or explanation;
 * this applies the same treatment to every zone (run header, planning, fleet, work, event log) AND
 * to the benchmark chrome around the panel, so "which is which" is never ambiguous.
 *
 * A zone is NOT a node: it carries no hue. Hierarchy comes from type weight and case alone — the
 * per-zone colour marks (and node-5 pink on the benchmark) were colour on things that carry no
 * meaning. Never a tint, never a left rail.
 */

/** The zone-header scale — the ONLY place uppercase + tracking is used. */
export const ZONE_LABEL_CLASS = 'text-[11px] font-semibold uppercase tracking-[0.08em]';

export const ZoneHeader: React.FC<{
  label: string;
  /** What this zone IS, in plain words — rendered lowercase-secondary after the name. */
  explain?: string;
  /** Right-aligned content (counts, chips, a summary when collapsed). */
  right?: React.ReactNode;
  collapsed?: boolean;
  onToggle?: () => void;
  className?: string;
}> = ({ label, explain, right, collapsed, onToggle, className = '' }) => {
  const body = (
    <>
      <span className={`${ZONE_LABEL_CLASS} shrink-0 text-text-primary`}>{label}</span>
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
