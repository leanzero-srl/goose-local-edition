import * as React from 'react';
import * as ScrollAreaPrimitive from '@radix-ui/react-scroll-area';

import { cn } from '../../utils';
import { SCROLL_INTENT_EVENT } from '../../utils/userScroll';

type ScrollBehavior = 'auto' | 'smooth';

export interface ScrollAreaHandle {
  /** Follow the live edge again and go there now (the "Jump to latest" button, a sent message). */
  scrollToBottom: () => void;
  scrollToPosition: (options: { top: number; behavior?: ScrollBehavior }) => void;
  isAtBottom: () => boolean;
  isFollowing: boolean;
  viewportRef: React.RefObject<HTMLDivElement | null>;
}

interface ScrollAreaProps extends React.ComponentPropsWithoutRef<typeof ScrollAreaPrimitive.Root> {
  /** Follow the live edge: content that grows or folds keeps the view at the bottom. */
  autoScroll?: boolean;
  /** Called when follow mode turns on or off. */
  onScrollChange?: (isFollowing: boolean) => void;
  /* padding needs to be passed into the container inside ScrollArea to avoid pushing the scrollbar out */
  paddingX?: number;
  paddingY?: number;
  handleScroll?: (viewport: HTMLDivElement) => void;
}

/** Sub-pixel rounding and zoom leave a bottom that is a few px short of scrollHeight. */
const LIVE_EDGE_SLACK_PX = 8;
const SCROLL_UP_KEYS = new Set(['ArrowUp', 'PageUp', 'Home']);

function isEditable(target: Event['target']): boolean {
  if (!(target instanceof HTMLElement)) return false;
  return (
    target.isContentEditable ||
    target.tagName === 'INPUT' ||
    target.tagName === 'TEXTAREA' ||
    target.tagName === 'SELECT'
  );
}

/** A scroller between `target` and the viewport that takes an upward scroll itself (a tool's output box). */
function nestedScrollerTakesUp(target: Event['target'], viewport: HTMLElement): boolean {
  let el = target instanceof Element ? target : null;
  while (el && el !== viewport) {
    if (el instanceof HTMLElement && el.scrollTop > 0 && el.scrollHeight > el.clientHeight) {
      const { overflowY } = window.getComputedStyle(el);
      if (overflowY === 'auto' || overflowY === 'scroll') return true;
    }
    el = el.parentElement;
  }
  return false;
}

/**
 * Follow mode (Q-496). While following, the view stays at the bottom whatever the content does —
 * a block landing, a card folding, scroll anchoring moving scrollTop, a smooth scroll overtaken by
 * more content. It ends ONLY on the person: a wheel or touch drag upward, a scroll-up key, a
 * scrollbar drag, or a jump they asked for (SCROLL_INTENT_EVENT). It resumes when they scroll back
 * down to the bottom or call scrollToBottom.
 *
 * The old rule — "any scroll event more than 200 px from the bottom is the person scrolling up" —
 * turned follow off on the first frame of its OWN smooth auto-scroll whenever one render added more
 * than that (a tool group, a failed row with its error, a notice); the animation's target was fixed
 * when it started, the turn kept streaming, and nothing ever scrolled again.
 */
const ScrollArea = React.forwardRef<ScrollAreaHandle, ScrollAreaProps>(
  (
    {
      className,
      children,
      autoScroll = false,
      onScrollChange,
      paddingX,
      paddingY,
      handleScroll: handleScrollProp,
      ...props
    },
    ref
  ) => {
    const rootRef = React.useRef<React.ElementRef<typeof ScrollAreaPrimitive.Root>>(null);
    const viewportRef = React.useRef<HTMLDivElement>(null);
    const contentRef = React.useRef<HTMLDivElement>(null);
    const [isFollowing, setIsFollowing] = React.useState(true);
    const [isScrolled, setIsScrolled] = React.useState(false);
    const followingRef = React.useRef(true);
    const pointerHeldRef = React.useRef(false);
    const lastScrollTopRef = React.useRef(0);
    const touchYRef = React.useRef<number | null>(null);

    const autoScrollRef = React.useRef(autoScroll);
    const onScrollChangeRef = React.useRef(onScrollChange);
    const handleScrollPropRef = React.useRef(handleScrollProp);
    React.useLayoutEffect(() => {
      autoScrollRef.current = autoScroll;
      onScrollChangeRef.current = onScrollChange;
      handleScrollPropRef.current = handleScrollProp;
    });

    const setFollowing = React.useCallback((next: boolean) => {
      if (followingRef.current === next) return;
      followingRef.current = next;
      setIsFollowing(next);
      onScrollChangeRef.current?.(next);
    }, []);

    const distanceFromBottom = React.useCallback(() => {
      const viewport = viewportRef.current;
      if (!viewport) return 0;
      return viewport.scrollHeight - viewport.scrollTop - viewport.clientHeight;
    }, []);

    const isAtBottom = React.useCallback(
      () => distanceFromBottom() <= LIVE_EDGE_SLACK_PX,
      [distanceFromBottom]
    );

    const pinToBottom = React.useCallback(() => {
      const viewport = viewportRef.current;
      if (!viewport) return;
      viewport.scrollTop = viewport.scrollHeight;
      lastScrollTopRef.current = viewport.scrollTop;
    }, []);

    /** The person moved the view up (or asked to): stop following, if there is anywhere to go. */
    const personScrolledUp = React.useCallback(() => {
      const viewport = viewportRef.current;
      if (!viewport || viewport.scrollTop <= 0) return;
      setFollowing(false);
    }, [setFollowing]);

    const scrollToBottom = React.useCallback(() => {
      setFollowing(true);
      pinToBottom();
    }, [pinToBottom, setFollowing]);

    const scrollToPosition = React.useCallback(
      ({ top, behavior = 'smooth' }: { top: number; behavior?: ScrollBehavior }) => {
        const viewport = viewportRef.current;
        if (!viewport) return;
        if (top < viewport.scrollHeight - viewport.clientHeight - LIVE_EDGE_SLACK_PX) {
          setFollowing(false);
        }
        viewport.scrollTo({ top, behavior });
      },
      [setFollowing]
    );

    React.useImperativeHandle(
      ref,
      () => ({
        scrollToBottom,
        scrollToPosition,
        isAtBottom,
        get isFollowing() {
          return followingRef.current;
        },
        viewportRef,
      }),
      [scrollToBottom, scrollToPosition, isAtBottom]
    );

    // A scroll event is a FACT about where the view is, never evidence of who moved it: while
    // following, a view that is off the bottom was moved by content (or anchoring, or an overtaken
    // animation) and goes back; only a held pointer — the scrollbar thumb, a selection drag — makes
    // an upward scroll the person's.
    React.useEffect(() => {
      const viewport = viewportRef.current;
      if (!viewport) return;
      lastScrollTopRef.current = viewport.scrollTop;
      const onScroll = () => {
        const top = viewport.scrollTop;
        const movedUp = top < lastScrollTopRef.current;
        lastScrollTopRef.current = top;
        const distance = viewport.scrollHeight - top - viewport.clientHeight;
        if (followingRef.current) {
          if (pointerHeldRef.current && movedUp && distance > LIVE_EDGE_SLACK_PX) {
            setFollowing(false);
          } else if (autoScrollRef.current && distance > LIVE_EDGE_SLACK_PX) {
            pinToBottom();
          }
        } else if (!movedUp && distance <= LIVE_EDGE_SLACK_PX) {
          setFollowing(true);
        }
        setIsScrolled(viewport.scrollTop > 0);
        handleScrollPropRef.current?.(viewport);
      };
      viewport.addEventListener('scroll', onScroll, { passive: true });
      return () => viewport.removeEventListener('scroll', onScroll);
    }, [pinToBottom, setFollowing]);

    // The person's intents. Each is read where it happens, before the scroll it causes.
    React.useEffect(() => {
      const viewport = viewportRef.current;
      const root = rootRef.current;
      if (!viewport || !root) return;
      const listeners = new AbortController();
      const passive = { passive: true, signal: listeners.signal };
      viewport.addEventListener(
        'wheel',
        (e) => {
          if (e.deltaY < 0 && !nestedScrollerTakesUp(e.target, viewport)) personScrolledUp();
        },
        passive
      );
      viewport.addEventListener(
        'touchstart',
        (e) => {
          touchYRef.current = e.touches[0]?.clientY ?? null;
        },
        passive
      );
      viewport.addEventListener(
        'touchmove',
        (e) => {
          const y = e.touches[0]?.clientY;
          const from = touchYRef.current;
          if (y === undefined || from === null) return;
          touchYRef.current = y;
          // A finger moving DOWN drags the content down: the view goes up.
          if (y > from && !nestedScrollerTakesUp(e.target, viewport)) personScrolledUp();
        },
        passive
      );
      viewport.addEventListener(SCROLL_INTENT_EVENT, personScrolledUp, {
        signal: listeners.signal,
      });
      // Keys scroll the transcript when focus is in it — or on the body after a click on its text,
      // which is why this listens on the window and not on the root.
      window.addEventListener(
        'keydown',
        (e) => {
          if (e.metaKey || e.ctrlKey || e.altKey || isEditable(e.target)) return;
          // A hidden chat (another session's pane) has no height and is not the one being read.
          const inTranscript =
            (e.target === document.body && viewport.clientHeight > 0) ||
            (e.target instanceof Node && root.contains(e.target));
          const up = SCROLL_UP_KEYS.has(e.key) || (e.key === ' ' && e.shiftKey);
          if (inTranscript && up && !nestedScrollerTakesUp(e.target, viewport)) personScrolledUp();
        },
        { signal: listeners.signal }
      );
      root.addEventListener(
        'pointerdown',
        (e) => {
          if (e.button === 0) pointerHeldRef.current = true;
        },
        { signal: listeners.signal }
      );
      const release = () => {
        pointerHeldRef.current = false;
      };
      window.addEventListener('pointerup', release, { signal: listeners.signal });
      window.addEventListener('pointercancel', release, { signal: listeners.signal });
      return () => listeners.abort();
    }, [personScrolledUp]);

    // Content that changed with this render: back to the edge before paint.
    React.useLayoutEffect(() => {
      if (autoScroll && followingRef.current) pinToBottom();
    }, [children, autoScroll, pinToBottom]);

    // Content that changed WITHOUT this component rendering (a card inside the list expanding, an
    // image loading) and a viewport that changed height (a tray opening under it).
    React.useEffect(() => {
      const viewport = viewportRef.current;
      const content = contentRef.current;
      if (!autoScroll || !viewport || !content || typeof ResizeObserver === 'undefined') return;
      const observer = new ResizeObserver(() => {
        if (followingRef.current) pinToBottom();
      });
      observer.observe(viewport);
      observer.observe(content);
      return () => observer.disconnect();
    }, [autoScroll, pinToBottom]);

    return (
      <ScrollAreaPrimitive.Root
        ref={rootRef}
        className={cn('relative overflow-hidden', className)}
        data-scrolled={isScrolled}
        data-following={autoScroll ? isFollowing : undefined}
        {...props}
      >
        <div className={cn('absolute top-0 left-0 right-0 z-10 transition-all duration-200')} />
        <ScrollAreaPrimitive.Viewport
          ref={viewportRef}
          className="h-full w-full rounded-[inherit] [&>div]:!block"
        >
          <div
            ref={contentRef}
            className={cn(paddingX ? `px-${paddingX}` : '', paddingY ? `py-${paddingY}` : '')}
          >
            {children}
          </div>
        </ScrollAreaPrimitive.Viewport>
        <ScrollBar />
        <ScrollAreaPrimitive.Corner />
      </ScrollAreaPrimitive.Root>
    );
  }
);
ScrollArea.displayName = ScrollAreaPrimitive.Root.displayName;

const ScrollBar = React.forwardRef<
  React.ElementRef<typeof ScrollAreaPrimitive.ScrollAreaScrollbar>,
  React.ComponentPropsWithoutRef<typeof ScrollAreaPrimitive.ScrollAreaScrollbar>
>(({ className, orientation = 'vertical', ...props }, ref) => (
  <ScrollAreaPrimitive.ScrollAreaScrollbar
    ref={ref}
    orientation={orientation}
    className={cn(
      'flex touch-none select-none transition-colors',
      orientation === 'vertical' && 'h-full w-2.5 border-l border-l-transparent p-[1px]',
      orientation === 'horizontal' && 'h-2.5 flex-col border-t border-t-transparent p-[1px]',
      className
    )}
    {...props}
  >
    <ScrollAreaPrimitive.ScrollAreaThumb className="relative flex-1 rounded-full bg-border-primary dark:bg-background-secondary" />
  </ScrollAreaPrimitive.ScrollAreaScrollbar>
));
ScrollBar.displayName = ScrollAreaPrimitive.ScrollAreaScrollbar.displayName;

export { ScrollArea, ScrollBar };
