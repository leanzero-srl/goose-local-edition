import { useRef, type MouseEvent, type ReactNode } from 'react';
import { MainPanelLayout } from '../Layout/MainPanelLayout';
import { useFindShortcut } from '../../hooks/useFindShortcut';
import {
  FOCUS,
  MOTION,
  PageHeader,
  RADIUS,
  SURFACE,
  SectionHeader,
  Toolbar,
  WEIGHT,
  cx,
} from '../lz';

/**
 * The shell the Skills and Memories views share: a page header, a list column headed by a real
 * search field (⌘F focuses it), and the reading pane. ONE shell so the two views cannot drift —
 * they had drifted into the same two bugs at once: a title in no ink (inheriting a grey) and a
 * `block line-clamp-1` preview that never clamped (Tailwind emits `.block` AFTER `.line-clamp-*`,
 * so `display: block` beat `-webkit-box` and every description printed whole).
 */
export function LibraryShell({
  title,
  subtitle,
  search,
  list,
  detail,
  testId,
}: {
  title: string;
  subtitle: ReactNode;
  search: { value: string; onChange: (value: string) => void; placeholder: string; label: string };
  list: ReactNode;
  detail: ReactNode;
  testId: string;
}) {
  const searchRef = useRef<HTMLInputElement>(null);
  useFindShortcut(searchRef);
  return (
    <MainPanelLayout>
      <div className={cx('flex min-h-0 flex-1 flex-col', SURFACE.page)} data-testid={testId}>
        <div className={cx('border-b px-lz-page pb-4 pt-5', SURFACE.hairline)}>
          <PageHeader className="page-transition" title={title} subtitle={subtitle} />
        </div>
        <div className="flex min-h-0 flex-1">
          <div
            className={cx(
              'flex w-[400px] min-w-[320px] shrink-0 flex-col border-r',
              SURFACE.hairline
            )}
            data-testid="library-list-column"
          >
            <Toolbar
              className="shrink-0 px-4 pt-3"
              aria-label={search.label}
              search={{
                value: search.value,
                onChange: search.onChange,
                placeholder: search.placeholder,
                'aria-label': search.label,
                fill: true,
                inputRef: searchRef,
              }}
            />
            <div className="min-h-0 flex-1 overflow-y-auto px-2 pb-4 pt-2">{list}</div>
          </div>
          <div className="min-h-0 min-w-0 flex-1" data-testid="library-detail">
            {detail}
          </div>
        </div>
      </div>
    </MainPanelLayout>
  );
}

/** A group of rows under the zone register; the count is the number of rows the group shows. */
export function LibraryGroup({
  title,
  count,
  children,
}: {
  title: string;
  count: number;
  children: ReactNode;
}) {
  return (
    <section className="mb-3">
      <SectionHeader title={title} count={count} as="h2" className="px-2" />
      <div className="flex flex-col gap-px">{children}</div>
    </section>
  );
}

/**
 * One list row: the name on the first line (with an optional plain-text label at the right — a
 * word, never a coloured square), a two-line preview under it. The full text belongs to the
 * reading pane. Selected = the accent fill with accent ink and the accent-hover step.
 */
export function LibraryRow({
  title,
  label,
  preview,
  selected,
  onSelect,
  onContextMenu,
  testId,
}: {
  title: string;
  label?: string;
  preview: string;
  selected: boolean;
  onSelect: () => void;
  onContextMenu?: (e: MouseEvent) => void;
  testId: string;
}) {
  const muted = selected ? 'text-lz-accent-ink' : 'text-lz-ink-2';
  return (
    <button
      type="button"
      onClick={onSelect}
      onContextMenu={onContextMenu}
      aria-current={selected ? 'true' : undefined}
      data-testid={testId}
      className={cx(
        'flex w-full min-w-0 flex-col gap-0.5 px-3 py-2 text-left',
        RADIUS.control,
        FOCUS,
        MOTION,
        selected ? cx(SURFACE.selected, SURFACE.selectedHover) : cx('text-lz-ink', SURFACE.hover)
      )}
    >
      <span className="flex w-full min-w-0 items-baseline gap-2">
        <span className={cx('min-w-0 flex-1 truncate text-lz-body', WEIGHT.medium)}>{title}</span>
        {label && (
          <span
            data-testid="library-row-label"
            className={cx(
              'shrink-0 text-lz-meta',
              selected ? 'text-lz-accent-ink' : 'text-lz-ink-3'
            )}
          >
            {label}
          </span>
        )}
      </span>
      {preview && (
        <span data-testid="library-row-preview" className={cx('line-clamp-2 text-lz-meta', muted)}>
          {preview}
        </span>
      )}
    </button>
  );
}

/**
 * The item the reading pane shows: the chosen one while it is still in the list, otherwise the
 * first row the list shows — so the page never opens on an empty pane, and a search that hides
 * the chosen item moves the pane to what is visible instead of showing a stale selection.
 */
export function shownSelection<T>(
  visible: readonly T[],
  chosen: string | null,
  idOf: (item: T) => string
): T | null {
  if (chosen != null) {
    const hit = visible.find((item) => idOf(item) === chosen);
    if (hit) return hit;
  }
  return visible[0] ?? null;
}
