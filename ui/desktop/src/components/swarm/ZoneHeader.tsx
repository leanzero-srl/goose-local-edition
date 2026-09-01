import React from 'react';
import { ChevronDown, ChevronRight } from 'lucide-react';
import { MOTION, SURFACE, SectionHeader, TNUM, cx } from '../lz';

/**
 * The ONE header register every zone of the swarm run view uses — the Studio SectionHeader (the
 * `zone` type step: 11px, 600, uppercase, +0.08em) plus a plain-language explainer of what the zone
 * IS. Mihai's critique of the old view was that its parts "float" with no visual definition or
 * explanation; this applies the same treatment to every zone (run header, planning, fleet, work,
 * event log) AND to the benchmark chrome around the panel, so "which is which" is never ambiguous.
 *
 * A zone is NOT a node: it carries no hue. Hierarchy comes from type weight and case alone — never a
 * coloured square, never a tint, never a left rail. The optional `count` is the SectionHeader's count
 * pill and MUST be the length of the list the zone body renders (a header counts what the body shows).
 */

/** The zone-header scale — the ONLY place uppercase + tracking is used (the Studio `zone` step). */
export const ZONE_LABEL_CLASS = 'text-lz-zone uppercase';

export const ZoneHeader: React.FC<{
  label: string;
  /** What this zone IS, in plain words — rendered normal-case meta after the name. */
  explain?: string;
  /** The count pill — the number of rows the body renders. */
  count?: number;
  /** Right-aligned content (counts, chips, a summary when collapsed). */
  right?: React.ReactNode;
  collapsed?: boolean;
  onToggle?: () => void;
  className?: string;
}> = ({ label, explain, count, right, collapsed, onToggle, className = '' }) => {
  const header = (
    <SectionHeader
      title={
        <>
          <span className="shrink-0">{label}</span>
          {explain ? (
            <span className="ml-2 normal-case tracking-normal text-lz-meta text-lz-ink-3">
              — {explain}
            </span>
          ) : null}
        </>
      }
      count={count}
      right={
        right != null || onToggle ? (
          <>
            {right}
            {onToggle ? (
              collapsed ? (
                <ChevronRight className="size-3 shrink-0 text-lz-ink-3" aria-hidden />
              ) : (
                <ChevronDown className="size-3 shrink-0 text-lz-ink-3" aria-hidden />
              )
            ) : null}
          </>
        ) : undefined
      }
      className={cx('w-full min-w-0 [&>h2]:min-w-0 [&>h2]:truncate', TNUM)}
    />
  );
  return onToggle ? (
    <button
      type="button"
      onClick={onToggle}
      aria-expanded={!collapsed}
      className={cx('flex w-full items-center px-3 text-left', SURFACE.hover, MOTION, className)}
    >
      {header}
    </button>
  ) : (
    <div className={cx('flex items-center px-3', className)}>{header}</div>
  );
};

export default ZoneHeader;
