import { afterEach, describe, expect, it, vi } from 'vitest';
import { readFileSync } from 'node:fs';
import { resolve } from 'node:path';
import { render, waitFor } from '@testing-library/react';
import { __unstable__loadDesignSystem as loadDesignSystem } from '@tailwindcss/node';
import MarkdownContent from './MarkdownContent';
import ThinkingContent from './ThinkingContent';
import { CODE_THEMES, codeBackground, type CodeThemeName } from './codeThemes';
import { IntlTestWrapper } from '../i18n/test-utils';

/**
 * The code-block contract, born from the owner's light-theme screenshot (2026-09-25): a Python
 * block in a Thinking section whose identifiers, `self` and argument lists were near-black on a
 * #282c34 panel, set in italic, with a stray backtick at each end. Three mechanisms, three pins:
 *
 *  1. the theme itself — every colour it can paint meets WCAG AA (4.5:1) on its block background;
 *  2. the render — every character of a rendered block takes its colour from INSIDE the block (so
 *     no page/thinking colour can reach an untokenised run), and nothing in it is italic, even
 *     under the italic Thinking wrapper;
 *  3. the cascade — Tailwind Typography's code/pre rules (its colour, weight 600 and the
 *     "`"-content ::before/::after) exclude a not-prose subtree, and the block root is one.
 */

vi.mock('./icons', () => ({
  Check: () => <span data-testid="check-icon" />,
  Copy: () => <span data-testid="copy-icon" />,
}));

const THEMES: CodeThemeName[] = ['light', 'dark'];
const AA = 4.5;

const PYTHON = [
  'Let me design a clean, simple class:',
  '',
  '```python',
  'class Ledger:',
  '    def __init__(self):',
  '        self._entries = []',
  '',
  '    def record(self, date, account, amount, memo=""):',
  '        # one entry per call',
  '        entry = Entry(date, account, amount, memo)',
  '        self._entries.append(entry)',
  '        return len(self._entries) > 0 and True',
  '```',
  '',
  'Then `ledger/core.py` holds it.',
].join('\n');

function toHex(color: string): string {
  const c = color.trim().toLowerCase();
  if (/^#[0-9a-f]{6}$/.test(c)) return c;
  const m = /^rgba?\((\d+),\s*(\d+),\s*(\d+)(?:,\s*([\d.]+))?\)$/.exec(c);
  if (!m) throw new Error(`not a solid colour: ${color}`);
  if (m[4] !== undefined && Number(m[4]) !== 1) throw new Error(`translucent colour: ${color}`);
  return '#' + [m[1], m[2], m[3]].map((v) => Number(v).toString(16).padStart(2, '0')).join('');
}

function luminance(hex: string): number {
  const lin = [1, 3, 5]
    .map((i) => parseInt(hex.slice(i, i + 2), 16) / 255)
    .map((v) => (v <= 0.03928 ? v / 12.92 : ((v + 0.055) / 1.055) ** 2.4));
  return 0.2126 * lin[0] + 0.7152 * lin[1] + 0.0722 * lin[2];
}

function contrast(a: string, b: string): number {
  const [hi, lo] = [luminance(toHex(a)), luminance(toHex(b))].sort((x, y) => y - x);
  return (hi + 0.05) / (lo + 0.05);
}

function setDocumentTheme(theme: CodeThemeName) {
  document.documentElement.classList.remove('light', 'dark');
  document.documentElement.classList.add(theme);
}

afterEach(() => {
  document.documentElement.classList.remove('light', 'dark');
});

/** The rendered block's parts, found from the block root the CodeBlock marks. */
async function renderedBlock(container: HTMLElement) {
  const root = await waitFor(() => {
    const el = container.querySelector<HTMLElement>('[data-code-block]');
    if (!el) throw new Error('no code block rendered');
    return el;
  });
  const code = root.querySelector('code');
  const pre = code?.parentElement;
  if (!code || !pre) throw new Error('code block has no <code> inside its pre');
  return { root, pre, code };
}

/** The token span holding exactly this text (inline styles carry no class names). */
function spanOf(code: HTMLElement, text: string): HTMLElement {
  const span = Array.from(code.querySelectorAll<HTMLElement>('span')).find(
    (s) => s.textContent === text
  );
  if (!span) throw new Error(`no token span for ${JSON.stringify(text)}`);
  return span;
}

/** Every text node's colour, taken from the nearest ancestor INSIDE the block that sets one. */
function paintedRuns(code: HTMLElement): Array<{ text: string; color: string | null }> {
  const walker = document.createTreeWalker(code, NodeFilter.SHOW_TEXT);
  const runs: Array<{ text: string; color: string | null }> = [];
  for (let node = walker.nextNode(); node; node = walker.nextNode()) {
    if (!node.textContent?.trim()) continue;
    let el: HTMLElement | null = node.parentElement;
    let color: string | null = null;
    while (el && !color) {
      color = el.style.color || null;
      if (el === code) break;
      el = el.parentElement;
    }
    runs.push({ text: node.textContent, color });
  }
  return runs;
}

describe('code themes: every colour meets WCAG AA on its own block background', () => {
  for (const name of THEMES) {
    it(`${name}: each token class, and the base colour of untokenised text`, () => {
      const theme = CODE_THEMES[name];
      const bg = codeBackground(name);
      expect(theme['pre[class*="language-"]'].background).toBe(bg);
      const coloured = Object.entries(theme).filter(([, s]) => typeof s.color === 'string');
      // the classes an identifier-heavy block actually paints must all be covered
      for (const cls of [
        'code[class*="language-"]',
        'keyword',
        'function',
        'string',
        'comment',
        'number',
      ]) {
        expect(
          coloured.map(([k]) => k),
          cls
        ).toContain(cls);
      }
      const failing = coloured
        .map(([cls, s]) => ({
          cls,
          color: s.color as string,
          ratio: contrast(s.color as string, bg),
        }))
        .filter((r) => r.ratio < AA);
      expect(failing, `token classes under ${AA}:1 on ${bg}`).toEqual([]);
    });

    it(`${name}: nothing in the theme is italic`, () => {
      const italic = Object.entries(CODE_THEMES[name]).filter(([, s]) => s.fontStyle === 'italic');
      expect(italic.map(([k]) => k)).toEqual([]);
      expect(CODE_THEMES[name]['code[class*="language-"]'].fontStyle).toBe('normal');
      expect(CODE_THEMES[name]['pre[class*="language-"]'].fontStyle).toBe('normal');
    });
  }
});

describe('a rendered block paints every run from inside itself, in both themes', () => {
  for (const name of THEMES) {
    it(`${name}: identifiers, self and argument lists carry a readable colour of the block's own`, async () => {
      setDocumentTheme(name);
      const { container } = render(<MarkdownContent content={PYTHON} />, {
        wrapper: IntlTestWrapper,
      });
      const { root, pre, code } = await renderedBlock(container);

      expect(root.getAttribute('data-code-block')).toBe(name);
      const bg = toHex(pre.style.backgroundColor || pre.style.background);
      expect(bg).toBe(codeBackground(name));

      // the highlighter really tokenised (the keyword wears the theme's keyword colour), and
      // untokenised runs exist too — the ones the screenshot showed near-black
      expect(toHex(spanOf(code, 'class').style.color)).toBe(CODE_THEMES[name].keyword.color);
      const runs = paintedRuns(code);
      expect(runs.some((r) => r.text.includes('self'))).toBe(true);

      const unpainted = runs.filter((r) => r.color === null).map((r) => r.text);
      expect(unpainted, 'runs whose colour would come from OUTSIDE the block').toEqual([]);
      const unreadable = runs
        .map((r) => ({
          text: r.text,
          color: r.color as string,
          ratio: contrast(r.color as string, bg),
        }))
        .filter((r) => r.ratio < AA);
      expect(unreadable).toEqual([]);

      // the fence's own backticks never reach the text
      expect(code.textContent?.startsWith('class Ledger:')).toBe(true);
      expect(code.textContent?.includes('`')).toBe(false);
    });
  }

  it('switching the app theme switches the block theme', async () => {
    setDocumentTheme('light');
    const first = render(<MarkdownContent content={PYTHON} />, { wrapper: IntlTestWrapper });
    const light = await renderedBlock(first.container);
    first.unmount();
    setDocumentTheme('dark');
    const second = render(<MarkdownContent content={PYTHON} />, { wrapper: IntlTestWrapper });
    const dark = await renderedBlock(second.container);
    expect(toHex(light.code.style.color)).not.toBe(toHex(dark.code.style.color));
  });
});

describe('code is never italic, even inside the italic Thinking section', () => {
  it('the block under ThinkingContent sets an upright face on pre, code and every token', async () => {
    setDocumentTheme('light');
    const { container } = render(<ThinkingContent content={PYTHON} isExpanded />, {
      wrapper: IntlTestWrapper,
    });
    const { root, pre, code } = await renderedBlock(container);
    // the premise: the block really sits inside the italic wrapper
    expect(root.closest('.italic')).not.toBeNull();
    expect(pre.style.fontStyle).toBe('normal');
    expect(code.style.fontStyle).toBe('normal');
    const italicTokens = Array.from(code.querySelectorAll<HTMLElement>('span')).filter(
      (s) => s.style.fontStyle === 'italic'
    );
    expect(italicTokens.map((s) => s.textContent)).toEqual([]);
    // the comment token is upright too (One Dark set comments in italic)
    const comment = spanOf(code, '# one entry per call');
    expect(comment.style.color).not.toBe('');
    expect(comment.style.fontStyle).toBe('normal');
  });

  it('inline code keeps an upright face (main.css .bg-inline-code)', () => {
    const css = readFileSync(resolve(__dirname, '../styles/main.css'), 'utf8');
    const rule = /\.bg-inline-code\s*\{([^}]*)\}/.exec(css)?.[1] ?? '';
    expect(rule).toMatch(/font-style:\s*normal/);
  });
});

describe('the prose cascade cannot reach the block', () => {
  it('every Typography rule on code/pre excludes not-prose, and the block root is not-prose', async () => {
    const base = resolve(__dirname, '../styles');
    const css = readFileSync(resolve(base, 'main.css'), 'utf8');
    const design = await loadDesignSystem(css, { base });
    const source = readFileSync(resolve(__dirname, 'MarkdownContent.tsx'), 'utf8');
    const proseUtilities = Array.from(
      new Set(source.match(/\b(?:dark:)?prose(?:-[a-z]+)?(?::[\w-[\]!]+)?/g) ?? [])
    );
    expect(proseUtilities).toContain('prose');
    const compiled = design.candidatesToCss(proseUtilities).filter((c): c is string => c != null);
    const selectors = compiled
      .flatMap((c) => c.split('\n'))
      .filter(
        (line) => line.trimEnd().endsWith('{') && /:where\((?:[^)]*\s)?(code|pre)\b/.test(line)
      );
    expect(selectors.length, 'Typography emitted code/pre rules').toBeGreaterThan(0);
    const leaking = selectors.filter((s) => !s.includes('not-prose'));
    expect(leaking).toEqual([]);

    setDocumentTheme('light');
    const { container } = render(<MarkdownContent content={PYTHON} />, {
      wrapper: IntlTestWrapper,
    });
    const { root } = await renderedBlock(container);
    expect(root.classList.contains('not-prose')).toBe(true);
  }, 30_000);
});
