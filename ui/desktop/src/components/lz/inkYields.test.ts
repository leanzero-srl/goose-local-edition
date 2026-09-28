import { readdirSync, readFileSync } from 'node:fs';
import { join, resolve } from 'node:path';
import { __unstable__loadDesignSystem as loadDesignSystem } from '@tailwindcss/node';
import { describe, expect, it } from 'vitest';
import { resolvedPaint, studioToken } from './resolvedPaint';
import { TONE_TEXT_TOKEN, tonesThatDoNotPaint } from './tonePaint';
import { TONE_TEXT, TONES, TYPE, cx } from './tokens';

/**
 * Q-247 — THE BASE INK YIELDS. Every TYPE step carries its own ink, so an error line is written
 * `cx(TYPE.meta, TONE_TEXT.err)` and the element carries TWO colour utilities. Which one paints is
 * decided by the STYLESHEET order, never by the class attribute; before this, the compiled CSS put
 * text-lz-ink* after text-lz-accent and text-lz-err, and every such line in the app painted grey
 * (measured in Chromium: 48 of 108 ink × colour pairs per theme painted the ink).
 *
 * These tests read the real compiled CSS: every text colour the source names must sort after the
 * base inks under every variant, every `*-lz-ink*` class the source names must still compile, and a
 * TYPE step beside a tone must PAINT the tone in both themes.
 */

const stylesDir = resolve(__dirname, '../../styles');
const srcDir = resolve(__dirname, '../..');
const INK = /^text-lz-ink(-\d)?$/;

function sourceFiles(dir: string, out: string[] = []): string[] {
  for (const entry of readdirSync(dir, { withFileTypes: true })) {
    const path = join(dir, entry.name);
    if (entry.isDirectory()) sourceFiles(path, out);
    else if (/\.(tsx?|html)$/.test(entry.name) && !/\.(test|spec)\./.test(entry.name))
      out.push(path);
  }
  return out;
}

function variantOf(cls: string): string {
  let depth = 0;
  for (let i = cls.length - 1; i >= 0; i--) {
    if (cls[i] === ']') depth++;
    else if (cls[i] === '[') depth--;
    else if (cls[i] === ':' && depth === 0) return cls.slice(0, i + 1);
  }
  return '';
}

/** Every class-shaped token in the source (the same over-approximation Tailwind's scanner makes). */
const sourceTokens = (() => {
  const tokens = new Set<string>();
  for (const file of sourceFiles(srcDir)) {
    for (const m of readFileSync(file, 'utf8').matchAll(/[A-Za-z0-9_:!\-[\]#.%/=()&>*+~@]+/g))
      tokens.add(m[0]);
  }
  return [...tokens];
})();

let designPromise: ReturnType<typeof loadDesignSystem> | null = null;
function design() {
  return (designPromise ??= loadDesignSystem(readFileSync(resolve(stylesDir, 'main.css'), 'utf8'), {
    base: stylesDir,
  }));
}

describe('Q-247 — the base ink yields to every other text colour, in the compiled CSS', () => {
  it('every text colour the source names sorts after the base inks, under every variant', async () => {
    const system = await design();
    const candidates = sourceTokens.filter((c) => {
      const bare = c.slice(variantOf(c).length);
      return bare.startsWith('text-');
    });
    const css = system.candidatesToCss(candidates);
    const colours = candidates.filter((_, i) => css[i] != null && /(?<![\w-])color:/.test(css[i]!));
    const byVariant = new Map<string, string[]>();
    for (const c of colours)
      byVariant.set(variantOf(c), [...(byVariant.get(variantOf(c)) ?? []), c]);

    const base = byVariant.get('') ?? [];
    for (const named of [
      'text-lz-ink-3',
      'text-lz-err',
      'text-lz-accent',
      'text-lz-accent-ink',
      'text-lz-ok',
    ]) {
      expect(base, `${named} is named by the source`).toContain(named);
    }

    const violations: string[] = [];
    for (const [variant, group] of byVariant) {
      const inks = group.filter((c) => INK.test(c.slice(variant.length)));
      if (inks.length === 0) continue;
      const order = new Map(system.getClassOrder(group));
      const lastInk = inks.reduce((max, c) => (order.get(c)! > order.get(max)! ? c : max));
      for (const c of group) {
        const bare = c.slice(variant.length);
        if (INK.test(bare) || bare.startsWith('!')) continue;
        if (order.get(c)! < order.get(lastInk)!) violations.push(`${c} sorts before ${lastInk}`);
      }
    }
    expect(violations).toEqual([]);
  }, 60_000);

  it('every *-lz-ink* class the source names still compiles (ink is registered per namespace)', async () => {
    const system = await design();
    const inkClasses = sourceTokens.filter((c) =>
      /^[a-z]+(-[a-z]+)*-lz-ink(-\d)?$/.test(c.slice(variantOf(c).length))
    );
    expect(inkClasses).toEqual(
      expect.arrayContaining(['text-lz-ink', 'bg-lz-ink', 'border-lz-ink', 'fill-lz-ink-3'])
    );
    const css = system.candidatesToCss(inkClasses);
    expect(inkClasses.filter((_, i) => css[i] == null)).toEqual([]);
  }, 60_000);

  it('a TYPE step beside a tone PAINTS the tone, in both themes and in either class order', async () => {
    const wrong: string[] = [];
    for (const theme of ['light', 'dark'] as const) {
      const meta = document.createElement('span');
      meta.className = TYPE.meta;
      expect((await resolvedPaint(meta, theme)).text).toBe(studioToken('--color-lz-ink-3', theme));
      for (const [step, typeClasses] of Object.entries(TYPE)) {
        for (const tone of TONES) {
          const want = studioToken(TONE_TEXT_TOKEN[tone], theme);
          for (const className of [
            cx(typeClasses, TONE_TEXT[tone]),
            cx(TONE_TEXT[tone], typeClasses),
          ]) {
            const el = document.createElement('span');
            el.className = className;
            const got = (await resolvedPaint(el, theme)).text;
            if (got !== want)
              wrong.push(`${theme} "${className}" (TYPE.${step}) painted ${got}, want ${want}`);
          }
        }
      }
    }
    expect(wrong).toEqual([]);
  }, 60_000);

  it('the after-each paint check refuses a tone that loses, and two tones on one element', async () => {
    const root = document.createElement('div');
    const line = (className: string) => {
      const el = document.createElement('p');
      el.className = className;
      el.textContent = className;
      root.appendChild(el);
    };
    line(cx(TYPE.meta, TONE_TEXT.err));
    line(cx(TYPE.body, TONE_TEXT.accent));
    expect(await tonesThatDoNotPaint(root)).toEqual([]);
    line(cx(TONE_TEXT.err, 'text-white'));
    line(cx(TONE_TEXT.ok, TONE_TEXT.warn));
    const wrong = await tonesThatDoNotPaint(root);
    expect(wrong).toHaveLength(2);
    expect(wrong[0]).toMatch(/paints #ffffff, not its err/);
    expect(wrong[1]).toMatch(/names two tones \(ok \+ warn\)/);
  }, 60_000);
});
