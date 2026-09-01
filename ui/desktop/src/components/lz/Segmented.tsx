import {
  Fragment,
  forwardRef,
  type ForwardedRef,
  type HTMLAttributes,
  type KeyboardEvent,
  type ReactElement,
  type ReactNode,
  type Ref,
} from 'react';
import { DISABLED, FOCUS, MOTION, RADIUS, SURFACE, cx } from './tokens';

export interface SegmentedOption<V extends string> {
  value: V;
  label: ReactNode;
  icon?: ReactNode;
  disabled?: boolean;
  /** Native tooltip on the segment — the words a locked or terse segment explains itself with. */
  title?: string;
  /** `aria-describedby`: the id of the element that says why (e.g. "locked while the run is live"). */
  describedBy?: string;
  /** DOM id of the segment element. */
  id?: string;
  /** `data-testid` of the segment element. */
  testId?: string;
}

/**
 * - `radiogroup` (default): role=radiogroup over role=radio buttons, roving focus — Arrow keys
 *   move AND select, Home/End jump.
 * - `buttons`: role=group over plain `aria-pressed` buttons, every one in the tab order, no
 *   arrow handling. Each button keeps its own `title` / `aria-describedby` while disabled, so a
 *   locked strip still says why.
 * - `tabs`: role=tablist. Without `renderOption` the segments are role=tab buttons carrying
 *   `aria-selected` and `data-state` with roving focus; with `renderOption` the caller renders
 *   each segment (a Radix `Tabs.Trigger`) wearing the recipe handed to it, and the caller's tab
 *   machinery owns selection and focus.
 */
export type SegmentedMode = 'radiogroup' | 'buttons' | 'tabs';

export interface SegmentedRenderOption<V extends string> {
  option: SegmentedOption<V>;
  active: boolean;
  /** The group's `disabled` or the option's own. */
  disabled: boolean;
  /** The segment recipe for this option — wear it on the rendered element (`cx`, never `cn`). */
  className: string;
  /** The icon and label exactly as the primitive would render them. */
  content: ReactNode;
  /** `onChange(option.value)`. */
  select: () => void;
}

export interface SegmentedProps<V extends string> extends Omit<
  HTMLAttributes<HTMLDivElement>,
  'onChange' | 'children'
> {
  options: readonly SegmentedOption<V>[];
  value: V;
  onChange: (value: V) => void;
  'aria-label': string;
  size?: 'sm' | 'md';
  className?: string;
  as?: SegmentedMode;
  /** Locks every segment. The active one keeps its accent fill so the selection stays readable. */
  disabled?: boolean;
  /** `tabs` mode: render each segment yourself (a Radix `Tabs.Trigger`) on the recipe handed in. */
  renderOption?: (args: SegmentedRenderOption<V>) => ReactNode;
}

const SIZE = { sm: 'h-6 px-2 text-[11px]', md: 'h-7 px-2.5 text-[12px]' } as const;

const ROLE: Record<SegmentedMode, string> = {
  radiogroup: 'radiogroup',
  buttons: 'group',
  tabs: 'tablist',
};

/**
 * The segment recipe. A locked strip keeps its selection readable: the active segment stays the
 * accent fill and only the others take the solid disabled neutral — never an opacity.
 */
function segmentClass(active: boolean, size: 'sm' | 'md'): string {
  return cx(
    'inline-flex items-center gap-1.5 whitespace-nowrap rounded-[4px] font-lz-medium [&_svg]:size-3.5 [&_svg]:shrink-0',
    SIZE[size],
    active
      ? cx(SURFACE.selected, 'disabled:pointer-events-none')
      : cx('text-lz-ink-2 hover:text-lz-ink', SURFACE.hover, DISABLED),
    FOCUS,
    MOTION
  );
}

/**
 * A controlled single-choice group: neutral outline, the active segment is the accent fill with
 * white ink. The ref reaches the strip element so Radix `asChild` (Slot) can make it a
 * `Tabs.List` — Radix's roving focus collects its items through that node.
 */
function SegmentedInner<V extends string>(
  {
    options,
    value,
    onChange,
    size = 'md',
    className,
    'aria-label': ariaLabel,
    as: mode = 'radiogroup',
    disabled = false,
    renderOption,
    role,
    onKeyDown: onKeyDownProp,
    ...rest
  }: SegmentedProps<V>,
  ref: ForwardedRef<HTMLDivElement>
) {
  const ownsFocus = mode !== 'buttons' && !(mode === 'tabs' && renderOption);

  const onKeyDown = (e: KeyboardEvent<HTMLDivElement>) => {
    onKeyDownProp?.(e);
    if (!ownsFocus || disabled || e.defaultPrevented) return;
    const enabled = options.filter((o) => !o.disabled);
    const i = enabled.findIndex((o) => o.value === value);
    if (i < 0) return;
    let next = -1;
    if (e.key === 'ArrowRight' || e.key === 'ArrowDown') next = (i + 1) % enabled.length;
    else if (e.key === 'ArrowLeft' || e.key === 'ArrowUp')
      next = (i - 1 + enabled.length) % enabled.length;
    else if (e.key === 'Home') next = 0;
    else if (e.key === 'End') next = enabled.length - 1;
    if (next < 0 || next === i) return;
    e.preventDefault();
    const target = enabled[next].value;
    onChange(target);
    e.currentTarget.querySelector<HTMLButtonElement>(`[data-value="${target}"]`)?.focus();
  };

  return (
    <div
      {...rest}
      ref={ref}
      role={role ?? ROLE[mode]}
      aria-label={ariaLabel}
      onKeyDown={onKeyDown}
      data-testid="lz-segmented"
      className={cx(
        'inline-flex items-center gap-0.5 bg-lz-surface p-0.5',
        SURFACE.outline,
        RADIUS.control,
        className
      )}
    >
      {options.map((o) => {
        const active = o.value === value;
        const locked = disabled || Boolean(o.disabled);
        const recipe = segmentClass(active, size);
        const content = (
          <>
            {o.icon != null && <span aria-hidden>{o.icon}</span>}
            {o.label}
          </>
        );
        const select = () => onChange(o.value);
        if (mode === 'tabs' && renderOption) {
          return (
            <Fragment key={o.value}>
              {renderOption({
                option: o,
                active,
                disabled: locked,
                className: recipe,
                content,
                select,
              })}
            </Fragment>
          );
        }
        const roving = mode === 'buttons' ? undefined : active ? 0 : -1;
        return (
          <button
            key={o.value}
            type="button"
            id={o.id}
            data-testid={o.testId}
            title={o.title}
            aria-describedby={o.describedBy}
            role={mode === 'radiogroup' ? 'radio' : mode === 'tabs' ? 'tab' : undefined}
            aria-checked={mode === 'radiogroup' ? active : undefined}
            aria-pressed={mode === 'buttons' ? active : undefined}
            aria-selected={mode === 'tabs' ? active : undefined}
            data-state={mode === 'tabs' ? (active ? 'active' : 'inactive') : undefined}
            data-value={o.value}
            tabIndex={roving}
            disabled={locked}
            onClick={select}
            className={recipe}
          >
            {content}
          </button>
        );
      })}
    </div>
  );
}

export const Segmented = forwardRef(SegmentedInner) as <V extends string>(
  props: SegmentedProps<V> & { ref?: Ref<HTMLDivElement> }
) => ReactElement;
