import { readdirSync, readFileSync } from 'node:fs';
import { join, relative, resolve } from 'node:path';
import { describe, expect, it } from 'vitest';

/**
 * Q-474 — NO LEFT ACCENT RAIL, source-wide (owner rule: "never a left accent line / left coloured
 * rail / left border-strip on cards, panels, list items, callouts"). The recipe form's Advanced
 * Options shipped `pl-6 border-l-2 border-border-primary` — a 2px stripe hung off the left edge of
 * the section — and assertStudioClean could not see it, because it only runs over the renders of
 * the Studio primitive tests. This reads every source file instead.
 *
 * Refused: a left/inline-start border with a WIDTH (`border-l-2`, `border-l-[3px]`, `border-s-4`),
 * a left border with a COLOUR of its own (`border-l-lz-accent`, `border-l-red-500`), a bare
 * `border-l` beside a hue-coloured border class, an inline `borderLeft` / `border-left` /
 * `border-inline-start` that paints, and an inset left box-shadow stripe.
 *
 * Allowed: the 1px NEUTRAL divider `border-l border-border-*` / `border-lz-border` between two
 * columns (structure, not decoration), and `border-l-transparent`.
 */

const SRC = resolve(__dirname, '../..');

function sourceFiles(dir: string, out: string[] = []): string[] {
  for (const entry of readdirSync(dir, { withFileTypes: true })) {
    const path = join(dir, entry.name);
    if (entry.isDirectory()) {
      if (entry.name !== 'node_modules' && entry.name !== '__tests__') sourceFiles(path, out);
    } else if (/\.(tsx?|css)$/.test(entry.name) && !/\.(test|spec)\.tsx?$/.test(entry.name)) {
      out.push(path);
    }
  }
  return out;
}

const VARIANTS = String.raw`(?:[\w-]+(?:\[[^\]]*\])?:)*`;
const SIDE = String.raw`border-(?:l|s)`;
/** A left border given its own width or colour. */
const RAIL_CLASS = new RegExp(
  String.raw`(?:^|[\s"'\`])${VARIANTS}!?${SIDE}-(?!transparent(?=[\s"'\`]|$))[\w#[\]().%-]+`,
  'g'
);
const BARE_SIDE = new RegExp(String.raw`(?:^|[\s"'\`])${VARIANTS}${SIDE}(?=[\s"'\`]|$)`);
/** A border colour that is a HUE, not a neutral line. */
const HUED_BORDER =
  /(?:^|[\s"'`])(?:[\w-]+:)*border-(?:lz-(?:accent|ok|warn|err|secondary|stopped|node)[\w-]*|(?:red|orange|amber|yellow|lime|green|emerald|teal|cyan|sky|blue|indigo|violet|purple|fuchsia|pink|rose)-\d+|\[#[0-9a-f]+\]|(?:block-teal|accent-local|action-solid|node-\d)|border-(?:danger|info|warning|success))\b/;
const INLINE_RAIL =
  /\bborder(?:Left|InlineStart)(?:Width|Color|Style)?\s*:\s*(?!['"`]?(?:0|none)\b)|\bborder-(?:left|inline-start)(?:-width|-color|-style)?\s*:\s*(?!0\b|none\b)|inset\s+-?\d+px\s+0\s+0/;

interface Hit {
  where: string;
  what: string;
}

function railsIn(file: string, text: string): Hit[] {
  const hits: Hit[] = [];
  text.split('\n').forEach((line, i) => {
    if (/^\s*(\*|\/\/|\/\*)/.test(line)) return;
    const where = `${file}:${i + 1}`;
    for (const m of line.matchAll(RAIL_CLASS)) hits.push({ where, what: m[0].trim() });
    if (BARE_SIDE.test(line) && HUED_BORDER.test(line)) {
      hits.push({ where, what: `border-l beside ${HUED_BORDER.exec(line)![0].trim()}` });
    }
    if (INLINE_RAIL.test(line)) hits.push({ where, what: INLINE_RAIL.exec(line)![0].trim() });
  });
  return hits;
}

describe('no left accent rail anywhere in the app (Q-474)', () => {
  it('the detector refuses every rail shape and passes the neutral divider (fixture)', () => {
    const refused = [
      '<div className="pl-6 border-l-2 border-border-primary ml-2">',
      '<div className="border-l-4 border-lz-accent">',
      '<li className="hover:border-l-[3px]">',
      '<div className="border-s-2">',
      '<div className="border-l-lz-err">',
      '<div className="border-l border-blue-500 pl-3">',
      "<div style={{ borderLeft: '3px solid #1d4ed8' }}>",
      '  border-left: 4px solid var(--color-lz-accent);',
      '  box-shadow: inset 3px 0 0 var(--color-lz-accent);',
    ];
    for (const line of refused) expect(railsIn('fixture', line), line).not.toEqual([]);
    const allowed = [
      '<div className="ml-3 border-l border-lz-border pl-3">',
      "i > 0 ? 'border-l border-border-primary' : ''",
      "'h-full w-2.5 border-l border-l-transparent p-[1px]'",
      '<div className="border-lz-border rounded-lg border">',
    ];
    for (const line of allowed) expect(railsIn('fixture', line), line).toEqual([]);
  });

  it('no source file draws one', () => {
    const hits = sourceFiles(SRC).flatMap((file) =>
      railsIn(relative(SRC, file), readFileSync(file, 'utf8'))
    );
    expect(hits.map((h) => `${h.where} ${h.what}`)).toEqual([]);
  });
});
