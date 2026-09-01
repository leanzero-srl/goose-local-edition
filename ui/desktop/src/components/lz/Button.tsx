import { forwardRef, type ButtonHTMLAttributes, type ReactNode } from 'react';
import { DISABLED, FOCUS, MOTION, RADIUS, cx } from './tokens';

export type ButtonVariant = 'primary' | 'secondary' | 'ghost';
export type ButtonSize = 'sm' | 'md';

export interface ButtonProps extends Omit<ButtonHTMLAttributes<HTMLButtonElement>, 'className'> {
  /** primary = the accent fill (ONE per view); secondary = neutral outline; ghost = text until hovered. */
  variant?: ButtonVariant;
  size?: ButtonSize;
  /** Leading icon slot (a lucide icon); sized by the button. */
  icon?: ReactNode;
  /**
   * A square control for a lone icon (the caller supplies the aria-label). Its own class set:
   * MEASURED, `px-0` sorts before `px-2.5` in this pipeline and cannot zero the text padding.
   */
  iconOnly?: boolean;
  className?: string;
}

const VARIANT: Record<ButtonVariant, string> = {
  primary:
    'border border-lz-accent bg-lz-accent text-lz-accent-ink hover:border-lz-accent-hover hover:bg-lz-accent-hover',
  secondary: 'border border-lz-border-strong bg-lz-surface text-lz-ink hover:bg-lz-surface-2',
  ghost:
    'border border-transparent bg-transparent text-lz-ink-2 hover:bg-lz-surface-2 hover:text-lz-ink',
};

const SIZE: Record<ButtonSize, string> = {
  sm: 'h-7 gap-1.5 px-2.5 text-[12px] [&_svg]:size-3.5',
  md: 'h-8 gap-2 px-3 text-lz-body [&_svg]:size-4',
};

/** Square: 28 and 32, the control heights of DESIGN.md, with no text padding. */
const SIZE_ICON_ONLY: Record<ButtonSize, string> = {
  sm: 'size-7 [&_svg]:size-3.5',
  md: 'size-8 [&_svg]:size-4',
};

/** The ref reaches the <button>: Radix `asChild` (Slot) and focus management depend on it. */
export const Button = forwardRef<HTMLButtonElement, ButtonProps>(function Button(
  {
    variant = 'secondary',
    size = 'md',
    icon,
    iconOnly = false,
    className,
    type = 'button',
    children,
    ...rest
  },
  ref
) {
  return (
    <button
      ref={ref}
      type={type}
      data-variant={variant}
      data-icon-only={iconOnly || undefined}
      className={cx(
        'inline-flex shrink-0 items-center justify-center whitespace-nowrap font-lz-medium [&_svg]:shrink-0',
        RADIUS.control,
        iconOnly ? SIZE_ICON_ONLY[size] : SIZE[size],
        VARIANT[variant],
        DISABLED,
        FOCUS,
        MOTION,
        className
      )}
      {...rest}
    >
      {icon != null && <span aria-hidden>{icon}</span>}
      {children}
    </button>
  );
});
Button.displayName = 'Button';
