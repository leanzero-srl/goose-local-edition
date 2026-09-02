import { readFileSync } from 'node:fs';
import { resolve } from 'node:path';
import { __unstable__loadDesignSystem as loadDesignSystem } from '@tailwindcss/node';
import { darkTokens, lightTokens } from '../../theme/theme-tokens';

/**
 * What a rendered element is actually PAINTED, per theme — the test-side answer to "is the
 * selected row visible?". A class list is compiled through the real Tailwind pipeline, the
 * `background-color` / `color` / ring declarations are pulled out of the emitted rules, and every
 * `var()` chain is walked through the cascade the element sits in: the runtime host tokens
 * (theme-tokens.ts, applied as inline styles on <html>), then `.dark.local-edition`,
 * `.local-edition`, `.dark`, `:root` (which is where `@theme` variables land), and finally the
 * `@theme inline` registry for tokens Tailwind inlines rather than emits.
 *
 * The Memories/Skills case this was born from: `bg-background-accent` compiled to NOTHING, so
 * the row kept no fill at all while `text-white` painted the words — white on the light page.
 * A class that compiles to nothing is reported in `missing`; a colour that resolves to anything
 * but a solid hex (a color-mix tint, `transparent`) is reported as-is so the contrast assertion
 * refuses it.
 *
 * Variant model (deliberately pessimistic): base classes apply first, `data-[state=active]:`
 * when the element carries data-state="active", `aria-selected:`/`aria-current:` when the
 * attribute is set, and `hover:` LAST when `hover` is asked for — so a selected row that still
 * carries the neutral hover step resolves to that step under the pointer, which is exactly the
 * "selected must always win over hover" rule the tests pin.
 */
export type Theme = 'light' | 'dark';

export interface ResolvedPaint {
  bg: string | null;
  text: string | null;
  ring: string | null;
  /** Classes that produced no rule at all. */
  missing: string[];
}

const base = resolve(__dirname, '../../styles');
const css = readFileSync(resolve(base, 'main.css'), 'utf8');

type Decls = Record<string, string>;

/** Top-level blocks of main.css by selector: `:root`, `.dark`, `.local-edition`, … and the two @theme kinds. */
function collectBlocks(source: string): Record<string, Decls> {
  const out: Record<string, Decls> = {};
  let i = 0;
  while (i < source.length) {
    const open = source.indexOf('{', i);
    if (open < 0) break;
    // the selector is the text between the previous block end (or comment end) and this brace
    let selStart = open - 1;
    while (selStart >= 0 && !'{};'.includes(source[selStart]) && source.slice(selStart - 1, selStart + 1) !== '*/') selStart--;
    const selector = source.slice(selStart + 1, open).trim();
    // find the matching close brace, counting nested blocks (@keyframes inside @theme)
    let depth = 1;
    let j = open + 1;
    while (j < source.length && depth > 0) {
      if (source[j] === '{') depth++;
      else if (source[j] === '}') depth--;
      j++;
    }
    const body = source.slice(open + 1, j - 1);
    const key = selector.startsWith('@theme') ? (selector.includes('inline') ? '@theme inline' : '@theme') : selector;
    if (['@theme', '@theme inline', ':root', '.dark', '.local-edition', '.dark.local-edition'].includes(key)) {
      const decls = (out[key] ??= {});
      for (const m of body.matchAll(/(--[a-z0-9-]+)\s*:\s*([^;{}]+);/g)) {
        decls[m[1]] = m[2].replace(/!important/g, '').trim();
      }
    }
    i = j;
  }
  return out;
}

const blocks = collectBlocks(css);

function cascade(theme: Theme): Decls[] {
  const host = theme === 'dark' ? darkTokens : lightTokens;
  return theme === 'dark'
    ? [host as unknown as Decls, blocks['.dark.local-edition'] ?? {}, blocks['.local-edition'] ?? {}, blocks['.dark'] ?? {}, blocks[':root'] ?? {}, blocks['@theme'] ?? {}]
    : [host as unknown as Decls, blocks['.local-edition'] ?? {}, blocks[':root'] ?? {}, blocks['@theme'] ?? {}];
}

function splitTopLevelComma(s: string): [string, string | undefined] {
  let depth = 0;
  for (let k = 0; k < s.length; k++) {
    if (s[k] === '(') depth++;
    else if (s[k] === ')') depth--;
    else if (s[k] === ',' && depth === 0) return [s.slice(0, k).trim(), s.slice(k + 1).trim()];
  }
  return [s.trim(), undefined];
}

/** Resolve a CSS colour expression to a solid hex in the given theme, or return it verbatim. */
export function resolveExpr(expr: string, theme: Theme, depth = 0): string {
  const e = expr.trim();
  if (depth > 24) throw new Error(`var() chain too deep at ${e}`);
  if (/^#[0-9a-f]{6}$/i.test(e)) return e.toLowerCase();
  const m = /^var\(\s*(--[a-z0-9-]+)\s*(?:,(.*))?\)$/is.exec(e);
  if (!m) return e;
  const name = m[1];
  const fallback = m[2] !== undefined ? m[2].trim() : undefined;
  for (const layer of cascade(theme)) {
    const v = layer[name];
    if (v !== undefined && !refersTo(v, name)) return resolveExpr(v, theme, depth + 1);
  }
  const inline = blocks['@theme inline']?.[name];
  if (inline !== undefined && !refersTo(inline, name)) return resolveExpr(inline, theme, depth + 1);
  if (inline !== undefined && refersTo(inline, name)) {
    const [, fb] = splitTopLevelComma(inline.replace(/^var\(/, '').replace(/\)$/, ''));
    if (fb !== undefined) return resolveExpr(fb, theme, depth + 1);
  }
  if (fallback !== undefined) return resolveExpr(fallback, theme, depth + 1);
  throw new Error(`unresolved ${name} in ${theme}`);
}

function refersTo(value: string, name: string): boolean {
  return new RegExp(`var\\(\\s*${name}(?![a-z0-9-])`).test(value);
}

/** A Studio token by name (`--color-lz-accent`), resolved for the theme. */
export function studioToken(name: string, theme: Theme): string {
  return resolveExpr(`var(${name})`, theme);
}

let designPromise: ReturnType<typeof loadDesignSystem> | null = null;
function design() {
  return (designPromise ??= loadDesignSystem(css, { base }));
}

const VARIANT_ORDER = ['', 'aria-selected:', 'aria-current:', 'data-[state=active]:', 'hover:'] as const;

function variantOf(cls: string): (typeof VARIANT_ORDER)[number] | 'other' {
  for (const v of VARIANT_ORDER) if (v && cls.startsWith(v)) return v;
  if (cls.includes(':')) return 'other';
  return '';
}

/**
 * The element's own classes resolved for the theme. `inherit` supplies what the element takes
 * from its container when it sets no fill/ink of its own (a row inside a `bg-lz-surface` panel).
 */
export async function resolvedPaint(
  el: HTMLElement,
  theme: Theme,
  opts: { hover?: boolean; inherit?: { bg?: string; text?: string } } = {}
): Promise<ResolvedPaint> {
  const classes = Array.from(el.classList);
  const compiled = (await design()).candidatesToCss(classes);
  const active =
    el.getAttribute('data-state') === 'active' ||
    el.getAttribute('aria-selected') === 'true' ||
    el.getAttribute('aria-current') != null;
  const paint: ResolvedPaint = {
    bg: opts.inherit?.bg ?? null,
    text: opts.inherit?.text ?? null,
    ring: null,
    missing: [],
  };
  const applicable: Array<{ order: number; rule: string }> = [];
  classes.forEach((cls, i) => {
    const rule = compiled[i];
    if (rule == null) {
      paint.missing.push(cls);
      return;
    }
    const v = variantOf(cls);
    if (v === 'other') return;
    if (v === 'hover:' && !opts.hover) return;
    if ((v === 'aria-selected:' || v === 'aria-current:' || v === 'data-[state=active]:') && !active) return;
    applicable.push({ order: VARIANT_ORDER.indexOf(v), rule });
  });
  applicable.sort((a, b) => a.order - b.order);
  for (const { rule } of applicable) {
    const bg = /background-color:\s*([^;]+);/.exec(rule);
    if (bg) paint.bg = resolveExpr(bg[1], theme);
    const text = /(?<![\w-])color:\s*([^;]+);/.exec(rule);
    if (text) paint.text = resolveExpr(text[1], theme);
    const ring = /--tw-ring-color:\s*([^;]+);/.exec(rule);
    if (ring) paint.ring = resolveExpr(ring[1], theme);
  }
  return paint;
}

function luminance(hex: string): number {
  const c = [1, 3, 5].map((i) => parseInt(hex.slice(i, i + 2), 16) / 255);
  const lin = c.map((v) => (v <= 0.03928 ? v / 12.92 : ((v + 0.055) / 1.055) ** 2.4));
  return 0.2126 * lin[0] + 0.7152 * lin[1] + 0.0722 * lin[2];
}

/** WCAG contrast ratio of two solid hexes; anything that is not a solid hex is 1 (invisible). */
export function contrast(a: string | null, b: string | null): number {
  if (!a || !b || !/^#[0-9a-f]{6}$/i.test(a) || !/^#[0-9a-f]{6}$/i.test(b)) return 1;
  const [hi, lo] = [luminance(a), luminance(b)].sort((x, y) => y - x);
  return (hi + 0.05) / (lo + 0.05);
}
