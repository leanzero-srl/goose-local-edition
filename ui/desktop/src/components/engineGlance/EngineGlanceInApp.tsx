import { useEffect, useRef, useState, useSyncExternalStore, type RefObject } from 'react';
import { useNavigate } from 'react-router-dom';
import { EngineGlanceCard } from './EngineGlanceCard';
import { useEngineGlance } from './glanceStore';
import { useGlanceDrag } from './useGlanceDrag';
import { dockFits, inAppPlacement, nearestCorner } from '../../utils/engineGlanceRules';
import type { GlanceCorner } from '../../utils/engineGlance';
import { sessionHref } from '../sessionActivity/sessionActivityStore';
import { LAYER, RADIUS, cx } from '../lz';

/**
 * The engine glance inside a goose window: DOCKED in the sidebar's empty space below the session
 * trees when that space fits it, FLOATING over the content (draggable, snapping to a corner,
 * collapsible to a pill) when it does not — the sidebar is collapsed, or its trees fill it. Which
 * one is `inAppPlacement` (engineGlanceRules.ts); the two slots below only measure and render.
 */

/** The dock's measured facts, shared by the sidebar slot (which measures) and the float slot. */
let dockRoom = false;
/** The card's last measured height; the first decision uses the docked card's usual height. */
const DOCK_ESTIMATE_PX = 132;
let cardHeight = DOCK_ESTIMATE_PX;
const dockListeners = new Set<() => void>();

function setDockRoom(next: boolean): void {
  if (next === dockRoom) return;
  dockRoom = next;
  dockListeners.forEach((l) => l());
}

function useDockRoom(): boolean {
  return useSyncExternalStore(
    (l) => {
      dockListeners.add(l);
      return () => dockListeners.delete(l);
    },
    () => dockRoom
  );
}

/** The gap between the trees and the docked card (the sidebar's own `mt-2`). */
const DOCK_GAP_PX = 8;

function useOpeners() {
  const navigate = useNavigate();
  return {
    openEngine: () => navigate('/leanzero-swarm?tab=mlx'),
    openSession: (sessionId: string) => navigate(sessionHref(sessionId)),
  };
}

/**
 * The sidebar slot, rendered below the trees' scroll area. It measures the column: the trees' own
 * height (which does not change when the card docks, so the decision never flaps) against the room
 * the column has, card included.
 */
export function EngineGlanceDockSlot({
  scrollRef,
  contentRef,
}: {
  scrollRef: RefObject<HTMLElement | null>;
  contentRef: RefObject<HTMLElement | null>;
}) {
  const push = useEngineGlance();
  const room = useDockRoom();
  const slot = useRef<HTMLDivElement>(null);
  const [expanded, setExpanded] = useState(false);
  const { openEngine, openSession } = useOpeners();
  const placement = push ? inAppPlacement(push, { navExpanded: true, dockRoom: room }) : 'hidden';

  useEffect(() => {
    const scroll = scrollRef.current;
    const content = contentRef.current;
    if (!scroll || !content) return undefined;
    const measure = () => {
      const docked = slot.current?.offsetHeight ?? 0;
      if (docked > 0) cardHeight = docked;
      const column = scroll.clientHeight + (docked > 0 ? docked + DOCK_GAP_PX : 0);
      setDockRoom(dockFits(column, content.offsetHeight, cardHeight + DOCK_GAP_PX));
    };
    measure();
    // jsdom has none: the first measure stands (UserMessage.tsx and Clipped.tsx do the same).
    if (typeof ResizeObserver === 'undefined') return undefined;
    const observer = new ResizeObserver(measure);
    observer.observe(scroll);
    observer.observe(content);
    if (slot.current) observer.observe(slot.current);
    return () => observer.disconnect();
  }, [scrollRef, contentRef, placement]);

  // The sidebar went away with this slot: the float decides alone.
  useEffect(() => () => setDockRoom(false), []);

  if (!push || placement !== 'dock') return null;
  return (
    <div ref={slot} data-testid="engine-glance-dock" className="shrink-0 px-2 pt-2">
      <EngineGlanceCard
        push={push}
        variant="dock"
        collapsed={false}
        expanded={expanded}
        onOpenEngine={openEngine}
        onOpenSession={openSession}
        onToggleExpanded={() => setExpanded((v) => !v)}
        onCollapsedChange={() => undefined}
      />
    </div>
  );
}

const FLOAT_CORNER_KEY = 'engineGlance.float.corner';
const FLOAT_COLLAPSED_KEY = 'engineGlance.float.collapsed';

const CORNER_CLASS: Record<GlanceCorner, string> = {
  'top-left': 'left-4 top-14',
  'top-right': 'right-4 top-14',
  'bottom-left': 'left-4 bottom-4',
  'bottom-right': 'right-4 bottom-4',
};

function storedCorner(): GlanceCorner {
  const v = localStorage.getItem(FLOAT_CORNER_KEY);
  return v === 'top-left' || v === 'top-right' || v === 'bottom-left' || v === 'bottom-right'
    ? v
    : 'bottom-right';
}

/**
 * The floating card over the window's content, only while something is live and the sidebar cannot
 * hold it. Dragged by its body, it snaps to the nearest corner of the content area on release; it
 * shrinks to a pill and back. Where it sits and how big it is are remembered per machine.
 */
export function EngineGlanceFloat({ navExpanded }: { navExpanded: boolean }) {
  const push = useEngineGlance();
  const room = useDockRoom();
  const card = useRef<HTMLDivElement>(null);
  const [corner, setCorner] = useState<GlanceCorner>(storedCorner);
  const [collapsed, setCollapsed] = useState(
    () => localStorage.getItem(FLOAT_COLLAPSED_KEY) === '1'
  );
  const [expanded, setExpanded] = useState(false);
  const [offset, setOffset] = useState<{ x: number; y: number } | null>(null);
  const start = useRef<{ x: number; y: number } | null>(null);
  const { openEngine, openSession } = useOpeners();
  const { dragHandlers, consumeDrag } = useGlanceDrag({
    onStart: (x, y) => {
      start.current = { x, y };
    },
    onMove: (x, y) => {
      if (start.current) setOffset({ x: x - start.current.x, y: y - start.current.y });
    },
    onEnd: (moved) => {
      start.current = null;
      const el = card.current;
      const area = el?.offsetParent as HTMLElement | null;
      if (moved && el && area) {
        const r = el.getBoundingClientRect();
        const a = area.getBoundingClientRect();
        const next = nearestCorner(
          { x: r.left - a.left, y: r.top - a.top, width: r.width, height: r.height },
          { x: 0, y: 0, width: a.width, height: a.height }
        );
        setCorner(next);
        localStorage.setItem(FLOAT_CORNER_KEY, next);
      }
      setOffset(null);
    },
  });

  useEffect(() => {
    if (card.current && !collapsed) cardHeight = card.current.offsetHeight;
  });

  if (!push || inAppPlacement(push, { navExpanded, dockRoom: room }) !== 'float') return null;
  const setCollapsedStored = (next: boolean) => {
    setCollapsed(next);
    localStorage.setItem(FLOAT_COLLAPSED_KEY, next ? '1' : '0');
  };
  return (
    <div
      ref={card}
      data-testid="engine-glance-float"
      data-corner={corner}
      className={cx(
        'absolute',
        LAYER.chrome,
        collapsed ? RADIUS.pill : RADIUS.card,
        CORNER_CLASS[corner]
      )}
      style={
        offset
          ? { transform: `translate(${offset.x}px, ${offset.y}px)`, touchAction: 'none' }
          : { touchAction: 'none' }
      }
    >
      <EngineGlanceCard
        push={push}
        variant="float"
        collapsed={collapsed}
        expanded={expanded}
        onOpenEngine={openEngine}
        onOpenSession={openSession}
        onToggleExpanded={() => setExpanded((v) => !v)}
        onCollapsedChange={setCollapsedStored}
        dragHandlers={dragHandlers}
        consumeDrag={consumeDrag}
      />
    </div>
  );
}
