import React, { useCallback, useEffect, useId, useLayoutEffect, useRef, useState } from 'react';
import { createPortal } from 'react-dom';
import { Check, Copy, Maximize2, X } from 'lucide-react';
import { Button, Chip, FOCUS, MOTION, RADIUS, SURFACE, TYPE, cx } from '../lz';

/**
 * A CLIPPED TEXT IS AN AFFORDANCE, NEVER A DEAD END.
 *
 * Mihai, 2026-09-02: "wherever there's stuff like prompts and there is not enough space in the UI to make
 * room for it — for example in build phases — it's important to have a way to click on that element and
 * bring it up so we can see it." Every row in the swarm panel clips something — a 400-char task brief
 * behind a 90-char summary, the durable log line a fleet cell shows two lines of, an event's sub-detail
 * behind `truncate` — and until this module the only way to read it was to know which row expands inline
 * and which does not.
 *
 * Three pieces, one overlay:
 *  - `Clipped`  — a span that MEASURES its own overflow (or knows its text is a derivation of a longer
 *                 one) and, when clipped, becomes a button: hover title with the full text, an expand glyph
 *                 at the end of the line, click / Enter / Space opens the reveal. Unclipped text renders as
 *                 a plain span, so a short row gains no chrome.
 *  - `RevealGlyph` — the same door for a BLOCK the caller clips itself (a clamped live-generation cell).
 *  - `RevealDialog` — the overlay: the full text in a scrollable body (mono for machine text), a Copy
 *                 button, and the element's context (task id, phase, node) as chips. Escape closes ONLY the
 *                 reveal — it is captured on document ahead of the node inspector's window listener, so a
 *                 reveal stacked over the inspector never takes the inspector down with it. Focus returns
 *                 to the element that opened it.
 */

export interface RevealFact {
  label: string;
  value: string;
}

export interface RevealSpec {
  /** What the dialog is titled — "Task brief", "Working on", "Prompt". */
  label: string;
  /** The whole text, exactly as the element holds it. */
  text: string;
  /** Where the text came from — task id, phase, node — rendered as chips in the dialog header. */
  context?: RevealFact[];
  /** Machine text (a log line, a path, a command) keeps its monospace and alignment. */
  mono?: boolean;
}

/**
 * True when the element's content overflows the box the row gave it — `truncate` (scrollWidth) and a
 * line-clamp (scrollHeight) both report through the same two numbers. Re-measured when the text changes
 * and, where the platform has one, whenever the box resizes. A ±1px tolerance absorbs sub-pixel layout.
 */
export function useOverflow(ref: React.RefObject<HTMLElement | null>, text: string): boolean {
  const [over, setOver] = useState(false);
  useLayoutEffect(() => {
    const el = ref.current;
    if (!el) return;
    const measure = () => {
      const o = el.scrollWidth > el.clientWidth + 1 || el.scrollHeight > el.clientHeight + 1;
      setOver((prev) => (prev === o ? prev : o));
    };
    measure();
    if (typeof ResizeObserver === 'undefined') return;
    const ro = new ResizeObserver(measure);
    ro.observe(el);
    return () => ro.disconnect();
  }, [ref, text]);
  return over;
}

export const RevealDialog: React.FC<RevealSpec & { onClose: () => void }> = ({
  label,
  text,
  context,
  mono,
  onClose,
}) => {
  const [copied, setCopied] = useState(false);
  const closeRef = useRef<HTMLButtonElement>(null);
  const titleId = useId();

  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (e.key !== 'Escape') return;
      e.stopPropagation();
      e.preventDefault();
      onClose();
    };
    document.addEventListener('keydown', onKey, true);
    return () => document.removeEventListener('keydown', onKey, true);
  }, [onClose]);

  useEffect(() => {
    closeRef.current?.focus();
  }, []);

  useEffect(() => {
    if (!copied) return;
    const t = setTimeout(() => setCopied(false), 1200);
    return () => clearTimeout(t);
  }, [copied]);

  return createPortal(
    <>
      <div
        className="fixed inset-0 z-[60] bg-black/60"
        data-testid="reveal-backdrop"
        onClick={(e) => {
          e.stopPropagation();
          onClose();
        }}
      />
      <div
        role="dialog"
        aria-modal="true"
        aria-labelledby={titleId}
        data-testid="reveal-dialog"
        className={cx(
          'fixed inset-x-4 top-[8vh] z-[70] mx-auto flex max-h-[84vh] max-w-[56rem] flex-col text-lz-body',
          SURFACE.overlay
        )}
        onClick={(e) => e.stopPropagation()}
      >
        <div className="flex h-12 shrink-0 items-center gap-2 border-b border-lz-border px-4">
          <span id={titleId} className={cx(TYPE.h2, 'shrink-0')}>
            {label}
          </span>
          <span className="flex min-w-0 flex-1 flex-wrap items-center gap-1.5">
            {(context ?? []).map((f) => (
              <Chip key={f.label} title={`${f.label}: ${f.value}`}>
                <span className="text-lz-ink-3">{f.label}</span> {f.value}
              </Chip>
            ))}
          </span>
          <Button
            variant="secondary"
            size="sm"
            data-testid="reveal-copy"
            icon={copied ? <Check /> : <Copy />}
            onClick={() => {
              void navigator.clipboard?.writeText(text);
              setCopied(true);
            }}
          >
            {copied ? 'Copied' : 'Copy'}
          </Button>
          <Button
            ref={closeRef}
            variant="ghost"
            size="sm"
            iconOnly
            data-testid="reveal-close"
            onClick={onClose}
            aria-label="Close"
            icon={<X />}
          />
        </div>
        <div
          data-testid="reveal-body"
          className={cx(
            'min-h-0 flex-1 overflow-y-auto whitespace-pre-wrap break-words px-4 py-3 text-lz-ink',
            mono ? 'font-mono text-lz-mono' : 'text-lz-body'
          )}
          style={{ lineHeight: 1.65 }}
        >
          {text}
        </div>
      </div>
    </>,
    document.body
  );
};

/** The one control class every reveal opener shares: pointer, focus ring, the glyph brightening on hover. */
const OPENER = cx('group cursor-pointer', FOCUS, MOTION);
const GLYPH = cx('size-3 shrink-0 text-lz-ink-3 group-hover:text-lz-ink', MOTION);

export const Clipped: React.FC<{
  /** The text the row shows. */
  text: string;
  /** The whole text the row cannot show; defaults to `text`. A summary derived from a longer brief passes the brief. */
  full?: string;
  /** What the reveal is titled — "Task brief", "Working on", "Prompt". */
  label: string;
  context?: RevealFact[];
  mono?: boolean;
  /** Rendered inside the clipped span before the text (the "· " separators the rows use). */
  prefix?: React.ReactNode;
  /** The clip the row applies: `truncate` (default) or a line-clamp class set. */
  clamp?: string;
  /** Sizing and colour for the control — the span's old classes minus the clip itself. The control is a
   *  flex item with min-w-0; a row bounds it, a `max-w-*` from the caller bounds it further. */
  className?: string;
  /** False when the row already shows a styled tooltip with the full text (a native title on top would double it). */
  hoverTitle?: boolean;
  /** 'start' for a multi-line clamp, so the glyph sits on the first line rather than mid-block. */
  align?: 'center' | 'start';
  testId?: string;
}> = ({ text, full, label, context, mono, prefix, clamp, className, hoverTitle = true, align = 'center', testId }) => {
  const [open, setOpen] = useState(false);
  const spanRef = useRef<HTMLSpanElement>(null);
  const openerRef = useRef<HTMLElement | null>(null);
  const fullText = full ?? text;
  const clipped = useOverflow(spanRef, text) || fullText !== text;

  const openReveal = (e: React.SyntheticEvent<HTMLElement>) => {
    e.stopPropagation();
    openerRef.current = e.currentTarget;
    setOpen(true);
  };
  const close = useCallback(() => {
    setOpen(false);
    openerRef.current?.focus();
  }, []);

  return (
    <>
      <span
        role={clipped ? 'button' : undefined}
        tabIndex={clipped ? 0 : undefined}
        aria-haspopup={clipped ? 'dialog' : undefined}
        title={clipped && hoverTitle ? fullText : undefined}
        data-testid={testId ?? 'clipped-text'}
        data-clipped={clipped ? 'true' : 'false'}
        className={cx(
          'inline-flex min-w-0 gap-1',
          align === 'start' ? 'items-start' : 'items-center',
          className,
          clipped && OPENER
        )}
        onClick={clipped ? openReveal : undefined}
        onKeyDown={
          clipped
            ? (e) => {
                if (e.key === 'Enter' || e.key === ' ') {
                  e.preventDefault();
                  openReveal(e);
                }
              }
            : undefined
        }
      >
        <span ref={spanRef} className={cx('min-w-0', clamp ?? 'truncate')}>
          {prefix}
          {text}
        </span>
        {clipped ? <Maximize2 className={cx(GLYPH, align === 'start' && 'mt-[3px]')} aria-hidden /> : null}
      </span>
      {open ? (
        <RevealDialog label={label} text={fullText} context={context} mono={mono} onClose={close} />
      ) : null}
    </>
  );
};

/**
 * The reveal door for a block the caller clips itself (a clamped live-generation cell): a small solid
 * control the caller places at the block's corner. Its click and its keys stop at the button, so a row
 * that opens the node inspector on click keeps doing that everywhere except on this glyph.
 */
export const RevealGlyph: React.FC<{ spec: RevealSpec; className?: string }> = ({ spec, className }) => {
  const [open, setOpen] = useState(false);
  const openerRef = useRef<HTMLButtonElement>(null);
  const close = useCallback(() => {
    setOpen(false);
    openerRef.current?.focus();
  }, []);
  return (
    <>
      <button
        ref={openerRef}
        type="button"
        title={`Show the full ${spec.label.toLowerCase()}`}
        aria-label={`Show the full ${spec.label.toLowerCase()}`}
        aria-haspopup="dialog"
        data-testid="reveal-glyph"
        className={cx(
          'inline-flex size-4 shrink-0 items-center justify-center border border-lz-border bg-lz-surface-2 text-lz-ink-3 hover:text-lz-ink',
          RADIUS.control,
          FOCUS,
          MOTION,
          className
        )}
        onClick={(e) => {
          e.stopPropagation();
          setOpen(true);
        }}
        onKeyDown={(e) => {
          if (e.key === 'Enter' || e.key === ' ') e.stopPropagation();
        }}
      >
        <Maximize2 className="size-3" aria-hidden />
      </button>
      {open ? <RevealDialog {...spec} onClose={close} /> : null}
    </>
  );
};
