/**
 * Find-in-page highlighting through the CSS Custom Highlight API.
 *
 * Q-457 reopened (3.0.76 live walk): the old implementation drew each match as an absolutely
 * positioned <div> in an overlay ABOVE the transcript. While the fill was a 50% yellow the words
 * showed through; once it became a solid #fde047 it painted over every match, and the dark
 * `--highlight-ink` it set coloured a div that holds no text. A registered Highlight is painted by
 * the browser BEHIND the text of the ranges it names, with the ink main.css gives
 * `::highlight(goose-search-match)` / `::highlight(goose-search-current)` — the words stay on top
 * and readable, and the ranges follow reflow without any position bookkeeping.
 */
type DomRange = ReturnType<typeof document.createRange>;
type DomHighlight = InstanceType<typeof window.Highlight>;

export const MATCH_HIGHLIGHT = 'goose-search-match';
export const CURRENT_HIGHLIGHT = 'goose-search-current';

/**
 * The two registered highlights are shared by every SearchHighlighter (the registry is global and
 * the stylesheet names them statically); each instance adds and removes only its own ranges.
 */
function registered(name: string, priority: number): DomHighlight {
  const existing = window.CSS.highlights.get(name);
  if (existing) return existing;
  const created = new window.Highlight();
  created.priority = priority;
  window.CSS.highlights.set(name, created);
  return created;
}

export class SearchHighlighter {
  private readonly container: HTMLElement;
  private readonly matches: DomHighlight;
  private readonly current: DomHighlight;
  private ranges: DomRange[] = [];
  private mutationObserver: MutationObserver;
  private scrollContainer: HTMLElement | null = null;
  private currentTerm: string = '';
  private caseSensitive: boolean = false;
  private onMatchesChange?: (count: number) => void;
  private currentMatchIndex: number = -1;
  private highlightTimeout?: ReturnType<typeof setTimeout>;

  constructor(container: HTMLElement, onMatchesChange?: (count: number) => void) {
    this.container = container;
    this.onMatchesChange = onMatchesChange;
    this.matches = registered(MATCH_HIGHLIGHT, 0);
    this.current = registered(CURRENT_HIGHLIGHT, 1);

    // Find scroll container (look for our custom data attribute first, then fall back to radix)
    const searchScrollArea = container.closest('[data-search-scroll-area]');
    this.scrollContainer =
      searchScrollArea?.querySelector('[data-radix-scroll-area-viewport]') ||
      (searchScrollArea as HTMLElement) ||
      container.closest('[data-radix-scroll-area-viewport]');

    // New or streamed message text invalidates the ranges; re-run the search over the new DOM.
    this.mutationObserver = new MutationObserver(() => {
      if (!this.currentTerm) return;
      if (this.highlightTimeout) {
        clearTimeout(this.highlightTimeout);
      }
      this.highlightTimeout = setTimeout(() => {
        this.highlight(this.currentTerm, this.caseSensitive);
      }, 100);
    });
    this.mutationObserver.observe(container, {
      childList: true,
      subtree: true,
      characterData: true,
    });
  }

  highlight(term: string, caseSensitive = false): DomRange[] {
    const currentIndex = this.currentMatchIndex;
    const oldCount = this.ranges.length;

    this.clearHighlights();
    this.currentTerm = term;
    this.caseSensitive = caseSensitive;

    if (!term.trim()) return [];

    const regex = new RegExp(
      term.replace(/[.*+?^${}()|[\]\\]/g, '\\$&'),
      caseSensitive ? 'g' : 'gi'
    );

    const walker = document.createTreeWalker(this.container, NodeFilter.SHOW_TEXT, {
      acceptNode: (node) => {
        const parent = node.parentElement;
        if (parent?.closest('.search-bar, .search-results')) {
          return NodeFilter.FILTER_REJECT;
        }
        return NodeFilter.FILTER_ACCEPT;
      },
    });

    let node: Text | null;
    while ((node = walker.nextNode() as Text)) {
      const text = node.textContent || '';
      regex.lastIndex = 0;
      let match;
      while ((match = regex.exec(text)) !== null) {
        const range = document.createRange();
        range.setStart(node, match.index);
        range.setEnd(node, match.index + match[0].length);
        this.ranges.push(range);
        this.matches.add(range);
      }
    }

    if (this.ranges.length !== oldCount) {
      this.onMatchesChange?.(this.ranges.length);
    }

    if (currentIndex >= 0 && this.ranges.length === oldCount) {
      this.setCurrentMatch(currentIndex, false);
    } else if (this.ranges.length > 0) {
      this.setCurrentMatch(0, false);
    }

    return this.ranges;
  }

  setCurrentMatch(index: number, shouldScroll = true) {
    if (!this.ranges.length) return;

    const wrappedIndex =
      ((index % this.ranges.length) + this.ranges.length) % this.ranges.length;
    this.currentMatchIndex = wrappedIndex;

    for (const range of this.ranges) this.current.delete(range);
    const range = this.ranges[wrappedIndex];
    this.current.add(range);

    if (shouldScroll && this.scrollContainer) {
      const containerRect = this.scrollContainer.getBoundingClientRect();
      const matchRect = range.getBoundingClientRect();
      this.scrollContainer.scrollTop =
        this.scrollContainer.scrollTop +
        (matchRect.top - containerRect.top) -
        (containerRect.height - matchRect.height) / 2;
    }
  }

  clearHighlights() {
    for (const range of this.ranges) {
      this.matches.delete(range);
      this.current.delete(range);
    }
    this.ranges = [];
    this.currentTerm = '';
    this.currentMatchIndex = -1;
  }

  destroy() {
    if (this.highlightTimeout) {
      clearTimeout(this.highlightTimeout);
    }
    this.mutationObserver.disconnect();
    this.clearHighlights();
  }
}
