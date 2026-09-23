import { useId, useState, type ReactNode } from 'react';
import { ChevronRight } from 'lucide-react';
import { FOCUS, MOTION, RADIUS, SURFACE, cx } from './tokens';

export interface DisclosureProps {
  /** The always-visible summary line. */
  title: ReactNode;
  /** A right-aligned slot on the summary row (a count, a chip). Outside the toggle button. */
  meta?: ReactNode;
  children: ReactNode;
  /** Uncontrolled start state. */
  defaultOpen?: boolean;
  /** Controlled state — pass with `onOpenChange`. */
  open?: boolean;
  onOpenChange?: (open: boolean) => void;
  /** `card`: a bordered panel whose summary is the header. `plain`: the summary row alone. */
  variant?: 'card' | 'plain';
  className?: string;
  testId?: string;
}

/**
 * Show/hide on the Studio tokens — never the native `<details>`/`<summary>`, whose triangle the
 * app cannot style. The summary is a button carrying `aria-expanded` + `aria-controls`; a chevron
 * points right when closed and down when open. The body stays MOUNTED but `hidden` while closed
 * (the `<details>` contract), so controlled fields inside keep their DOM and their labels.
 */
export function Disclosure({
  title,
  meta,
  children,
  defaultOpen = false,
  open: openProp,
  onOpenChange,
  variant = 'card',
  className,
  testId,
}: DisclosureProps) {
  const [openState, setOpenState] = useState(defaultOpen);
  const open = openProp ?? openState;
  const id = useId();
  const bodyId = `${id}-body`;
  const toggle = () => {
    const next = !open;
    if (openProp == null) setOpenState(next);
    onOpenChange?.(next);
  };
  const card = variant === 'card';
  return (
    <div
      data-testid={testId ?? 'lz-disclosure'}
      data-state={open ? 'open' : 'closed'}
      className={cx(card && cx(SURFACE.card, 'overflow-hidden'), className)}
    >
      <div className={cx('flex items-center gap-2', card && 'pr-3')}>
        <button
          type="button"
          aria-expanded={open}
          aria-controls={bodyId}
          onClick={toggle}
          className={cx(
            'flex min-w-0 flex-1 items-center gap-2 text-left text-lz-body text-lz-ink font-lz-semibold [&_svg]:size-4 [&_svg]:shrink-0',
            card ? 'h-11 px-4' : cx('h-8 px-1', RADIUS.control),
            SURFACE.hover,
            FOCUS,
            MOTION
          )}
        >
          <ChevronRight aria-hidden className={cx('text-lz-ink-2', open && 'rotate-90')} />
          <span className="min-w-0 truncate">{title}</span>
        </button>
        {meta != null && <div className="flex shrink-0 items-center gap-2">{meta}</div>}
      </div>
      <div
        id={bodyId}
        hidden={!open}
        className={cx(card ? cx('border-t p-4', SURFACE.hairline) : 'pt-2')}
      >
        {children}
      </div>
    </div>
  );
}
