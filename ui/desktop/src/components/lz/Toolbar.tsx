import type { ReactNode, Ref } from 'react';
import { Search, X } from 'lucide-react';
import { FOCUS, MOTION, RADIUS, ROW, SURFACE, cx } from './tokens';

export interface ToolbarSearch {
  value: string;
  onChange: (value: string) => void;
  placeholder?: string;
  'aria-label': string;
  /** Take the row's free width instead of the fixed 240px — a search that heads a list column. */
  fill?: boolean;
  /** For a view that focuses the field from a shortcut (⌘F). */
  inputRef?: Ref<HTMLInputElement>;
}

export interface ToolbarProps {
  search?: ToolbarSearch;
  /** Filter controls (Segmented, Chips) after the search. */
  filters?: ReactNode;
  /** Right-aligned actions. */
  actions?: ReactNode;
  'aria-label'?: string;
  className?: string;
}

/** One 36px row: search, filters, actions. The search is a plain text input with its own clear button — never a native search field. */
export function Toolbar({
  search,
  filters,
  actions,
  'aria-label': ariaLabel,
  className,
}: ToolbarProps) {
  return (
    <div
      role="toolbar"
      aria-label={ariaLabel}
      data-testid="lz-toolbar"
      className={cx('flex items-center gap-2', ROW.default, className)}
    >
      {search && (
        <div className={cx('relative flex items-center', search.fill && 'min-w-0 flex-1')}>
          <Search
            aria-hidden
            className="pointer-events-none absolute left-2.5 size-3.5 text-lz-ink-3"
          />
          <input
            ref={search.inputRef}
            type="text"
            inputMode="search"
            autoComplete="off"
            spellCheck={false}
            value={search.value}
            onChange={(e) => search.onChange(e.target.value)}
            placeholder={search.placeholder}
            aria-label={search['aria-label']}
            className={cx(
              search.fill ? 'w-full' : 'w-60',
              'h-8 bg-lz-surface pl-8 pr-7 text-lz-body text-lz-ink placeholder:text-lz-ink-4',
              SURFACE.outline,
              RADIUS.control,
              FOCUS,
              MOTION
            )}
          />
          {search.value !== '' && (
            <button
              type="button"
              aria-label="Clear search"
              onClick={() => search.onChange('')}
              className={cx(
                'absolute right-1.5 flex size-5 items-center justify-center text-lz-ink-3 hover:bg-lz-surface-2 hover:text-lz-ink [&_svg]:size-3',
                RADIUS.control,
                FOCUS,
                MOTION
              )}
            >
              <X />
            </button>
          )}
        </div>
      )}
      {filters != null && <div className="flex items-center gap-2">{filters}</div>}
      {actions != null && <div className="ml-auto flex items-center gap-2">{actions}</div>}
    </div>
  );
}
