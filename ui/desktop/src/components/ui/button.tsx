import * as React from 'react';
import { Slot } from '@radix-ui/react-slot';
import { cva, type VariantProps } from 'class-variance-authority';
import { cn } from '../../utils';

// Disabled is SOLID — surface-2 fill, ink-4 text, hairline border, not-allowed cursor — and focus
// is the accent outline: the Studio tokens on every variant, never an opacity or a faded ring.
const buttonVariants = cva(
  "inline-flex items-center justify-center gap-2 whitespace-nowrap text-sm transition-all cursor-pointer disabled:cursor-not-allowed disabled:border-lz-border disabled:bg-lz-surface-2 disabled:text-lz-ink-4 [&_svg]:pointer-events-none [&_svg:not([class*='size-'])]:size-4 shrink-0 [&_svg]:shrink-0 outline-none focus-visible:outline-2 focus-visible:outline-offset-1 focus-visible:outline-ring aria-invalid:border-destructive",
  {
    variants: {
      variant: {
        default: 'bg-background-inverse text-text-inverse hover:bg-lz-inverse-hover',
        destructive: 'bg-background-danger text-white hover:bg-lz-danger-hover',
        outline: 'border hover:bg-background-secondary',
        secondary: 'bg-background-secondary text-text-primary hover:bg-background-tertiary',
        ghost: 'hover:bg-background-secondary',
        link: 'text-text-inverse underline-offset-4 hover:underline',
      },
      size: {
        xs: 'h-6 gap-1 ![&_svg:not([class*="size-"])]:size-3',
        default: 'h-9',
        sm: 'h-8 gap-1.5',
        lg: 'h-10',
      },
      shape: {
        pill: 'rounded-md',
        round: '',
      },
    },
    compoundVariants: [
      {
        shape: 'pill',
        size: 'xs',
        className: 'px-2 has-[>svg]:px-2',
      },
      {
        shape: 'pill',
        size: 'default',
        className: 'px-4 py-2 has-[>svg]:px-4',
      },
      {
        shape: 'pill',
        size: 'sm',
        className: 'px-4 has-[>svg]:px-3',
      },
      {
        shape: 'pill',
        size: 'lg',
        className: 'px-6 has-[>svg]:px-6',
      },
      {
        shape: 'round',
        size: 'xs',
        className: 'w-6 h-6 p-0 rounded-full',
      },
      {
        shape: 'round',
        size: 'default',
        className: 'w-9 h-9 p-0 rounded-full',
      },
      {
        shape: 'round',
        size: 'sm',
        className: 'w-8 h-8 p-0 rounded-full',
      },
      {
        shape: 'round',
        size: 'lg',
        className: 'w-10 h-10 p-0 rounded-full',
      },
    ],
    defaultVariants: {
      variant: 'default',
      size: 'default',
      shape: 'pill',
    },
  }
);

const Button = React.forwardRef<
  HTMLButtonElement,
  React.ComponentProps<'button'> &
    VariantProps<typeof buttonVariants> & {
      asChild?: boolean;
      shape?: 'pill' | 'round';
    }
>(({ className, variant, size, asChild = false, shape = 'pill', ...props }, ref) => {
  const Comp = asChild ? Slot : 'button';

  return (
    <Comp
      data-slot="button"
      className={cn(buttonVariants({ variant, size, shape, className }))}
      ref={ref}
      {...props}
    />
  );
});

Button.displayName = 'Button';

export { Button, buttonVariants };
