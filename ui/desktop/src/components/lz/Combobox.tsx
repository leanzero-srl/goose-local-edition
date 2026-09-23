import {
  useEffect,
  useId,
  useMemo,
  useRef,
  useState,
  type KeyboardEvent,
  type ReactNode,
} from 'react';
import { ChevronDown } from 'lucide-react';
import { DISABLED, FOCUS, MOTION, RADIUS, ROW, SURFACE, TONE_FILL, cx } from './tokens';

export interface ComboboxOption {
  value: string;
  /** What the list shows; defaults to the value. */
  label?: string;
  /** A quieter right-aligned note on the option row (an offset, a count). */
  hint?: ReactNode;
}

export interface ComboboxProps {
  options: readonly ComboboxOption[];
  value: string;
  onChange: (value: string) => void;
  'aria-label': string;
  placeholder?: string;
  /**
   * Free text is the value: every keystroke calls `onChange` and the list only SUGGESTS — the
   * caller validates. Off: typing only filters, and leaving the field restores the chosen value.
   */
  allowFreeText?: boolean;
  disabled?: boolean;
  /** Shown in the list when the query matches nothing. */
  emptyText?: string;
  className?: string;
}

/**
 * A searchable dropdown on the Studio tokens — never a native `<select>`/`<datalist>`. The field
 * IS the search box (`role="combobox"`, `aria-autocomplete="list"`): focusing or clicking opens
 * the full list, typing filters it (a case-insensitive substring match on value and label),
 * ArrowUp/Down move, Enter picks, Escape closes, an outside press closes. The list renders
 * whatever the caller hands it — a caller with a very long vocabulary bounds it upstream.
 */
export function Combobox({
  options,
  value,
  onChange,
  'aria-label': ariaLabel,
  placeholder,
  allowFreeText = false,
  disabled = false,
  emptyText = 'No matches.',
  className,
}: ComboboxProps) {
  const [open, setOpen] = useState(false);
  const [text, setText] = useState(value);
  const [query, setQuery] = useState('');
  const [highlight, setHighlight] = useState(0);
  const rootRef = useRef<HTMLDivElement>(null);
  const listId = `${useId()}-list`;

  useEffect(() => {
    if (!open) setText(value);
  }, [value, open]);

  const matches = useMemo(() => {
    const q = query.trim().toLowerCase();
    if (!q) return options;
    return options.filter(
      (o) => o.value.toLowerCase().includes(q) || (o.label ?? '').toLowerCase().includes(q)
    );
  }, [options, query]);

  // Closing hands the field back to the committed value (the effect above) — for free text the
  // two are already equal, so a close never discards what was typed.
  const close = () => {
    setOpen(false);
    setQuery('');
  };

  useEffect(() => {
    if (!open) return undefined;
    const onDocMouseDown = (e: MouseEvent) => {
      if (rootRef.current && !rootRef.current.contains(e.target as Node)) {
        setOpen(false);
        setQuery('');
      }
    };
    document.addEventListener('mousedown', onDocMouseDown);
    return () => document.removeEventListener('mousedown', onDocMouseDown);
  }, [open]);

  const openList = () => {
    if (disabled || open) return;
    setOpen(true);
    setQuery('');
    setHighlight(
      Math.max(
        0,
        options.findIndex((o) => o.value === value)
      )
    );
  };

  const pick = (o: ComboboxOption) => {
    onChange(o.value);
    setText(o.value);
    setOpen(false);
    setQuery('');
  };

  const onKeyDown = (e: KeyboardEvent<HTMLInputElement>) => {
    if (e.key === 'Escape') {
      if (open) {
        e.stopPropagation();
        close();
      }
      return;
    }
    if (e.key === 'ArrowDown' || e.key === 'ArrowUp') {
      e.preventDefault();
      if (!open) {
        openList();
        return;
      }
      setHighlight((h) =>
        e.key === 'ArrowDown' ? Math.min(h + 1, matches.length - 1) : Math.max(0, h - 1)
      );
      return;
    }
    if (e.key === 'Enter' && open) {
      e.preventDefault();
      const o = matches[highlight];
      if (o) pick(o);
      else close();
    }
  };

  const activeId = open && matches[highlight] ? `${listId}-${highlight}` : undefined;

  return (
    <div ref={rootRef} className={cx('relative', className)}>
      <div className="relative">
        <input
          type="text"
          role="combobox"
          aria-label={ariaLabel}
          aria-expanded={open}
          aria-controls={listId}
          aria-autocomplete="list"
          aria-activedescendant={activeId}
          autoComplete="off"
          spellCheck={false}
          disabled={disabled}
          value={text}
          placeholder={placeholder}
          onFocus={openList}
          onClick={openList}
          onKeyDown={onKeyDown}
          onChange={(e) => {
            const next = e.target.value;
            setText(next);
            setQuery(next);
            setHighlight(0);
            if (!open) setOpen(true);
            if (allowFreeText) onChange(next);
          }}
          className={cx(
            'h-8 w-full bg-lz-surface pl-2.5 pr-8 text-lz-body text-lz-ink placeholder:text-lz-ink-4',
            SURFACE.outline,
            RADIUS.control,
            DISABLED,
            FOCUS,
            MOTION
          )}
        />
        <ChevronDown
          aria-hidden
          className="pointer-events-none absolute right-2.5 top-1/2 size-4 -translate-y-1/2 text-lz-ink-3"
        />
      </div>
      {open && (
        <div
          id={listId}
          role="listbox"
          aria-label={ariaLabel}
          className={cx(
            'absolute left-0 top-full z-[60] mt-1 max-h-64 w-full min-w-[220px] overflow-y-auto p-1',
            SURFACE.overlay
          )}
        >
          {matches.map((o, i) => {
            const selected = o.value === value;
            return (
              <button
                key={o.value}
                id={`${listId}-${i}`}
                type="button"
                role="option"
                aria-selected={selected}
                tabIndex={-1}
                onMouseDown={(e) => e.preventDefault()}
                onMouseEnter={() => setHighlight(i)}
                onClick={() => pick(o)}
                className={cx(
                  'flex w-full items-center gap-2 px-2.5 text-left text-lz-body',
                  ROW.dense,
                  RADIUS.control,
                  selected
                    ? TONE_FILL.accent
                    : cx('text-lz-ink', i === highlight && 'bg-lz-surface-2', SURFACE.hover),
                  MOTION
                )}
              >
                <span className="min-w-0 flex-1 truncate">{o.label ?? o.value}</span>
                {o.hint != null && (
                  <span
                    className={cx(
                      'shrink-0 text-lz-meta',
                      selected ? 'text-lz-accent-ink' : 'text-lz-ink-3'
                    )}
                  >
                    {o.hint}
                  </span>
                )}
              </button>
            );
          })}
          {matches.length === 0 && (
            <div className={cx('flex items-center px-2.5 text-lz-body text-lz-ink-3', ROW.dense)}>
              {emptyText}
            </div>
          )}
        </div>
      )}
    </div>
  );
}
