import React, { useEffect, useState, type ReactNode } from 'react';
import { createPortal } from 'react-dom';
import { Check } from 'lucide-react';
import { FOCUS, MOTION, RADIUS, ROW, SURFACE, TONE_FILL, TYPE, cx } from '../lz';

/**
 * The sidebar tree register shared by every tree in the navigation — Projects, Agent Work and
 * Benchmark are ONE shape (Mihai 2026-09-22: "agents should expand same as projects and be
 * fashioned exactly like projects and have sessions under"). One dense 32px row for parents and
 * leaves alike; hover is a solid step to surface-2; the current item is a 2px inset accent ring.
 */
export const treeRowClass = cx(
  'flex w-full items-center gap-2 px-2 text-left',
  ROW.dense,
  RADIUS.control,
  MOTION,
  FOCUS
);
export const treeParentClass = cx(treeRowClass, 'min-w-0 flex-1', SURFACE.hover);
export const treeStateRowClass = cx('flex items-center px-2', ROW.dense, TYPE.meta);

/** How many children a tree row previews before "Show more" — the sidebar's density, never a cap. */
export const TREE_PREVIEW_COUNT = 5;

/**
 * Children of an expanded row sit beside a 1px hairline guide (bg-lz-border, structural — it is a
 * separate element under the parent's chevron, never a border-left on the rows).
 */
export const TreeChildren: React.FC<{ children: ReactNode }> = ({ children }) => (
  <div className="flex">
    <span
      aria-hidden
      data-testid="tree-guide"
      className={cx('ml-4 w-px shrink-0 self-stretch', 'bg-lz-border')}
    />
    <div className="flex min-w-0 flex-1 flex-col gap-px pl-2">{children}</div>
  </div>
);

/**
 * A section title that opens its view — Agent Work and Benchmark have no pinned nav row (owner
 * 2026-09-23: the row and the section were two doors to one view), so the title is the door.
 * While the view is open the title carries the accent ink and aria-current.
 */
export const SectionTitleLink: React.FC<{
  label: string;
  active: boolean;
  onClick: () => void;
  testId: string;
}> = ({ label, active, onClick, testId }) => (
  <button
    type="button"
    onClick={onClick}
    aria-current={active ? 'page' : undefined}
    data-testid={testId}
    className={cx(
      'no-drag uppercase',
      RADIUS.control,
      FOCUS,
      MOTION,
      active ? 'text-lz-accent' : 'hover:text-lz-ink'
    )}
  >
    {label}
  </button>
);

/** Row actions stay out of the way until the row is hovered or holds focus. Visibility, not opacity
 *  (opacity utilities are banned as faded colour); group-focus-within keeps them reachable by
 *  keyboard. */
export const rowActionClass = (shown: boolean) =>
  shown ? 'visible' : 'invisible group-hover:visible group-focus-within:visible';

/** Compact "time since". */
export function timeAgo(iso: string | undefined): string {
  if (!iso) return '';
  const t = Date.parse(iso);
  if (Number.isNaN(t)) return '';
  const s = Math.max(0, Math.round((Date.now() - t) / 1000));
  if (s < 45) return 'now';
  const m = Math.round(s / 60);
  if (m < 60) return `${m}m ago`;
  const h = Math.round(m / 60);
  if (h < 24) return `${h}h ago`;
  const d = Math.round(h / 24);
  if (d < 7) return `${d}d ago`;
  const w = Math.round(d / 7);
  return w < 5 ? `${w}w ago` : `${Math.round(d / 30)}mo ago`;
}

export interface TreeMenuItem {
  key: string;
  label: string;
  icon?: ReactNode;
  onClick: () => void;
  /** A destructive item reads in the err tone and confirms in-menu with `confirmLabel` before it
   *  fires — a single click can never drop anything. */
  danger?: boolean;
  confirmLabel?: string;
  /** Draw a hairline above this item. */
  separator?: boolean;
  disabled?: boolean;
  title?: string;
}

/**
 * The app's own context menu — portaled on the one overlay elevation, never a native menu. Opens
 * where the pointer was (or under a "…" button), closes on Escape, outside click or another
 * right-click. Every sidebar tree and every row that has actions uses this one component.
 */
export const TreeContextMenu: React.FC<{
  x: number;
  y: number;
  items: TreeMenuItem[];
  onClose: () => void;
  testId?: string;
}> = ({ x, y, items, onClose, testId = 'tree-context-menu' }) => {
  const [confirming, setConfirming] = useState<string | null>(null);
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (e.key === 'Escape') onClose();
    };
    window.addEventListener('keydown', onKey);
    return () => window.removeEventListener('keydown', onKey);
  }, [onClose]);

  const MENU_W = 240;
  const left = Math.min(x, window.innerWidth - MENU_W - 8);
  const top = Math.min(y, Math.max(8, window.innerHeight - 40 * items.length - 24));
  const itemBase = cx(
    'flex w-full items-center gap-2 px-3 text-left text-lz-body',
    ROW.dense,
    MOTION,
    FOCUS
  );

  return createPortal(
    <>
      <div
        className="fixed inset-0 z-[190]"
        onClick={onClose}
        onContextMenu={(e) => {
          e.preventDefault();
          onClose();
        }}
      />
      <div
        role="menu"
        data-testid={testId}
        className={cx('fixed z-[200] py-1', SURFACE.overlay)}
        style={{ left, top, minWidth: MENU_W }}
        onClick={(e) => e.stopPropagation()}
      >
        {items.map((item) => (
          <React.Fragment key={item.key}>
            {item.separator && <div className={cx('my-1 border-t', SURFACE.hairline)} />}
            {item.danger && item.confirmLabel && confirming === item.key ? (
              <button
                role="menuitem"
                className={cx(itemBase, TONE_FILL.err, 'hover:bg-lz-err')}
                onClick={item.onClick}
              >
                <Check className="size-3.5" strokeWidth={3} /> {item.confirmLabel}
              </button>
            ) : (
              <button
                role="menuitem"
                disabled={item.disabled}
                title={item.title}
                className={cx(
                  itemBase,
                  item.danger ? 'text-lz-err' : 'text-lz-ink',
                  SURFACE.hover,
                  item.disabled && 'text-lz-ink-3'
                )}
                onClick={() =>
                  item.danger && item.confirmLabel ? setConfirming(item.key) : item.onClick()
                }
              >
                {item.icon && <span className="[&>svg]:size-3.5 text-lz-ink-3">{item.icon}</span>}
                {item.label}
              </button>
            )}
          </React.Fragment>
        ))}
      </div>
    </>,
    document.body
  );
};
