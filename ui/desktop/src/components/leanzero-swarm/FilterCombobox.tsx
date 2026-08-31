import React, { useEffect, useMemo, useRef, useState } from 'react';
import { ChevronDown, X } from 'lucide-react';

/**
 * Custom type-ahead filter combobox in the benchmark register — never a native <select>.
 * Opening focuses a text input; typing filters the vocabulary client-side with the
 * backend's frequency order preserved within matches; Enter/click selects; free text
 * beyond the vocabulary applies as-is (the backend accepts unknown values and errors
 * loudly on malformed ones — that error surfaces on the browse banner, not here).
 * A selected value renders as a solid chip with an ✕ to clear.
 */
export function FilterCombobox({
  label,
  value,
  options,
  onChange,
  chipColor,
  chipInk = '#ffffff',
  disabled,
}: {
  label: string;
  /** The applied filter value, or null for "all". */
  value: string | null;
  /** Frequency-ordered vocabulary from the backend. */
  options: string[];
  onChange: (value: string | null) => void;
  chipColor: string;
  chipInk?: string;
  disabled?: boolean;
}) {
  const [open, setOpen] = useState(false);
  const [text, setText] = useState('');
  const [highlight, setHighlight] = useState(0);
  const rootRef = useRef<HTMLDivElement>(null);
  const inputRef = useRef<HTMLInputElement>(null);

  const query = text.trim().toLowerCase();
  const matches = useMemo(
    () => (query === '' ? options : options.filter((o) => o.toLowerCase().includes(query))),
    [options, query]
  );
  const freeText = text.trim();
  const offerFreeText =
    freeText !== '' && !matches.some((m) => m.toLowerCase() === freeText.toLowerCase());
  const itemCount = matches.length + (offerFreeText ? 1 : 0);

  useEffect(() => {
    if (open) {
      setText('');
      setHighlight(0);
      // Focus after the panel exists in the DOM.
      const id = window.setTimeout(() => inputRef.current?.focus(), 0);
      return () => window.clearTimeout(id);
    }
    return undefined;
  }, [open]);

  useEffect(() => {
    if (!open) return undefined;
    const onDocMouseDown = (e: MouseEvent) => {
      if (rootRef.current && !rootRef.current.contains(e.target as Node)) setOpen(false);
    };
    document.addEventListener('mousedown', onDocMouseDown);
    return () => document.removeEventListener('mousedown', onDocMouseDown);
  }, [open]);

  const pick = (v: string) => {
    onChange(v);
    setOpen(false);
  };

  const onKeyDown = (e: React.KeyboardEvent) => {
    if (e.key === 'Escape') {
      e.stopPropagation();
      setOpen(false);
      return;
    }
    if (e.key === 'ArrowDown') {
      e.preventDefault();
      setHighlight((h) => Math.min(h + 1, Math.max(0, itemCount - 1)));
      return;
    }
    if (e.key === 'ArrowUp') {
      e.preventDefault();
      setHighlight((h) => Math.max(0, h - 1));
      return;
    }
    if (e.key === 'Enter') {
      e.preventDefault();
      if (itemCount === 0) return;
      const idx = Math.min(highlight, itemCount - 1);
      if (idx < matches.length) pick(matches[idx]);
      else pick(freeText);
    }
  };

  return (
    <div ref={rootRef} className="relative">
      {value != null ? (
        <span
          className="inline-flex items-center gap-1.5 rounded px-2 py-1 text-xs font-bold"
          style={{ backgroundColor: chipColor, color: chipInk }}
        >
          <button
            type="button"
            onClick={() => setOpen((o) => !o)}
            className="uppercase tracking-wide hover:opacity-90"
            aria-label={`${label} filter`}
            aria-expanded={open}
            title={`${label}: ${value} — click to change`}
            style={{ color: chipInk }}
          >
            {label}: {value}
          </button>
          <button
            type="button"
            onClick={() => {
              onChange(null);
              setOpen(false);
            }}
            aria-label={`Clear ${label} filter`}
            className="rounded hover:opacity-80"
            style={{ color: chipInk }}
          >
            <X className="h-3 w-3" />
          </button>
        </span>
      ) : (
        <button
          type="button"
          onClick={() => setOpen((o) => !o)}
          disabled={disabled}
          aria-label={`${label} filter`}
          aria-expanded={open}
          className="flex items-center gap-1.5 rounded border border-border-primary bg-background-secondary px-2.5 py-1.5 text-sm font-semibold text-text-secondary transition-colors hover:text-text-primary disabled:opacity-60"
        >
          {label}: all
          <ChevronDown className="h-3.5 w-3.5" />
        </button>
      )}
      {open && (
        <div className="absolute left-0 top-full z-[60] mt-1 w-64 overflow-hidden rounded border border-border-primary bg-background-primary shadow-lg">
          <input
            ref={inputRef}
            value={text}
            onChange={(e) => {
              setText(e.target.value);
              setHighlight(0);
            }}
            onKeyDown={onKeyDown}
            placeholder={`Type to filter ${options.length} ${label.toLowerCase()}s…`}
            aria-label={`Search ${label}`}
            className="w-full border-b border-border-primary bg-background-primary px-3 py-2 font-mono text-sm text-text-primary outline-none placeholder:text-text-secondary"
          />
          <div role="listbox" aria-label={`${label} options`} className="max-h-60 overflow-y-auto py-1">
            {matches.map((option, i) => (
              <button
                key={option}
                type="button"
                role="option"
                aria-selected={option === value}
                onClick={() => pick(option)}
                onMouseEnter={() => setHighlight(i)}
                className={`block w-full px-3 py-1.5 text-left font-mono text-sm ${
                  i === highlight
                    ? 'bg-background-secondary text-text-primary'
                    : 'text-text-primary hover:bg-background-secondary'
                }`}
              >
                {option}
              </button>
            ))}
            {offerFreeText && (
              <button
                type="button"
                role="option"
                aria-selected={false}
                onClick={() => pick(freeText)}
                onMouseEnter={() => setHighlight(matches.length)}
                className={`block w-full px-3 py-1.5 text-left text-sm font-semibold ${
                  highlight === matches.length
                    ? 'bg-background-secondary text-text-primary'
                    : 'text-text-primary hover:bg-background-secondary'
                }`}
              >
                Use “{freeText}”
              </button>
            )}
            {itemCount === 0 && (
              <div className="px-3 py-1.5 text-sm text-text-secondary">No matches.</div>
            )}
          </div>
        </div>
      )}
    </div>
  );
}
