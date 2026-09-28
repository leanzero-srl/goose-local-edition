import { useEffect, useRef, useState } from 'react';
import { EngineGlanceCard } from './EngineGlanceCard';
import { glancePipAction, useEngineGlance } from './glanceStore';
import { useGlanceDrag } from './useGlanceDrag';

/**
 * The desktop mini window's whole page (`#/engine-glance`, engineGlanceWindow.ts): the glance card,
 * sized to its content (main fits the window to what this reports), moved by dragging its body and
 * snapped to a corner by main on release. It never navigates: every click is an action main runs.
 * Only the opening ones (the Open control, the chat line, the needs-you strip) bring a goose window
 * forward; the X closes it for the session and raises nothing (Q-426).
 */
export function EngineGlanceDesktopRoot() {
  const push = useEngineGlance();
  const [expanded, setExpanded] = useState(false);
  // The one-time hint (Q-224), decided by the first glance this window reads: main marks it seen the
  // moment it first shows the window, and that must not take it off the screen it is showing on.
  const [hint, setHint] = useState<boolean | null>(null);
  if (push != null && hint === null) setHint(!push.prefs.desktopHintSeen);
  const root = useRef<HTMLDivElement>(null);
  const dragHandlers = useGlanceDrag({
    onStart: (screenX, screenY) => glancePipAction({ type: 'drag-start', screenX, screenY }),
    onMove: (screenX, screenY) => glancePipAction({ type: 'drag-move', screenX, screenY }),
    onEnd: () => glancePipAction({ type: 'drag-end' }),
  });

  // The window around the card is clear (engineGlanceWindow.ts): the page must not paint over it.
  useEffect(() => {
    document.documentElement.style.background = 'transparent';
    document.body.style.background = 'transparent';
    document.body.style.margin = '0';
    document.body.style.overflow = 'hidden';
  }, []);

  const ready = push != null;
  useEffect(() => {
    const el = root.current;
    if (!ready || !el) return undefined;
    const report = () => {
      const { width, height } = el.getBoundingClientRect();
      if (width > 0 && height > 0) glancePipAction({ type: 'size', width, height });
    };
    report();
    if (typeof ResizeObserver === 'undefined') return undefined;
    const observer = new ResizeObserver(report);
    observer.observe(el);
    return () => observer.disconnect();
  }, [ready]);

  if (!push) return null;
  return (
    <div ref={root} data-testid="engine-glance-desktop" className="inline-block w-max align-top">
      <EngineGlanceCard
        push={push}
        variant="desktop"
        collapsed={push.prefs.desktopCollapsed}
        expanded={expanded}
        onOpenEngine={() => glancePipAction({ type: 'open-engine' })}
        onOpenSession={(sessionId) => glancePipAction({ type: 'open-session', sessionId })}
        onToggleExpanded={() => setExpanded((v) => !v)}
        onCollapsedChange={(collapsed) => glancePipAction({ type: 'collapse', collapsed })}
        onClose={() => glancePipAction({ type: 'close' })}
        corner={push.prefs.desktopPlace?.corner}
        closeHint={hint === true}
        onDismissHint={() => setHint(false)}
        dragHandlers={dragHandlers}
      />
    </div>
  );
}
