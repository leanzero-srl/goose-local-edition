import { readFileSync } from 'node:fs';
import { resolve } from 'node:path';
import { afterEach, beforeAll, describe, expect, it } from 'vitest';
import { CURRENT_HIGHLIGHT, MATCH_HIGHLIGHT, SearchHighlighter } from './searchHighlighter';

/**
 * Q-457 reopened — the Cmd+F matches must sit BEHIND the words. The 3.0.76 walk
 * (q457-findbar-matches-*.png) showed every "apples" replaced by a solid yellow block: the
 * highlighter drew positioned <div>s in an overlay above the transcript (z-index 1), so the solid
 * fill hid the text it marked. This pins the other mechanism: the matches are Ranges registered
 * with the CSS Custom Highlight API (painted under the text by the browser), no element is laid
 * over the transcript, and the stylesheet paints those highlights with the fixed dark ink.
 *
 * jsdom has no Highlight registry; Electron's Chromium does. The stand-in below has the platform's
 * shape (a Set of ranges with a priority, in a Map keyed by name) and nothing more.
 */
type DomRange = ReturnType<typeof document.createRange>;
type Registry = Map<string, Set<DomRange>>;
const registry = () => (window.CSS as unknown as { highlights: Registry }).highlights;

beforeAll(() => {
  if (typeof globalThis.Highlight === 'undefined') {
    class HighlightStandIn extends Set<DomRange> {
      priority = 0;
    }
    (globalThis as unknown as { Highlight: unknown }).Highlight = HighlightStandIn;
  }
  const css = globalThis.CSS as unknown as { highlights?: Map<string, unknown> } | undefined;
  if (!css) (globalThis as unknown as { CSS: unknown }).CSS = { highlights: new Map() };
  else if (!css.highlights) css.highlights = new Map();
});

function transcript(): HTMLElement {
  const root = document.createElement('div');
  root.innerHTML =
    '<p>Here is a reply about <b>apples</b> and oranges.</p><p>The apples are red, apples again.</p>';
  document.body.appendChild(root);
  return root;
}

function ranges(name: string): string[] {
  const h = registry().get(name);
  return h ? Array.from(h).map((r) => r.toString()) : [];
}

let live: SearchHighlighter | null = null;
afterEach(() => {
  live?.destroy();
  live = null;
  document.body.innerHTML = '';
});

describe('find-in-page highlights sit behind the words (Q-457 reopened)', () => {
  it('registers every match as a Range and lays NO element over the transcript', () => {
    const root = transcript();
    const before = document.body.querySelectorAll('*').length;
    live = new SearchHighlighter(root);
    const found = live.highlight('apples');

    expect(found).toHaveLength(3);
    expect(ranges(MATCH_HIGHLIGHT)).toEqual(['apples', 'apples', 'apples']);
    expect(ranges(CURRENT_HIGHLIGHT)).toEqual(['apples']);
    expect(document.body.querySelectorAll('*').length, 'an overlay element was added').toBe(
      before
    );
    expect(root.textContent).toContain('about apples and oranges');
  });

  it('moves the current match without touching the others, and clears only its own ranges', () => {
    live = new SearchHighlighter(transcript());
    const found = live.highlight('APPLES', false);
    live.setCurrentMatch(2, false);
    const current = Array.from(registry().get(CURRENT_HIGHLIGHT) ?? []);
    expect(current).toEqual([found[2]]);
    expect(live.highlight('APPLES', true)).toHaveLength(0);
    expect(ranges(MATCH_HIGHLIGHT)).toEqual([]);
    expect(ranges(CURRENT_HIGHLIGHT)).toEqual([]);
  });

  it('the stylesheet paints the highlights with the solid fill AND the dark ink, and keeps no overlay rule', () => {
    const css = readFileSync(resolve(__dirname, '../styles/main.css'), 'utf8');
    const rule = (name: string) =>
      new RegExp(`::highlight\\(${name}\\)\\s*\\{([^}]*)\\}`).exec(css)?.[1] ?? '';
    expect(rule(MATCH_HIGHLIGHT)).toMatch(/background-color:\s*var\(--highlight-color\)/);
    expect(rule(MATCH_HIGHLIGHT)).toMatch(/(^|[^-])color:\s*var\(--highlight-ink\)/);
    expect(rule(CURRENT_HIGHLIGHT)).toMatch(/background-color:\s*var\(--highlight-current\)/);
    expect(rule(CURRENT_HIGHLIGHT)).toMatch(/(^|[^-])color:\s*var\(--highlight-ink\)/);
    expect(css).not.toMatch(/\.search-highlight\b/);
  });
});
