import type { ReactNode } from 'react';
import { SectionHeader } from './SectionHeader';
import { SPACE, SURFACE, cx } from './tokens';

export interface PanelProps {
  /** Header as a SectionHeader (zone register) with an optional count and right slot… */
  title?: ReactNode;
  count?: number;
  headerRight?: ReactNode;
  /** …or a fully custom header node (wins over `title`). */
  header?: ReactNode;
  /** 16px body padding (off for a DataTable, which brings its own). */
  padded?: boolean;
  children: ReactNode;
  className?: string;
}

/** The surface card: 1px border, radius 10, no shadow. */
export function Panel({
  title,
  count,
  headerRight,
  header,
  padded = true,
  children,
  className,
}: PanelProps) {
  const head =
    header ??
    (title != null ? (
      <SectionHeader title={title} count={count} right={headerRight} className="w-full" />
    ) : null);
  return (
    <section data-testid="lz-panel" className={cx(SURFACE.card, 'overflow-hidden', className)}>
      {head != null && (
        <div className={cx('flex h-10 items-center border-b px-4', SURFACE.hairline)}>{head}</div>
      )}
      <div className={padded ? SPACE.card : undefined}>{children}</div>
    </section>
  );
}
