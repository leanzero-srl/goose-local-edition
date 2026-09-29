/**
 * A scroll the person ASKED for without touching the scroller (a search result, "show in chat").
 * The chat follows its live edge until the person scrolls; a programmatic jump is the person too,
 * so it announces itself with this event first — otherwise the following chat would pull the view
 * straight back to the bottom (Q-496).
 */
export const SCROLL_INTENT_EVENT = 'goose:scroll-intent';

/** Tell the scroller around `el` (or `el` itself) that the next scroll is the person's. */
export function announceUserScroll(el: Element): void {
  el.dispatchEvent(new Event(SCROLL_INTENT_EVENT, { bubbles: true }));
}

/** Scroll `el` into view as the person's own jump. */
export function revealAsUser(
  el: Element | null,
  options: Parameters<Element['scrollIntoView']>[0]
): void {
  if (!el) return;
  announceUserScroll(el);
  el.scrollIntoView(options);
}
