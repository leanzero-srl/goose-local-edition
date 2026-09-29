import { readdirSync, readFileSync } from 'node:fs';
import { join, relative, resolve } from 'node:path';
import ts from 'typescript';
import { describe, expect, it } from 'vitest';
import { contrast, oklchToHex, resolvedPaint, type Theme } from './resolvedPaint';
import * as tokens from './tokens';

/**
 * Q-457 reopened — INK ON A FILL, source-wide. Q-457 turned 110 washes into solid fills and left
 * the words inside some of them in the fill's own hue: "Recipe parsed" became green-600 text on the
 * ok-solid green (1.4:1), and three schedule error boxes painted text-danger on background-danger —
 * the SAME hex, 1:1. The per-token contrast test could not see either, because each token is fine
 * on its own; the defect is the PAIR, and the pair only exists in the JSX tree.
 *
 * So this walks every component's JSX: an element's fill (its own `bg-*`, else the nearest
 * enclosing one in the same tree) against its ink (its own `text-*`, else the nearest enclosing
 * one), both resolved through the real Tailwind pipeline per theme (`dark:` applied in dark), is
 * weighed where ink is actually painted — words at 4.5:1, an icon at 3:1 — at rest, and under the
 * pointer when the element carries a `hover:` fill. REFUSED outright: ink in its fill's own hue,
 * and anything under 3:1. Every other pair under its bar rides a count that may only fall.
 *
 * Only UNCONDITIONAL classes are weighed (a literal className, the literal arms of cx/cn/clsx, a
 * template's static text, a Studio token like TONE_FILL.err); a ternary's branches are
 * alternatives, not a pair, and a fill or ink the tree cannot see (inherited from another
 * component) is unknown, never guessed.
 */

const SRC = resolve(__dirname, '../..');

function componentFiles(dir: string, out: string[] = []): string[] {
  for (const entry of readdirSync(dir, { withFileTypes: true })) {
    const path = join(dir, entry.name);
    if (entry.isDirectory()) {
      if (entry.name !== 'node_modules' && entry.name !== '__tests__') componentFiles(path, out);
    } else if (/\.tsx$/.test(entry.name) && !/\.(test|spec)\.tsx$/.test(entry.name)) {
      out.push(path);
    }
  }
  return out;
}

const TOKEN_MAPS = tokens as unknown as Record<string, unknown>;

/** The classes an expression ALWAYS contributes. */
function staticClasses(expr: ts.Expression | undefined): string[] {
  if (!expr) return [];
  if (ts.isParenthesizedExpression(expr)) return staticClasses(expr.expression);
  if (ts.isStringLiteral(expr) || ts.isNoSubstitutionTemplateLiteral(expr)) {
    return expr.text.split(/\s+/).filter(Boolean);
  }
  if (ts.isTemplateExpression(expr)) {
    // Static text only; a class glued to a substitution (`bg-${x}`) is not a whole class.
    const parts = [expr.head.text, ...expr.templateSpans.map((s) => s.literal.text)];
    const out: string[] = [];
    parts.forEach((text, i) => {
      const words = text.split(/\s+/);
      words.forEach((w, j) => {
        const gluedLeft = j === 0 && i > 0 && !/^\s/.test(text);
        const gluedRight = j === words.length - 1 && i < parts.length - 1 && !/\s$/.test(text);
        if (w && !gluedLeft && !gluedRight) out.push(w);
      });
    });
    return out;
  }
  if (ts.isCallExpression(expr) && ts.isIdentifier(expr.expression)) {
    if (['cx', 'cn', 'clsx', 'classNames'].includes(expr.expression.text)) {
      return expr.arguments.flatMap((a) => staticClasses(a));
    }
    return [];
  }
  if (ts.isPropertyAccessExpression(expr) && ts.isIdentifier(expr.expression)) {
    const map = TOKEN_MAPS[expr.expression.text];
    if (map && typeof map === 'object') {
      const v = (map as Record<string, unknown>)[expr.name.text];
      if (typeof v === 'string') return v.split(/\s+/).filter(Boolean);
    }
    return [];
  }
  if (ts.isIdentifier(expr)) {
    const v = TOKEN_MAPS[expr.text];
    return typeof v === 'string' ? v.split(/\s+/).filter(Boolean) : [];
  }
  return [];
}

function classNameOf(attrs: ts.JsxAttributes): string[] {
  for (const p of attrs.properties) {
    if (!ts.isJsxAttribute(p) || p.name.getText() !== 'className' || !p.initializer) continue;
    if (ts.isStringLiteral(p.initializer)) return staticClasses(p.initializer);
    if (ts.isJsxExpression(p.initializer)) return staticClasses(p.initializer.expression);
  }
  return [];
}

const COLOUR_BG = /^(hover:)?bg-(?!opacity|clip|origin|none|repeat|cover|contain|center|fixed|local|scroll|linear|radial|conic|gradient|\[url)/;
const COLOUR_TEXT =
  /^(hover:)?text-(?!(xs|sm|base|lg|xl|\d?xl|left|right|center|justify|start|end|wrap|nowrap|balance|pretty|ellipsis|clip|lz-(body|meta|title|display|label|caption|mono|small|heading|h\d))$)(?!\[\d)/;

interface Pair {
  where: string;
  theme: Theme;
  state: 'rest' | 'hover';
  bg: string;
  text: string;
  ratio: number;
  /** 4.5 where the element paints words, 3 where it paints a glyph (an icon component). */
  need: number;
}

/**
 * Where the ink is actually PAINTED at this element: words among its own children (4.5:1), or a
 * self-closing component — an icon, drawn in currentColor (3:1, WCAG 1.4.11). A swatch or dot with
 * no content, or a wrapper whose children set their own ink, paints no ink here (null).
 */
function inkNeed(node: ts.Node, opening: ts.JsxOpeningLikeElement): number | null {
  if (ts.isJsxSelfClosingElement(node)) {
    const tag = opening.tagName.getText();
    if (/^(input|textarea)$/.test(tag)) return 4.5;
    return /^[A-Z]/.test(tag) ? 3 : null;
  }
  if (!ts.isJsxElement(node)) return null;
  const words = node.children.some(
    (c) => (ts.isJsxText(c) && c.text.trim() !== '') || (ts.isJsxExpression(c) && !!c.expression)
  );
  return words ? 4.5 : null;
}

interface Ctx {
  bg: Record<Theme, string | null>;
  text: Record<Theme, string | null>;
}

const HEX = /^#[0-9a-f]{6}$/i;

function colourClasses(classes: string[]): string[] {
  return classes.filter((c) => COLOUR_BG.test(c) || COLOUR_TEXT.test(c));
}

/**
 * Layered the way the compiled sheet cascades them: base, then `dark:` (in dark), then `hover:`,
 * then `dark:hover:`. MEASURED on the 3.0.76 walk (q457-schedule-detail-edit-hover-dark.png):
 * `dark:text-blue-400 hover:bg-lz-accent hover:text-white` paints WHITE under the pointer in dark,
 * so hover beats a plain `dark:`; and `bg-white dark:bg-gray-800` is gray in dark whatever the two
 * utilities' own sort order says.
 */
async function paintOf(classes: string[], theme: Theme, ctx: Ctx, hover: boolean) {
  const isDark = (c: string) => c.startsWith('dark:');
  const isHover = (c: string) => c.replace(/^dark:/, '').startsWith('hover:');
  const layers = [
    classes.filter((c) => !isDark(c) && !isHover(c)),
    theme === 'dark' ? classes.filter((c) => isDark(c) && !isHover(c)) : [],
    hover ? classes.filter((c) => !isDark(c) && isHover(c)) : [],
    hover && theme === 'dark' ? classes.filter((c) => isDark(c) && isHover(c)) : [],
  ];
  let bg = ctx.bg[theme] ?? undefined;
  let text = ctx.text[theme] ?? undefined;
  for (const layer of layers) {
    const own = colourClasses(layer.map((c) => c.replace(/^dark:/, '')));
    if (!own.length) continue;
    const el = document.createElement('div');
    el.className = own.join(' ');
    const painted = await resolvedPaint(el, theme, { hover, inherit: { bg, text } });
    bg = painted.bg ?? undefined;
    text = painted.text ?? undefined;
  }
  return { bg: bg ?? null, text: text ?? null };
}

/** A fill set by an inline style is invisible to the class pipeline — the fill is unknown here. */
function hasInlineFill(attrs: ts.JsxAttributes): boolean {
  return attrs.properties.some(
    (p) =>
      ts.isJsxAttribute(p) &&
      p.name.getText() === 'style' &&
      /\bbackground(Color)?\s*:/.test(p.initializer?.getText() ?? '')
  );
}

async function scan(file: string, text: string): Promise<Pair[]> {
  if (!/\b(bg|text)-/.test(text)) return [];
  const source = ts.createSourceFile(file, text, ts.ScriptTarget.Latest, true, ts.ScriptKind.TSX);
  const pairs: Pair[] = [];
  const root: Ctx = { bg: { light: null, dark: null }, text: { light: null, dark: null } };

  async function visit(node: ts.Node, ctx: Ctx): Promise<void> {
    let next = ctx;
    const opening = ts.isJsxElement(node)
      ? node.openingElement
      : ts.isJsxSelfClosingElement(node)
        ? node
        : null;
    if (opening) {
      const classes = classNameOf(opening.attributes);
      const ownBg = classes.some((c) => COLOUR_BG.test(c.replace(/^dark:/, '')));
      const ownText = classes.some((c) => COLOUR_TEXT.test(c.replace(/^dark:/, '')));
      const ownHoverBg = classes.some((c) => /^(dark:)?hover:bg-/.test(c));
      if (hasInlineFill(opening.attributes)) {
        next = { bg: { light: null, dark: null }, text: { ...ctx.text } };
      } else if (ownBg || ownText || ownHoverBg) {
        next = { bg: { ...ctx.bg }, text: { ...ctx.text } };
        const line = source.getLineAndCharacterOfPosition(opening.getStart()).line + 1;
        const where = `${relative(SRC, file)}:${line}`;
        const need = inkNeed(node, opening);
        for (const theme of ['light', 'dark'] as const) {
          const rest = await paintOf(classes, theme, ctx, false);
          next.bg[theme] = rest.bg && HEX.test(rest.bg) ? rest.bg : ownBg ? null : ctx.bg[theme];
          next.text[theme] = rest.text && HEX.test(rest.text) ? rest.text : ctx.text[theme];
          const bg = next.bg[theme];
          const ink = next.text[theme];
          if (need && bg && ink && (ownBg || ownText)) {
            pairs.push({
              where,
              theme,
              state: 'rest',
              bg,
              text: ink,
              ratio: contrast(bg, ink),
              need,
            });
          }
          if (need && ownHoverBg) {
            const hover = await paintOf(classes, theme, ctx, true);
            if (hover.bg && HEX.test(hover.bg) && hover.text && HEX.test(hover.text)) {
              pairs.push({
                where,
                theme,
                state: 'hover',
                bg: hover.bg,
                text: hover.text,
                ratio: contrast(hover.bg, hover.text),
                need,
              });
            }
          }
        }
      }
    }
    for (const child of node.getChildren(source)) await visit(child, next);
  }

  await visit(source, root);
  return pairs;
}

/** HSL hue and saturation — enough to say two colours are the same hue family. */
function hueOf(hex: string): { h: number; s: number; l: number } {
  const [r, g, b] = [1, 3, 5].map((i) => parseInt(hex.slice(i, i + 2), 16) / 255);
  const max = Math.max(r, g, b);
  const min = Math.min(r, g, b);
  const l = (max + min) / 2;
  const d = max - min;
  const s = d === 0 ? 0 : d / (1 - Math.abs(2 * l - 1));
  let h = 0;
  if (d) {
    if (max === r) h = ((g - b) / d) % 6;
    else if (max === g) h = (b - r) / d + 2;
    else h = (r - g) / d + 4;
    h = (h * 60 + 360) % 360;
  }
  return { h, s, l };
}

/** Both chromatic and within 40° of hue: green words on a green box, red on red. */
function sameHue(a: string, b: string): boolean {
  const [x, y] = [hueOf(a), hueOf(b)];
  const chromatic = (c: { s: number; l: number }) => c.s > 0.25 && c.l > 0.08 && c.l < 0.95;
  const dh = Math.min(Math.abs(x.h - y.h), 360 - Math.abs(x.h - y.h));
  return chromatic(x) && chromatic(y) && dh <= 40;
}

function describePair(p: Pair): string {
  return `${p.where} ${p.theme} ${p.state}: ${p.text} on ${p.bg} = ${p.ratio.toFixed(2)}:1`;
}

/**
 * Pairs under their bar that are NOT the refused class: mostly the host's own text tokens
 * (--color-text-secondary #878787 is 3.59:1 on white, --color-text-danger #f94b4b 3.42:1) on host
 * surfaces — a token-level debt outside Q-457, filed as its own ledger row. The count may only
 * FALL; a change that lowers it lowers this number in the same commit.
 */
const BELOW_BAR_RATCHET = 200;

async function allPairs(): Promise<Pair[]> {
  const out: Pair[] = [];
  for (const file of componentFiles(SRC)) out.push(...(await scan(file, readFileSync(file, 'utf8'))));
  return out;
}

describe('ink on a fill (Q-457 reopened)', () => {
  it('converts the Tailwind v4 palette the stylesheet paints', () => {
    expect(oklchToHex('oklch(62.7% 0.194 149.214)')).toBe('#00a63e');
    expect(oklchToHex('oklch(98.5% 0 0)')).toBe('#fafafa');
  });

  it('REFUSES the shipped Q-457 shape: green words on the ok-solid box (fixture)', async () => {
    const fixture = `export const X = () => (
      <div className="mt-2 p-2 bg-lz-ok-solid rounded-md">
        <p className="text-xs text-white font-medium">Recipe parsed</p>
        <p className="text-xs text-green-600 dark:text-green-400">Title: {t}</p>
      </div>
    );`;
    const bad = (await scan(join(SRC, 'fixture.tsx'), fixture)).filter(
      (p) => p.ratio < p.need
    );
    expect(bad.map((p) => `${p.theme} ${p.text}`).sort()).toEqual([
      'dark #05df72',
      'light #00a63e',
    ]);
    expect(bad.every((p) => sameHue(p.text, p.bg))).toBe(true);
  });

  it(
    'no pair anywhere paints ink in its own fill\'s hue, or below 3:1, in either theme or under hover',
    async () => {
      const pairs = await allPairs();
      expect(pairs.length, 'the scan found no pairs — it is not reading the tree').toBeGreaterThan(
        1000
      );
      const refused = pairs
        .filter((p) => p.ratio < p.need && (sameHue(p.text, p.bg) || p.ratio < 3))
        .map(describePair);
      expect(refused).toEqual([]);
    },
    120_000
  );

  it(
    'the remaining below-bar pairs only ever shrink (ratchet)',
    async () => {
      const below = (await allPairs()).filter((p) => p.ratio < p.need).map(describePair);
      expect(below.length, below.join('\n')).toBeLessThanOrEqual(BELOW_BAR_RATCHET);
    },
    120_000
  );
});
