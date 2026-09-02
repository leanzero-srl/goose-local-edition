import {
  Fragment,
  type HTMLAttributes,
  type KeyboardEvent,
  type MouseEvent,
  type ReactNode,
} from 'react';
import { MOTION, ROW, SURFACE, TNUM, cx } from './tokens';

export interface DataTableColumn<T> {
  key: string;
  header: ReactNode;
  cell: (row: T) => ReactNode;
  align?: 'left' | 'right' | 'center';
  width?: number | string;
  /** Right-aligned tabular figures — every number a person compares down the column. */
  numeric?: boolean;
  className?: string;
}

/**
 * What `rowProps` may put on a row's `<tr>`: native attributes plus any `data-*` hook. Typed on
 * HTMLElement like the table's own handlers (the app's eslint globals stop at HTMLDivElement).
 */
export type DataTableRowProps = HTMLAttributes<HTMLElement> & {
  [attribute: `data-${string}`]: string | number | boolean | undefined;
};

export interface DataTableProps<T> {
  columns: readonly DataTableColumn<T>[];
  rows: readonly T[];
  /** Stable identity per row — never an index. */
  rowKey: (row: T) => string;
  /** 32px rows instead of 36px. */
  dense?: boolean;
  /** The selected row is the accent fill with white ink. */
  selectedKey?: string | null;
  onRowClick?: (row: T) => void;
  /** A trailing per-row slot (a ghost Button, a menu trigger). */
  rowAction?: (row: T) => ReactNode;
  /**
   * Extra attributes for a row's `<tr>` — `data-*` hooks, a `title`, a `className` joined onto
   * the row recipe. `onClick`/`onKeyDown` run before the table's own and can `preventDefault()`
   * to swallow the row click. `data-key` and the row's test id stay the table's.
   */
  rowProps?: (row: T) => DataTableRowProps;
  /** The row's `data-testid` (default `lz-row`). */
  rowTestId?: (row: T) => string;
  /**
   * A full-width row under the row, rendered when this returns content — a live download line,
   * the fleet's sibling lanes. Keyed by the row's identity; carries the row's selection fill.
   */
  renderSubRow?: (row: T) => ReactNode;
  /** Rendered in one full-width row when `rows` is empty — pass an EmptyState. */
  empty?: ReactNode;
  'aria-label'?: string;
  className?: string;
}

const ALIGN = { left: 'text-left', right: 'text-right', center: 'text-center' } as const;

/**
 * A plain table: zone-register header, hairline dividers, solid surface-2 hover, no chip piles.
 * Numbers are right-aligned tabular figures. The header row names columns; the COUNT of rows
 * belongs to the SectionHeader above it.
 */
export function DataTable<T>({
  columns,
  rows,
  rowKey,
  dense = false,
  selectedKey = null,
  onRowClick,
  rowAction,
  rowProps,
  rowTestId,
  renderSubRow,
  empty,
  'aria-label': ariaLabel,
  className,
}: DataTableProps<T>) {
  const span = columns.length + (rowAction ? 1 : 0);
  return (
    <div className={cx('w-full overflow-x-auto', className)}>
      <table data-testid="lz-data-table" aria-label={ariaLabel} className="w-full border-collapse">
        <thead>
          <tr className="h-8">
            {columns.map((c) => (
              <th
                key={c.key}
                scope="col"
                style={c.width != null ? { width: c.width } : undefined}
                className={cx(
                  'whitespace-nowrap px-3 align-middle text-lz-zone uppercase text-lz-ink-3',
                  c.numeric ? ALIGN.right : ALIGN[c.align ?? 'left'],
                  c.className
                )}
              >
                {c.header}
              </th>
            ))}
            {rowAction && <th scope="col" className="w-0 px-3" />}
          </tr>
        </thead>
        <tbody>
          {rows.length === 0 && empty != null && (
            <tr className={cx('border-t', SURFACE.hairline)}>
              <td colSpan={span} className="px-3 py-lz-section">
                {empty}
              </td>
            </tr>
          )}
          {rows.map((row) => {
            const key = rowKey(row);
            const selected = selectedKey != null && key === selectedKey;
            const {
              className: extraClassName,
              onClick: extraOnClick,
              onKeyDown: extraOnKeyDown,
              'data-testid': extraTestId,
              ...extra
            } = rowProps?.(row) ?? {};
            const onClick =
              onRowClick || extraOnClick
                ? (e: MouseEvent<HTMLElement>) => {
                    extraOnClick?.(e);
                    if (!e.defaultPrevented) onRowClick?.(row);
                  }
                : undefined;
            const onKeyDown =
              onRowClick || extraOnKeyDown
                ? (e: KeyboardEvent<HTMLElement>) => {
                    extraOnKeyDown?.(e);
                    if (e.defaultPrevented || !onRowClick) return;
                    if (e.key === 'Enter' || e.key === ' ') {
                      e.preventDefault();
                      onRowClick(row);
                    }
                  }
                : undefined;
            const sub = renderSubRow?.(row);
            const tr = (
              <tr
                {...extra}
                data-testid={rowTestId ? rowTestId(row) : (extraTestId ?? 'lz-row')}
                data-key={key}
                aria-selected={selected || undefined}
                tabIndex={onRowClick ? 0 : extra.tabIndex}
                onClick={onClick}
                onKeyDown={onKeyDown}
                className={cx(
                  'border-t',
                  SURFACE.hairline,
                  dense ? ROW.dense : ROW.default,
                  selected ? SURFACE.selected : cx('text-lz-ink', SURFACE.hover),
                  onRowClick && 'cursor-pointer',
                  MOTION,
                  extraClassName
                )}
              >
                {columns.map((c) => (
                  <td
                    key={c.key}
                    className={cx(
                      'px-3 align-middle text-lz-body',
                      c.numeric ? cx(ALIGN.right, TNUM) : ALIGN[c.align ?? 'left'],
                      c.className
                    )}
                  >
                    {c.cell(row)}
                  </td>
                ))}
                {rowAction && (
                  <td className="whitespace-nowrap px-3 text-right align-middle">
                    {rowAction(row)}
                  </td>
                )}
              </tr>
            );
            if (sub == null || sub === false) return <Fragment key={key}>{tr}</Fragment>;
            return (
              <Fragment key={key}>
                {tr}
                <tr
                  data-testid="lz-sub-row"
                  data-key={key}
                  aria-selected={selected || undefined}
                  className={cx(selected ? SURFACE.selected : 'text-lz-ink', MOTION)}
                >
                  <td colSpan={span} className="px-3 pb-2 align-top text-lz-body">
                    {sub}
                  </td>
                </tr>
              </Fragment>
            );
          })}
        </tbody>
      </table>
    </div>
  );
}
