import { useLayoutEffect, useState } from 'react';

/**
 * The top band's chrome and the chat title that shares it (Q-315). The left cluster (the sidebar
 * toggle and "N needs you", AppLayout) and the brand chip (BaseChat) float over the band from two
 * different trees; the title is centred over the chat. At 460 px the pill sat on the title
 * ("J[1 needs you]ssessm…") and the brand chip on its end. Each floating piece registers itself
 * here; the title reads how far they reach into its band and keeps clear of them — centred while
 * the room allows, shifted and then truncated when it does not.
 */

export type HeaderSide = 'left' | 'right';

interface Box {
  left: number;
  right: number;
  top: number;
  bottom: number;
}

export interface HeaderInsets {
  left: number;
  right: number;
}

/**
 * How far the obstacles reach into `band` from each side: a left obstacle's right edge past the
 * band's left edge, a right obstacle's left edge short of the band's right edge. Obstacles that do
 * not share the band's rows, or sit wholly outside it (the cluster over an open sidebar), reach 0.
 */
export function headerInsets(
  band: Box,
  obstacles: ReadonlyArray<{ side: HeaderSide; box: Box }>
): HeaderInsets {
  let left = 0;
  let right = 0;
  for (const { side, box } of obstacles) {
    if (box.bottom <= band.top || box.top >= band.bottom) continue;
    if (box.right <= box.left) continue;
    if (side === 'left') left = Math.max(left, box.right - band.left);
    else right = Math.max(right, band.right - box.left);
  }
  return { left, right };
}

const obstacles = new Map<symbol, { side: HeaderSide; el: HTMLElement }>();
const listeners = new Set<() => void>();

function notify(): void {
  for (const listener of listeners) listener();
}

/**
 * A piece of chrome floating over the top band, on `side`: the title keeps clear of it. Takes the
 * element itself (a callback ref's state), so an element that mounts later still registers.
 */
export function useHeaderObstacle(el: HTMLElement | null, side: HeaderSide): void {
  useLayoutEffect(() => {
    if (!el) return;
    const key = Symbol(side);
    obstacles.set(key, { side, el });
    notify();
    const observer = typeof ResizeObserver === 'undefined' ? null : new ResizeObserver(notify);
    observer?.observe(el);
    return () => {
      observer?.disconnect();
      obstacles.delete(key);
      notify();
    };
  }, [el, side]);
}

/** How far the registered chrome reaches into the element's band, kept current on every resize. */
export function useHeaderInsets(el: HTMLElement | null): HeaderInsets {
  const [insets, setInsets] = useState<HeaderInsets>({ left: 0, right: 0 });
  useLayoutEffect(() => {
    if (!el) return;
    const measure = () => {
      const next = headerInsets(
        el.getBoundingClientRect(),
        [...obstacles.values()].map(({ side, el: obstacle }) => ({
          side,
          box: obstacle.getBoundingClientRect(),
        }))
      );
      setInsets((prev) => (prev.left === next.left && prev.right === next.right ? prev : next));
    };
    measure();
    listeners.add(measure);
    window.addEventListener('resize', measure);
    const observer = typeof ResizeObserver === 'undefined' ? null : new ResizeObserver(measure);
    observer?.observe(el);
    return () => {
      listeners.delete(measure);
      window.removeEventListener('resize', measure);
      observer?.disconnect();
    };
  }, [el]);
  return insets;
}
