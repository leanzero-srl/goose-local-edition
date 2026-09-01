import type { ReactNode } from 'react';
import { TYPE, cx } from './tokens';

export interface PageHeaderProps {
  title: ReactNode;
  /** Zone-register line ABOVE the title — where the reader is ("Benchmark · sb-7"). */
  eyebrow?: ReactNode;
  /** One muted body line under the title — what this page is for. */
  subtitle?: ReactNode;
  /** Right-side slot: the page's primary actions (Buttons, a Segmented). */
  actions?: ReactNode;
  className?: string;
}

/** The top of a view: display-scale title, optional eyebrow and subtitle, actions on the right. */
export function PageHeader({ title, eyebrow, subtitle, actions, className }: PageHeaderProps) {
  return (
    <header
      data-testid="lz-page-header"
      className={cx('flex items-start justify-between gap-6', className)}
    >
      <div className="flex min-w-0 flex-col gap-1">
        {eyebrow != null && <div className={TYPE.zone}>{eyebrow}</div>}
        <h1 className={cx(TYPE.display, 'truncate')}>{title}</h1>
        {subtitle != null && <p className={TYPE.bodyMuted}>{subtitle}</p>}
      </div>
      {actions != null && <div className="flex shrink-0 items-center gap-2">{actions}</div>}
    </header>
  );
}
