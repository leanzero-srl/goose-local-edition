import { useEffect, useId, useState, type ReactNode } from 'react';
import { ChevronRight } from 'lucide-react';

export function ActivityDisclosure({
  label,
  children,
  isStartExpanded = false,
  isForceExpand,
  expandToken = 0,
  className = '',
}: {
  label: ReactNode;
  children: ReactNode;
  isStartExpanded?: boolean;
  isForceExpand?: boolean;
  /** Each new value opens the disclosure again, even after the person closed it (a "show me" jump). */
  expandToken?: number;
  className?: string;
}) {
  const [choice, setChoice] = useState<boolean | null>(null);
  const expanded = choice ?? isStartExpanded;
  const id = useId();
  useEffect(() => {
    if (isForceExpand) setChoice(true);
  }, [isForceExpand]);
  useEffect(() => {
    if (expandToken > 0) setChoice(true);
  }, [expandToken]);
  return (
    <div className={`min-w-0 ${className}`}>
      <button
        type="button"
        aria-expanded={expanded}
        aria-controls={id}
        onClick={() => setChoice(!expanded)}
        className="flex w-full min-w-0 items-center gap-3 rounded-lg px-3 py-2.5 text-left text-sm hover:bg-background-secondary focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-lz-accent motion-safe:transition-colors"
      >
        <ChevronRight
          aria-hidden
          size={15}
          className={`shrink-0 motion-safe:transition-transform ${expanded ? 'rotate-90' : ''}`}
        />
        <span className="min-w-0 flex-1">{label}</span>
      </button>
      {expanded && (
        <div id={id} className="min-w-0 overflow-hidden">
          {children}
        </div>
      )}
    </div>
  );
}
