import React, { useEffect, useMemo, useRef, useState } from 'react';
import { ChevronDown, X } from 'lucide-react';
import { DISABLED, FOCUS, MOTION, RADIUS, ROW, SURFACE, TONE_FILL, WEIGHT, cx } from '../lz';

/**
 * Custom type-ahead filter menu on the Studio tokens — never a native <select>. Opening
 * focuses a text input; typing filters the vocabulary client-side with the backend's frequency
 * order preserved within matches; Enter/click selects; free text beyond the vocabulary applies
 * as-is (the backend accepts unknown values and errors loudly on malformed ones — that error
 * surfaces on the browse banner, not here). A selected value renders as the accent chip with
 * an ✕ to clear: the applied filter is an ACTIVE state, which is what the accent means.
 */
export function FilterCombobox({
  label,
  value,
  options,
  onChange,
  disabled,
}: {
  label: string;
  /** The applied filter value, or null for "all". */
  value: string | null;
  /** Frequency-ordered vocabulary from the backend. */
  options: string[];
  onChange: (value: string | null) => void;
  /** Accepted for older callers; the applied chip is always the accent. */
  chipColor?: string;
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

  const optionClass = (active: boolean, selected: boolean) =>
    cx(
      'flex w-full items-center px-2.5 text-left',
      ROW.dense,
      RADIUS.control,
      selected ? TONE_FILL.accent : cx('text-lz-ink', active && 'bg-lz-surface-2', SURFACE.hover),
      MOTION
    );

  return (
    <div ref={rootRef} className="relative">
      {value != null ? (
        <span
          className={cx(
            'inline-flex h-7 items-center gap-1 whitespace-nowrap pl-2.5 pr-1 text-[12px]',
            WEIGHT.medium,
            RADIUS.control,
            TONE_FILL.accent
          )}
        >
          <button
            type="button"
            onClick={() => setOpen((o) => !o)}
            className={cx('whitespace-nowrap', FOCUS)}
            aria-label={`${label} filter`}
            aria-expanded={open}
            title={`${label}: ${value} — click to change`}
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
            className={cx(
              'flex size-5 items-center justify-center hover:bg-lz-accent-hover [&_svg]:size-3',
              RADIUS.control,
              FOCUS,
              MOTION
            )}
          >
            <X />
          </button>
        </span>
      ) : (
        <button
          type="button"
          onClick={() => setOpen((o) => !o)}
          disabled={disabled}
          aria-label={`${label} filter`}
          aria-expanded={open}
          className={cx(
            'inline-flex h-7 items-center gap-1.5 whitespace-nowrap bg-lz-surface px-2.5 text-[12px] text-lz-ink-2 hover:bg-lz-surface-2 hover:text-lz-ink [&_svg]:size-3.5',
            WEIGHT.medium,
            SURFACE.outline,
            RADIUS.control,
            DISABLED,
            FOCUS,
            MOTION
          )}
        >
          {label}: all
          <ChevronDown />
        </button>
      )}
      {open && (
        <div
          className={cx(
            'absolute left-0 top-full z-[60] mt-1 w-64 overflow-hidden',
            SURFACE.overlay
          )}
        >
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
            autoComplete="off"
            spellCheck={false}
            className={cx(
              'h-8 w-full border-b bg-lz-surface px-3 font-mono text-lz-mono text-lz-ink placeholder:text-lz-ink-4',
              SURFACE.hairline,
              FOCUS
            )}
          />
          <div
            role="listbox"
            aria-label={`${label} options`}
            className="max-h-60 overflow-y-auto p-1"
          >
            {matches.map((option, i) => (
              <button
                key={option}
                type="button"
                role="option"
                aria-selected={option === value}
                onClick={() => pick(option)}
                onMouseEnter={() => setHighlight(i)}
                className={cx(
                  'font-mono text-lz-mono',
                  optionClass(i === highlight, option === value)
                )}
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
                className={cx(
                  'text-lz-body',
                  WEIGHT.medium,
                  optionClass(highlight === matches.length, false)
                )}
              >
                Use “{freeText}”
              </button>
            )}
            {itemCount === 0 && (
              <div className={cx('flex items-center px-2.5 text-lz-body text-lz-ink-3', ROW.dense)}>
                No matches.
              </div>
            )}
          </div>
        </div>
      )}
    </div>
  );
}
