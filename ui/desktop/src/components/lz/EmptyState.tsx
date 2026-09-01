import type { ReactNode } from 'react';
import { RADIUS, TONE_FILL, TYPE, cx } from './tokens';

export interface EmptyStateProps {
  /** A lucide icon; it sits in a solid accent block. */
  icon?: ReactNode;
  title: ReactNode;
  body?: ReactNode;
  /** The ONE primary action (a Button variant="primary"). */
  action?: ReactNode;
  className?: string;
}

/** Centered, max-width, branded by the accent block — never a grey placeholder. */
export function EmptyState({ icon, title, body, action, className }: EmptyStateProps) {
  return (
    <div
      data-testid="lz-empty-state"
      className={cx(
        'mx-auto flex max-w-[440px] flex-col items-center gap-3 py-lz-page text-center',
        className
      )}
    >
      {icon != null && (
        <div
          aria-hidden
          data-testid="lz-empty-state-icon"
          className={cx(
            'flex size-12 items-center justify-center [&_svg]:size-6',
            RADIUS.card,
            TONE_FILL.accent
          )}
        >
          {icon}
        </div>
      )}
      <h2 className={TYPE.display}>{title}</h2>
      {body != null && <p className={TYPE.bodyMuted}>{body}</p>}
      {action != null && <div className="mt-2 flex items-center gap-2">{action}</div>}
    </div>
  );
}
