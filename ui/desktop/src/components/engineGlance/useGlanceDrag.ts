import { useCallback, useMemo, useRef, type PointerEvent } from 'react';

/** A press that moves less than this is a click (the system's own drag slop is a few px). */
export const DRAG_SLOP_PX = 4;

export interface GlanceDragCallbacks {
  /** Screen coordinates: the desktop window moves under the pointer, client ones would drift. */
  onStart: (screenX: number, screenY: number) => void;
  onMove: (screenX: number, screenY: number) => void;
  onEnd: (moved: boolean) => void;
}

/**
 * Drag the card by its body; a press that does not move is still the click that opens the Engine.
 * `consumeDrag()` answers the click that follows a drag's release — it must not open anything.
 */
export function useGlanceDrag(callbacks: GlanceDragCallbacks) {
  const press = useRef<{ x: number; y: number; moved: boolean } | null>(null);
  const justDragged = useRef(false);
  const cb = useRef(callbacks);
  cb.current = callbacks;

  const end = useCallback((e: PointerEvent<HTMLElement>) => {
    const p = press.current;
    press.current = null;
    if (!p) return;
    if (e.currentTarget.hasPointerCapture(e.pointerId)) {
      e.currentTarget.releasePointerCapture(e.pointerId);
    }
    justDragged.current = p.moved;
    cb.current.onEnd(p.moved);
  }, []);

  const dragHandlers = useMemo(
    () => ({
      onPointerDown: (e: PointerEvent<HTMLElement>) => {
        if (e.button !== 0) return;
        press.current = { x: e.screenX, y: e.screenY, moved: false };
        justDragged.current = false;
        e.currentTarget.setPointerCapture(e.pointerId);
        cb.current.onStart(e.screenX, e.screenY);
      },
      onPointerMove: (e: PointerEvent<HTMLElement>) => {
        const p = press.current;
        if (!p) return;
        if (!p.moved && Math.hypot(e.screenX - p.x, e.screenY - p.y) < DRAG_SLOP_PX) return;
        p.moved = true;
        cb.current.onMove(e.screenX, e.screenY);
      },
      onPointerUp: end,
      onPointerCancel: end,
    }),
    [end]
  );

  const consumeDrag = useCallback(() => {
    const dragged = justDragged.current;
    justDragged.current = false;
    return dragged;
  }, []);

  return { dragHandlers, consumeDrag };
}
